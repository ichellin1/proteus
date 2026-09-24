//! [`WebSurface`] — the canvas-specific half of GPU setup: DPI sizing and the
//! CSS-pixel bookkeeping the rest of the host reports as its `Viewport`.
//!
//! The wgpu half — instance, surface, adapter, device, swap-chain format —
//! lives in `proteus-gpu` (`GpuSurface::create`), shared with
//! `proteus-host-winit`; see `PLANNING.md` § M13.3's "shared GPU init". What
//! stays here is genuinely web-only: reading `devicePixelRatio`, sizing the
//! canvas backing store, and tracking logical size separately from physical.
//!
//! M13.2's one deliberate behaviour change from the M12 web shell: canvas
//! backing-store resolution tracks `devicePixelRatio` instead of being 1:1 CSS
//! pixels, so high-DPI displays render sharper. (An earlier note here claimed
//! M6 baselines needed re-capturing for it — that rested on a false premise;
//! see `PLANNING.md` § M13.2's Definition of Done for why M6 is unaffected.)

use proteus_runtime::config::RenderConfig;
use proteus_runtime::wgpu;
use proteus_runtime::{GpuSurface, SurfaceRequest, Viewport};
use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

pub struct WebSurface {
    /// Device, queue, surface and swap-chain config — created by
    /// `proteus-gpu`, identically to the native host.
    gpu: GpuSurface,
    /// CSS pixel size — `Viewport.logical_size` — tracked separately from
    /// `gpu.config.width/height` (physical px) so `resize`/`viewport` don't
    /// need to re-read `devicePixelRatio` (it can change mid-session, e.g.
    /// dragging a window between a retina and a non-retina display; the
    /// `ResizeObserver` callback re-reads it fresh each time regardless).
    logical_size: (f64, f64),
    scale_factor: f64,
}

impl WebSurface {
    /// Create the wgpu instance/adapter/device/surface for `canvas`, sized
    /// to its current CSS layout size × `devicePixelRatio`.
    pub async fn new(canvas: &HtmlCanvasElement, render: RenderConfig) -> Result<Self, JsValue> {
        let scale_factor = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(0.1);
        // `getBoundingClientRect` reflects actual CSS layout size (respects
        // stylesheet `width`/`height`, unlike the canvas's own `width`/
        // `height` attributes, which this function is about to set).
        let rect = canvas.get_bounding_client_rect();
        let (css_w, css_h) = (rect.width().max(1.0), rect.height().max(1.0));
        let (physical_w, physical_h) = to_physical(css_w, css_h, scale_factor);
        canvas.set_width(physical_w);
        canvas.set_height(physical_h);

        let gpu = GpuSurface::create(
            wgpu::SurfaceTarget::Canvas(canvas.clone()),
            SurfaceRequest {
                label: "proteus-host-web",
                size: (physical_w, physical_h),
                power_preference: render.power_preference,
                present_mode: render.present_mode,
                // The real per-platform difference: WebGL2 is the fallback
                // path, so the device must be requestable under its limits.
                limits: wgpu::Limits::downlevel_webgl2_defaults(),
            },
        )
        .await
        .map_err(|e| JsValue::from_str(&format!("GPU setup failed: {e}")))?;
        log::info!("proteus-host-web: adapter {}", gpu.adapter_description());

        Ok(Self {
            gpu,
            logical_size: (css_w, css_h),
            scale_factor,
        })
    }

    /// The live device — the `WebLoop` hands this to `Engine`/`Renderer` and a
    /// shell shim may need it for GPU work outside the generic bake pass.
    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    /// The live queue. See [`WebSurface::device`].
    pub fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }

    /// The swap chain, for acquiring each frame's texture.
    pub fn surface(&self) -> &wgpu::Surface<'static> {
        &self.gpu.surface
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.gpu.format()
    }

    pub fn viewport(&self) -> Viewport {
        Viewport::new(
            proteus_runtime::glam::Vec2::new(
                self.logical_size.0 as f32,
                self.logical_size.1 as f32,
            ),
            self.scale_factor as f32,
        )
    }

    /// Re-read `devicePixelRatio`, resize the canvas backing store, and
    /// reconfigure the surface for a new CSS layout size. Called from the
    /// `ResizeObserver` callback with the entry's `contentRect`.
    pub fn resize(&mut self, css_width: f64, css_height: f64) -> Viewport {
        let scale_factor = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(self.scale_factor)
            .max(0.1);
        let (physical_w, physical_h) = to_physical(css_width, css_height, scale_factor);

        self.gpu.resize(physical_w, physical_h);

        self.logical_size = (css_width, css_height);
        self.scale_factor = scale_factor;
        self.viewport()
    }

    /// Reconfigure at the current size — for `Lost`/`Outdated` surface
    /// errors and `webglcontextrestored`, neither of which changed the
    /// canvas's own size.
    pub fn reconfigure(&self) {
        self.gpu.reconfigure();
    }
}

fn to_physical(css_w: f64, css_h: f64, scale_factor: f64) -> (u32, u32) {
    (
        (css_w * scale_factor).round().max(1.0) as u32,
        (css_h * scale_factor).round().max(1.0) as u32,
    )
}
