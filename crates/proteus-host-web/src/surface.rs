//! [`WebSurface`] — canvas + wgpu device/surface setup, DPI-aware.
//!
//! M13.2's one deliberate behaviour change from the M12 web shell: canvas
//! backing-store resolution now tracks `devicePixelRatio` instead of being
//! 1:1 CSS pixels. Retina/high-DPI displays get a sharper render; M6 web
//! visual-regression baselines need re-capturing against this (see
//! `PLANNING.md`'s M13.2 section).

use proteus_runtime::config::RenderConfig;
use proteus_runtime::wgpu;
use proteus_runtime::Viewport;
use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

pub struct WebSurface {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    /// CSS pixel size — `Viewport.logical_size` — tracked separately from
    /// `config.width/height` (physical px) so `resize`/`viewport` don't need
    /// to re-read `devicePixelRatio` (it can change mid-session, e.g.
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

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| JsValue::from_str(&format!("create_surface: {e}")))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: render.power_preference,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| JsValue::from_str("no suitable WebGPU or WebGL2 adapter"))?;
        log::info!(
            "proteus-host-web: adapter {} (backend {:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("proteus-host-web"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| JsValue::from_str(&format!("request_device: {e}")))?;

        let surface_caps = surface.get_capabilities(&adapter);
        // Non-sRGB on purpose — see proteus-host-winit's identical choice
        // and comment; colours here are authored gamma-space.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: physical_w,
            height: physical_h,
            present_mode: render.present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            logical_size: (css_w, css_h),
            scale_factor,
        })
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.config.format
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

        self.config.width = physical_w;
        self.config.height = physical_h;
        self.surface.configure(&self.device, &self.config);

        self.logical_size = (css_width, css_height);
        self.scale_factor = scale_factor;
        self.viewport()
    }

    /// Reconfigure at the current size — for `Lost`/`Outdated` surface
    /// errors and `webglcontextrestored`, neither of which changed the
    /// canvas's own size.
    pub fn reconfigure(&self) {
        self.surface.configure(&self.device, &self.config);
    }
}

fn to_physical(css_w: f64, css_h: f64, scale_factor: f64) -> (u32, u32) {
    (
        (css_w * scale_factor).round().max(1.0) as u32,
        (css_h * scale_factor).round().max(1.0) as u32,
    )
}
