//! [`mount`] — the TS → web front door.
//!
//! Unlike [`crate::run`] this never touches `HostServices` — a JS `setup`
//! function receives a [`ProteusApp`] and has no `Frame`/`load_asset` to call
//! (that seam is Rust-`App`-specific); a JS app fetches/attaches its own
//! assets however it likes. Generalising asset injection for JS apps is
//! M13.4, not this.

use std::cell::RefCell;
use std::rc::Rc;

use proteus_runtime::glam::Vec2;
use proteus_runtime::{wgpu, ProteusConfig, Renderer, Viewport};
use proteus_sdk_web::ProteusApp;
use wasm_bindgen::prelude::*;

use crate::surface::WebSurface;
use crate::{FrameDriver, WebLoop};

/// Mount a TS-authored app on the `<canvas>` element with the given `id`.
///
/// Calls `setup(app)` once, synchronously, before the first frame is queued.
/// Calls `update(dtSeconds)` every frame after, if provided. `app` (the same
/// [`ProteusApp`] `setup` received) stays valid for the app's whole
/// lifetime — a JS app that wants access to it in `update` should capture it
/// from `setup`'s own closure rather than expect it passed again; see
/// `ts/src/index.ts`'s `mount()` wrapper.
#[wasm_bindgen]
pub async fn mount(
    canvas_id: String,
    setup: js_sys::Function,
    update: Option<js_sys::Function>,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let canvas = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?
        .get_element_by_id(&canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element is not a canvas"))?;

    let config = ProteusConfig::web();
    let surface = WebSurface::new(&canvas, config.render).await?;
    let viewport = surface.viewport();
    let surface_format = surface.surface_format();

    let proteus = Rc::new(RefCell::new(proteus_runtime::Proteus::new()));
    let renderer = Renderer::new(
        &mut proteus.borrow_mut(),
        surface.device(),
        surface.queue(),
        surface_format,
        viewport,
        config,
    );

    // A fresh JS-owned handle onto the same shared `Proteus` — `proteus`
    // itself stays with the driver for the render loop. See
    // `proteus_sdk_web`'s module doc for why this is sound (Rc<RefCell<_>>,
    // not a bare `Proteus`).
    let js_app = ProteusApp::from_shared(proteus.clone());
    setup.call1(&JsValue::NULL, &JsValue::from(js_app))?;

    let driver = JsDriver {
        proteus,
        renderer,
        update,
    };
    WebLoop::new(canvas, surface, driver).start();
    Ok(())
}

struct JsDriver {
    proteus: Rc<RefCell<proteus_runtime::Proteus>>,
    renderer: Renderer,
    update: Option<js_sys::Function>,
}

impl FrameDriver for JsDriver {
    /// Mirrors `Engine::frame`'s exact sequence — see that type's module
    /// doc — just with a JS function standing in for `App::update`. Each
    /// `borrow_mut()` is a short-lived temporary, dropped before `update` is
    /// invoked: holding one across the call would double-borrow the
    /// `RefCell` the moment `update` calls back into any `ProteusApp` method.
    fn frame(&mut self, dt_secs: f32, target: &wgpu::TextureView) {
        self.proteus.borrow_mut().tick(dt_secs);

        if let Some(update) = &self.update {
            if let Err(e) = update.call1(&JsValue::NULL, &JsValue::from_f64(dt_secs as f64)) {
                log::error!("proteus-host-web: update() threw: {e:?}");
            }
        }

        self.proteus.borrow_mut().refresh_cascades();
        self.renderer.render(&mut self.proteus.borrow_mut(), target);
    }

    fn resize(&mut self, viewport: Viewport) {
        self.renderer
            .resize(&mut self.proteus.borrow_mut(), viewport);
    }

    fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.proteus.borrow_mut().pointer_moved(pos);
    }

    fn pointer_pressed(&mut self) {
        self.proteus.borrow_mut().pointer_pressed();
    }

    fn pointer_released(&mut self) {
        self.proteus.borrow_mut().pointer_released();
    }
}
