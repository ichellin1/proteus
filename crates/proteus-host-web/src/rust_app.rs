//! [`run`] — the Rust → web front door.

use proteus_runtime::wgpu;
use proteus_runtime::{App, Engine, HostServices, ProteusConfig, Viewport};
use wasm_bindgen::{JsCast, JsValue};

use crate::surface::WebSurface;
use crate::{FrameDriver, WebLoop};

/// Run `app` on the `<canvas>` element with the given `id`, using `services`
/// for asset fetching (typically a [`crate::PreloadedHostServices`] you
/// built with [`crate::PreloadedHostServices::fetch`] before calling this —
/// `App::setup` runs synchronously inside `Engine::new`, so any asset it
/// needs must already be in hand).
///
/// Returns once the canvas/GPU/`Engine` are set up and the first
/// `requestAnimationFrame` is queued — like native's `run`, this does not
/// block; the browser's own event loop drives every frame after it returns,
/// and there is nothing left for the caller to hold.
pub async fn run<A: App + 'static, S: HostServices + 'static>(
    mut app: A,
    canvas_id: &str,
    config: ProteusConfig,
    mut services: S,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let canvas = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element is not a canvas"))?;

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
