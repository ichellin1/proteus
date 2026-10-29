//! [`mount`]: runs a TypeScript app on a canvas.
//!
//! Unlike [`crate::run`], this provides no `HostServices`: a JavaScript app
//! fetches its own assets, with `fetch` and `ProteusApp.loadTexture`.

use std::cell::RefCell;
use std::rc::Rc;

use proteus_runtime::glam::Vec2;
use proteus_runtime::{wgpu, KeyEvent, KeyMap, ProteusConfig, Renderer, Viewport};
use proteus_sdk_web::ProteusApp;
use wasm_bindgen::prelude::*;

use crate::surface::WebSurface;
use crate::{FrameDriver, WebLoop};

/// Runs a JavaScript app on the `<canvas>` element with the given `id`.
///
/// Calls `setup(app)` once, before the first frame, then `update(dtSeconds)`
/// every frame if it is given. To use the app in `update`, keep the one
/// passed to `setup`. `config` is a `ProteusConfigDto` object, or `null`.
///
/// # Errors
///
/// Throws if the canvas isn't found, the GPU can't be set up, or `config`
/// is invalid.
#[wasm_bindgen]
pub async fn mount(
    canvas_id: String,
    setup: js_sys::Function,
    update: Option<js_sys::Function>,
    config: JsValue,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let canvas = web_sys::window()
        .ok_or_else(|| crate::js_error("no window"))?
        .document()
        .ok_or_else(|| crate::js_error("no document"))?
        .get_element_by_id(&canvas_id)
        .ok_or_else(|| crate::js_error("canvas element not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| crate::js_error("element is not a canvas"))?;

    // Overrides on top of `ProteusConfig::web()`. `null` or `undefined`
    // keeps the web settings unchanged.
    let config = if config.is_null() || config.is_undefined() {
        ProteusConfig::web()
    } else {
        let dto: proteus_runtime::ProteusConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| crate::js_error(&format!("invalid config: {e}")))?;
        dto.apply().map_err(|e| crate::js_error(&e))?
    };

    // `Renderer::new` panics on settings that don't fit, which would stop
    // the wasm module, so check them first and throw an error naming the
    // setting. Against WebGL2's limits, whatever the browser's device
    // reports, so a config behaves the same in every browser.
    config
        .check(&crate::limits())
        .map_err(|e| crate::js_error(&e.to_string()))?;

    let autofocus = config.input.autofocus;
    let keep_tab = config.input.keep_tab;
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

    // JavaScript gets its own handle to the shared `Proteus`; the driver keeps
    // one for the frame loop.
    let js_app = ProteusApp::from_shared(proteus.clone());
    let keys_app = js_app.clone();
    setup.call1(&JsValue::NULL, &JsValue::from(js_app))?;

    let driver = JsDriver {
        proteus,
        app: keys_app,
        renderer,
        update,
    };
    WebLoop::new(canvas, surface, driver).start(autofocus, keep_tab);
    Ok(())
}

struct JsDriver {
    proteus: Rc<RefCell<proteus_runtime::Proteus>>,
    // The app JavaScript has, which runs its key callbacks.
    app: ProteusApp,
    renderer: Renderer,
    update: Option<js_sys::Function>,
}

impl FrameDriver for JsDriver {
    /// Runs one frame in the same order as `Engine::update`, calling the
    /// JavaScript `update` in place of `App::update`, after the JavaScript key
    /// callbacks. The `Proteus` borrow is released before either runs, since
    /// they may call back into the app.
    fn update(&mut self, dt_secs: f32) -> bool {
        self.app.dispatch_keys();
        self.proteus.borrow_mut().tick(dt_secs);

        if let Some(update) = &self.update {
            if let Err(e) = update.call1(&JsValue::NULL, &JsValue::from_f64(dt_secs as f64)) {
                log::error!("proteus-host-web: update() threw: {e:?}");
            }
        }

        self.proteus.borrow_mut().refresh_cascades();
        self.renderer.prepare(&mut self.proteus.borrow_mut())
    }

    fn draw(&mut self, target: &wgpu::TextureView) {
        self.renderer.draw(&mut self.proteus.borrow_mut(), target);
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

    fn key_event(&mut self, event: KeyEvent) {
        self.proteus.borrow_mut().key_event(event);
    }

    fn release_keys(&mut self) {
        self.proteus.borrow_mut().release_keys();
    }

    fn set_tab_wraps(&mut self, wraps: bool) {
        self.proteus.borrow_mut().set_tab_wraps(wraps);
    }

    fn key_map_version(&self) -> u64 {
        self.proteus.borrow().key_map().version()
    }

    fn key_map(&self) -> KeyMap {
        self.proteus.borrow().key_map().clone()
    }
}
