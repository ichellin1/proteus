//! [`run`]: runs a Rust [`App`] on a canvas.

use proteus_runtime::wgpu;
use proteus_runtime::{App, Engine, HostServices, ProteusConfig, Viewport};
use wasm_bindgen::{JsCast, JsValue};

use crate::surface::WebSurface;
use crate::{FrameDriver, WebLoop};

/// Runs `app` on the `<canvas>` element with the given `id`, loading assets
/// through `services`.
///
/// `services` is usually a [`crate::PreloadedHostServices`] made with
/// [`crate::PreloadedHostServices::fetch`], since `App::setup` runs before the
/// first frame and its assets must already be downloaded.
///
/// Returns once the app is set up and its first frame is scheduled; the
/// browser runs every frame after that.
///
/// # Errors
///
/// Returns an error if the canvas isn't found, `config` doesn't fit
/// [`crate::limits`] (the `ConfigError`'s message), or the GPU can't be set
/// up.
pub async fn run<A: App + 'static, S: HostServices + 'static>(
    mut app: A,
    canvas_id: &str,
    config: ProteusConfig,
    mut services: S,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let canvas = web_sys::window()
        .ok_or_else(|| crate::js_error("no window"))?
        .document()
        .ok_or_else(|| crate::js_error("no document"))?
        .get_element_by_id(canvas_id)
        .ok_or_else(|| crate::js_error("canvas element not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| crate::js_error("element is not a canvas"))?;

    config
        .check(&crate::limits())
        .map_err(|e| crate::js_error(&e.to_string()))?;
    let render_cfg = config.render;
    let surface = WebSurface::new(&canvas, render_cfg).await?;
    let viewport = surface.viewport();
    let surface_format = surface.surface_format();

    let engine = Engine::new(
        surface.device(),
        surface.queue(),
        surface_format,
        viewport,
        config,
        &mut app,
        &mut services,
    );

    let driver = RustDriver {
        engine,
        app,
        services,
    };
    WebLoop::new(canvas, surface, driver).start();
    Ok(())
}

pub(crate) struct RustDriver<A: App, S: HostServices> {
    engine: Engine,
    app: A,
    services: S,
}

impl<A: App, S: HostServices> FrameDriver for RustDriver<A, S> {
    fn frame(&mut self, dt_secs: f32, target: &wgpu::TextureView) {
        self.engine
            .frame(dt_secs, target, &mut self.app, &mut self.services);
    }

    fn resize(&mut self, viewport: Viewport) {
        self.engine.resize(viewport);
    }

    fn pointer_moved(&mut self, pos: Option<proteus_runtime::glam::Vec2>) {
        self.engine.pointer_moved(pos);
    }

    fn pointer_pressed(&mut self) {
        self.engine.pointer_pressed();
    }

    fn pointer_released(&mut self) {
        self.engine.pointer_released();
    }
}
