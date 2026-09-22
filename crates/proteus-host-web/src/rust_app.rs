//! [`run`] — the Rust → web front door.

use std::cell::RefCell;
use std::rc::Rc;

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
/// Returns the running [`WebLoop`] handle once the canvas/GPU/`Engine` are
/// set up and the first `requestAnimationFrame` is queued — like native's
/// `run`, this does not block; the browser's own event loop drives every
/// frame after this returns. Most callers can drop the handle (the loop
/// keeps running via its own internal clones — see [`WebLoop::start`]);
/// it's returned so a concrete, `A`/`S`-specific host shell can poll
/// [`WebLoop::driver_mut`]/[`RustDriver::app_mut`] for whatever isn't part
/// of the generic `App` contract yet (see `proteus-shell-web`'s crate doc
/// for its M13.4-debt video/gallery/texture-churn shim, the reason this
/// exists).
pub async fn run<A: App + 'static, S: HostServices + 'static>(
    mut app: A,
    canvas_id: &str,
    config: ProteusConfig,
    mut services: S,
) -> Result<Rc<RefCell<WebLoop<RustDriver<A, S>>>>, JsValue> {
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
    Ok(WebLoop::new(canvas, surface, driver).start())
}

pub struct RustDriver<A: App, S: HostServices> {
    engine: Engine,
    app: A,
    services: S,
}

impl<A: App, S: HostServices> RustDriver<A, S> {
    /// The running [`Engine`] — for a shim that needs `Proteus`/GPU-pipeline
    /// access the generic `App`/`Frame` contract doesn't expose (e.g. video
    /// texture registration, texture churn).
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// The concrete app — for a shim that needs `A`-specific methods
    /// (`Frame`'s contract stays generic, but a shim polling `Demo::
    /// take_pending_*` needs the real type, not `&mut dyn App`).
    pub fn app_mut(&mut self) -> &mut A {
        &mut self.app
    }

    /// [`Self::engine_mut`] and [`Self::app_mut`] together, as two
    /// independent borrows — needed whenever a shim must call into both in
    /// the same expression (e.g. handing `Engine::proteus_mut()`'s `Proteus`
    /// to a `Demo` method reached through `app_mut().demo_mut()`), which
    /// two separate `&mut self` accessor calls can't do: the borrow checker
    /// can't see across this crate's boundary that the two calls touch
    /// disjoint fields, so it refuses to let both borrows live at once.
    /// Splitting inside one `&mut self` method (which *can* see the two
    /// fields are disjoint) launders that for the caller.
    pub fn split_mut(&mut self) -> (&mut Engine, &mut A) {
        (&mut self.engine, &mut self.app)
    }
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
