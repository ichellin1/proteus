//! [`WebSurface`]: the canvas's GPU surface, sized for the display's pixel
//! density.
//!
//! GPU setup itself is `GpuSurface::create`, shared with the native host. This
//! module adds what only the web needs: reading `devicePixelRatio`, sizing the
//! canvas's pixel buffer to match, and keeping its CSS size, which is the
//! viewport's logical size.

use proteus_runtime::config::RenderConfig;
use proteus_runtime::wgpu;
use proteus_runtime::{GpuSurface, SurfaceRequest, Viewport};
use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

pub struct WebSurface {
    // Device, queue, surface and swap chain.
    gpu: GpuSurface,
    // The canvas's CSS size, which is the viewport's logical size. The pixel
    // density is read again on every resize, since it changes when a window
    // moves to another display.
    logical_size: (f64, f64),
    scale_factor: f64,
}

impl WebSurface {
    /// Sets up the GPU surface for `canvas`, at its CSS size multiplied by
    /// `devicePixelRatio`.
    pub async fn new(canvas: &HtmlCanvasElement, render: RenderConfig) -> Result<Self, JsValue> {
        let scale_factor = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0)
            .max(0.1);
        // The laid-out CSS size, not the `width` and `height` attributes, which
        // are about to be set.
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
                limits: crate::limits(),
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

    /// The GPU device.
    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    /// The device's command queue.
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

    /// Resizes the canvas's pixel buffer and the surface for a new CSS size,
    /// reading `devicePixelRatio` again.
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

    /// Reconfigures the surface at its current size, after the surface was
    /// lost or outdated, or the WebGL context was restored.
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
