//! [`GpuSurface`]: a GPU device, queue and swap chain for a platform surface.

use crate::GpuError;

/// Everything a host needs to draw to a platform surface.
///
/// The parts are created together because each depends on the last: the
/// adapter is chosen for the surface, the device comes from the adapter, and
/// the swap-chain format is picked from what that adapter supports.
pub struct GpuSurface {
    /// The surface frames are presented to.
    pub surface: wgpu::Surface<'static>,
    /// The GPU device.
    pub device: wgpu::Device,
    /// The device's command queue.
    pub queue: wgpu::Queue,
    /// The swap chain's configuration. To resize, change `width` and `height`
    /// and call [`GpuSurface::reconfigure`], or use [`GpuSurface::resize`].
    pub config: wgpu::SurfaceConfiguration,
    /// The chosen GPU, for querying its capabilities and for logging.
    pub adapter: wgpu::Adapter,
    // Not read, but must outlive the surface it created.
    _instance: wgpu::Instance,
}

/// The parts of GPU setup that differ between hosts. Everything else is the
/// same on every platform, and happens in [`GpuSurface::create`].
pub struct SurfaceRequest {
    /// Debug label for the device.
    pub label: &'static str,
    /// The initial swap-chain size in physical pixels. A native host reads it
    /// from the window; the web host multiplies the canvas's CSS size by
    /// `devicePixelRatio`.
    pub size: (u32, u32),
    /// Which GPU to prefer on a machine with more than one.
    pub power_preference: wgpu::PowerPreference,
    /// How frames are synchronized with the display.
    pub present_mode: wgpu::PresentMode,
    /// Device limits. A native host asks for [`wgpu::Limits::default()`], the
    /// web host for [`wgpu::Limits::downlevel_webgl2_defaults()`] so that the
    /// WebGL2 fallback works. Proteus's default settings fit the WebGL2 limits;
    /// larger ones, such as `ProteusConfig::desktop()`, need more.
    pub limits: wgpu::Limits,
}

impl GpuSurface {
    /// Creates the surface, adapter, device and configured swap chain for
    /// `target`.
    ///
    /// `target` is whatever the platform provides: an `Arc<winit::Window>`
    /// natively, or `wgpu::SurfaceTarget::Canvas` on the web.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError`] if the surface can't be created, no GPU supports
    /// it, or the device can't be created.
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

    /// Applies [`GpuSurface::config`] to the surface again: after changing its
    /// size, or to recover when the surface is lost or outdated.
    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }

    /// Sets the swap-chain size in physical pixels and reconfigures. A zero
    /// width or height is ignored: a minimized window reports it, and a
    /// zero-sized surface is invalid.
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

/// Picks a non-sRGB swap-chain format when one is available.
///
/// Proteus colors are already gamma-encoded, and the shader passes them through
/// unchanged. An sRGB swap chain would encode them a second time and wash the
/// whole UI out.
fn preferred_format(caps: &wgpu::SurfaceCapabilities) -> wgpu::TextureFormat {
    caps.formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .unwrap_or(caps.formats[0])
}
