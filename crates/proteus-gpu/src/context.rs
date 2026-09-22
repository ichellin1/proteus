//! [`GpuSurface`] — the one place a Proteus host turns a platform surface
//! target into a live wgpu device, queue and configured swap chain.

use crate::GpuError;

/// Everything a host needs after GPU setup, created together because the steps
/// are interdependent: the adapter is requested *against* the surface, the
/// device comes from the adapter, and the swap-chain format is picked from what
/// that adapter reports the surface can do.
pub struct GpuSurface {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The configured swap chain. A host owns resizing: mutate `width`/`height`
    /// and call [`GpuSurface::reconfigure`].
    pub config: wgpu::SurfaceConfiguration,
    /// Kept for `get_capabilities` and for logging which GPU was chosen.
    pub adapter: wgpu::Adapter,
    /// Not read, but must outlive the surface it created.
    _instance: wgpu::Instance,
}

/// The parts of GPU setup that genuinely differ per host.
///
/// Everything *not* here is identical across platforms and lives in
/// [`GpuSurface::create`] — which is the whole point of this type existing.
pub struct SurfaceRequest {
    /// Debug label for the device.
    pub label: &'static str,
    /// Initial swap-chain size in **physical** pixels. Native reads this from
    /// the window; the web host multiplies its CSS size by `devicePixelRatio`.
    pub size: (u32, u32),
    /// Adapter selection hint (`ProteusConfig::render.power_preference`).
    pub power_preference: wgpu::PowerPreference,
    /// Swap-chain presentation mode (`ProteusConfig::render.present_mode`).
    pub present_mode: wgpu::PresentMode,
    /// Device limits. The real per-platform difference: native asks for
    /// [`wgpu::Limits::default()`], the web host for
    /// [`wgpu::Limits::downlevel_webgl2_defaults()`] so a WebGL2 fallback is
    /// actually usable. The pipeline never relies on anything above the
    /// WebGL2 floor either way — see `PLANNING.md` § M13.3's "GPU floor".
    pub limits: wgpu::Limits,
}

impl GpuSurface {
    /// Create an instance, surface, adapter, device and configured swap chain
    /// for `target`.
    ///
    /// `target` is whatever the platform presents: an `Arc<winit::Window>` on
    /// native, [`wgpu::SurfaceTarget::Canvas`] on the web. Both satisfy
    /// `Into<SurfaceTarget<'static>>`, which is the only shape of the platform
    /// this crate ever sees — no winit, no web-sys, no `#[cfg]`.
    ///
    /// ## Why this lives here
    ///
    /// `proteus-host-winit` and `proteus-host-web` each carried their own copy
    /// of this sequence, near-identical down to the comment explaining the
    /// surface-format choice. That's the duplication `PLANNING.md` § M13.3
    /// ("shared GPU init") scheduled for consolidation, and it's what this
    /// crate was always supposed to hold — before this it declared a
    /// surfaceless `GpuContext` that nothing anywhere constructed, so the
    /// "Layer 0" crate the docs describe wasn't actually in the build graph of
    /// anything.
    pub async fn create(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        request: SurfaceRequest,
    ) -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance.create_surface(target)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: request.power_preference,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| GpuError::NoAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(request.label),
                required_features: wgpu::Features::empty(),
                required_limits: request.limits,
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: preferred_format(&caps),
            width: request.size.0.max(1),
            height: request.size.1.max(1),
            present_mode: request.present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            adapter,
            _instance: instance,
        })
    }

    /// The swap-chain texture format the renderer must target.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    /// Re-apply [`GpuSurface::config`] to the surface — after changing its
    /// `width`/`height`, or to recover from a `Lost`/`Outdated` surface texture
    /// or a restored GPU context.
    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    /// Set the swap-chain size in physical pixels and reconfigure. Zero in
    /// either axis is ignored — a minimized window reports that, and
    /// configuring a zero-sized surface is a wgpu validation error.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.reconfigure();
    }

    /// A short description of the chosen adapter, for host startup logging.
    pub fn adapter_description(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }
}

/// Pick a **non-sRGB** swap-chain format where one is offered.
///
/// Every colour in Proteus is authored as a flat, already-gamma-space value and
/// the fragment shader passes it through untouched, so an sRGB-tagged swap chain
/// would encode it a second time and wash the whole UI out. Both hosts made this
/// same choice with the same comment before it moved here.
fn preferred_format(caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureFormat {
    caps.formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or(caps.formats[0])
}
