//! The web host: runs a Proteus app on an HTML canvas, compiled to
//! WebAssembly, drawing with WebGPU where available and WebGL2 otherwise.
//!
//! There are two ways to target a web browser when building a Proteus applicaiont,
//! an application written in Rust and an application written in TypeScript.
//! Both ways share the same canvas setup, frame loop and
//! input handling:
//!
//! - **From Rust:** [`run`] runs an [`App`](proteus_runtime::App) through a
//!   [`proteus_runtime::Engine`], exactly as `proteus-host-winit` does.
//! - **From TypeScript:** [`mount`] is what the TypeScript SDK's `mount` calls.
//!   A JavaScript app isn't an `App`, so `mount` runs the same frame sequence
//!   as [`Engine::frame`](proteus_runtime::Engine::frame) itself, calling the
//!   JavaScript `update` function in place of `App::update`.
//!
//! The canvas follows the device's pixel density and its CSS size, input comes
//! from Pointer Events (mouse, touch and pen). While the tab is hidden, the
//! browser stops running the frame loop; the first frame after it returns has
//! a time step of zero, not the whole time it was hidden.
//!
//! ## Loading assets
//!
//! `HostServices::load_asset` must return immediately, but a browser can only
//! fetch asynchronously. So [`PreloadedHostServices`] downloads a fixed list of
//! assets before the app starts, then serves them from memory. For anything
//! else, `fetch_async` makes a real request.
//!
//! ## Not supported yet
//!
//! - **Safe-area insets:** [`Viewport::safe_area`] is always zero. That is
//!   correct on desktop browsers.
//! - **Recovering a lost GPU context:** the loss is logged, and when the
//!   browser restores the context the surface is reconfigured, but textures
//!   are not rebuilt, so the canvas stays blank until the page reloads.
//! - **Keyboard input.**

#![warn(missing_docs)]

mod hls_video;
mod services;
mod surface;

pub mod js_app;
pub mod rust_app;

pub use js_app::mount;
pub use rust_app::run;
pub use services::PreloadedHostServices;

use std::cell::RefCell;
use std::rc::Rc;

use proteus_runtime::glam::Vec2;
use proteus_runtime::wgpu;
use proteus_runtime::Viewport;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

// ---------------------------------------------------------------------------
// FrameDriver: what WebLoop calls, for either way in
// ---------------------------------------------------------------------------

/// What [`WebLoop`] needs from each way in: `rust_app` forwards to an
/// `Engine`, and `js_app` runs `Proteus` and a `Renderer` itself, calling
/// JavaScript for `update`.
pub(crate) trait FrameDriver {
    fn frame(&mut self, dt_secs: f32, target: &wgpu::TextureView);
    fn resize(&mut self, viewport: Viewport);
    fn pointer_moved(&mut self, pos: Option<Vec2>);
    fn pointer_pressed(&mut self);
    fn pointer_released(&mut self);
}

// ---------------------------------------------------------------------------
// WebLoop: canvas, frame loop and input, shared by both ways in
// ---------------------------------------------------------------------------

/// Owns the GPU surface and drives `driver` from a `requestAnimationFrame`
/// loop, with listeners for the canvas's Pointer Events, its size, and the
/// page's visibility. [`WebLoop::start`] hands it to the browser and returns
/// immediately.
pub(crate) struct WebLoop<D: FrameDriver + 'static> {
    canvas: HtmlCanvasElement,
    surface: surface::WebSurface,
    driver: D,
    last_frame: Option<f64>,
    // Set when the WebGL context is lost; frames are skipped while it is.
    context_lost: bool,
}

impl<D: FrameDriver + 'static> WebLoop<D> {
    pub(crate) fn new(canvas: HtmlCanvasElement, surface: surface::WebSurface, driver: D) -> Self {
        Self {
            canvas,
            surface,
            driver,
            last_frame: None,
            context_lost: false,
        }
    }

    /// Adds the event listeners and starts the frame loop. The listeners
    /// share ownership of `self`, so the loop keeps running after this
    /// returns, with nothing for the caller to keep.
    pub(crate) fn start(self) {
        let state = Rc::new(RefCell::new(self));

        wire_resize(&state);
        wire_pointer(&state);
        wire_visibility(&state);
        wire_context_loss(&state);

        start_raf_loop(state);
    }

    fn render_frame(&mut self, ts_ms: f64) {
        if self.context_lost {
            // Already logged once by the listener; skip frames until the
            // page reloads.
            return;
        }

        let dt = match self.last_frame {
            Some(last) => ((ts_ms - last) / 1000.0) as f32,
            None => 0.0,
        };
        self.last_frame = Some(ts_ms);

        let frame = match self.surface.surface().get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.reconfigure();
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => return,
            e => {
                log::error!("proteus-host-web: surface error: {e:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        self.driver.frame(dt, &view);
        frame.present();
    }

    fn resize(&mut self, css_width: f64, css_height: f64) {
        if css_width <= 0.0 || css_height <= 0.0 {
            return;
        }
        let viewport = self.surface.resize(css_width, css_height);
        self.driver.resize(viewport);
    }
}

// ---------------------------------------------------------------------------
// Event listeners. Each `wire_*` adds one, and leaks it with
// `Closure::forget`, since it lives as long as the page.
// ---------------------------------------------------------------------------

fn start_raf_loop<D: FrameDriver + 'static>(state: Rc<RefCell<WebLoop<D>>>) {
    let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::new(move |ts_ms: f64| {
        state.borrow_mut().render_frame(ts_ms);
        request_animation_frame(f.borrow().as_ref().unwrap());
    }));
    request_animation_frame(g.borrow().as_ref().unwrap());
}

pub(crate) fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) {
    web_sys::window()
        .expect("no window")
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

fn wire_resize<D: FrameDriver + 'static>(state: &Rc<RefCell<WebLoop<D>>>) {
    let state = state.clone();
    let canvas = state.borrow().canvas.clone();
    let closure: Closure<dyn FnMut(js_sys::Array)> = Closure::new(move |entries: js_sys::Array| {
        // One canvas is observed, so there is one entry. `contentRect` is in
        // CSS pixels.
        let Some(entry) = entries
            .get(0)
            .dyn_into::<web_sys::ResizeObserverEntry>()
            .ok()
        else {
            return;
        };
        let rect = entry.content_rect();
        state.borrow_mut().resize(rect.width(), rect.height());
    });
    let observer = web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref())
        .expect("ResizeObserver construction failed");
    observer.observe(&canvas);
    closure.forget();
    // The observer must outlive the callback, so it is leaked too.
    std::mem::forget(observer);
}

fn wire_pointer<D: FrameDriver + 'static>(state: &Rc<RefCell<WebLoop<D>>>) {
    use web_sys::PointerEvent;

    let canvas = state.borrow().canvas.clone();

    let move_state = state.clone();
    let on_move: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
        let mut s = move_state.borrow_mut();
        // `offsetX` and `offsetY` are CSS pixels from the canvas's corner: the
        // same units as `Viewport::logical_size`.
        let w = s.surface.viewport().logical_size.x;
        let h = s.surface.viewport().logical_size.y;
        let wx = e.offset_x() as f32 - w / 2.0;
        let wy = h / 2.0 - e.offset_y() as f32;
        s.driver.pointer_moved(Some(Vec2::new(wx, wy)));
    });
    canvas
        .add_event_listener_with_callback("pointermove", on_move.as_ref().unchecked_ref())
        .expect("addEventListener(pointermove) failed");
    on_move.forget();

    let leave_state = state.clone();
    let on_leave: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |_e: PointerEvent| {
        leave_state.borrow_mut().driver.pointer_moved(None);
    });
    canvas
        .add_event_listener_with_callback("pointerleave", on_leave.as_ref().unchecked_ref())
        .expect("addEventListener(pointerleave) failed");
    on_leave.forget();

    let down_state = state.clone();
    let on_down: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
        if e.button() == 0 {
            down_state.borrow_mut().driver.pointer_pressed();
        }
    });
    canvas
        .add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref())
        .expect("addEventListener(pointerdown) failed");
    on_down.forget();

    let up_state = state.clone();
    let on_up: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
        if e.button() == 0 {
            up_state.borrow_mut().driver.pointer_released();
        }
    });
    canvas
        .add_event_listener_with_callback("pointerup", on_up.as_ref().unchecked_ref())
        .expect("addEventListener(pointerup) failed");
    on_up.forget();
}

fn wire_visibility<D: FrameDriver + 'static>(state: &Rc<RefCell<WebLoop<D>>>) {
    let state = state.clone();
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");
    let closure: Closure<dyn FnMut()> = Closure::new(move || {
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .map(|d| d.hidden())
            .unwrap_or(false);
        let mut s = state.borrow_mut();
        if hidden {
            // Forget the last frame's time, so the first frame after the tab
            // returns has a time step of zero rather than the whole time it
            // was hidden.
            s.last_frame = None;
        }
    });
    document
        .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
        .expect("addEventListener(visibilitychange) failed");
    closure.forget();
}

fn wire_context_loss<D: FrameDriver + 'static>(state: &Rc<RefCell<WebLoop<D>>>) {
    let canvas = state.borrow().canvas.clone();

    let lost_state = state.clone();
    let on_lost: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |e: web_sys::Event| {
        // Required for the browser to ever restore the context.
        e.prevent_default();
        log::error!(
            "proteus-host-web: WebGL context lost — rendering paused. Full recovery isn't \
             implemented yet (see this crate's root doc); reload the page."
        );
        lost_state.borrow_mut().context_lost = true;
    });
    canvas
        .add_event_listener_with_callback("webglcontextlost", on_lost.as_ref().unchecked_ref())
        .expect("addEventListener(webglcontextlost) failed");
    on_lost.forget();

    let restored_state = state.clone();
    let on_restored: Closure<dyn FnMut()> = Closure::new(move || {
        log::warn!("proteus-host-web: WebGL context restored — reconfiguring surface");
        let mut s = restored_state.borrow_mut();
        s.surface.reconfigure();
        s.context_lost = false;
        s.last_frame = None;
    });
    canvas
        .add_event_listener_with_callback(
            "webglcontextrestored",
            on_restored.as_ref().unchecked_ref(),
        )
        .expect("addEventListener(webglcontextrestored) failed");
    on_restored.forget();
}
