//! `proteus-host-web` — Layer 3: a WebGPU/WebGL2 wasm host for
//! [`proteus_runtime`] (M13.2).
//!
//! Two front doors, both driving the same [`WebLoop`] machinery
//! (canvas/wgpu setup, DPI, resize, Pointer Events, visibility-pause,
//! context-loss logging):
//!
//! - **Rust → web**: [`run`] — an app author writes `impl App`, compiles to
//!   wasm, calls `proteus_host_web::run(MyApp::new(), "canvas-id").await`.
//!   Uses the real [`proteus_runtime::Engine`], identical to
//!   `proteus-host-winit`.
//! - **TS → web**: [`mount`] — a wasm-bindgen export the `ts/` layer's
//!   `mount()` calls, taking `setup`/`update` JS functions. Does *not* go
//!   through `Engine` (JS isn't a Rust `App` impl) — [`JsDriver`] replicates
//!   `Engine::frame`'s exact sequence (`tick` → call JS `update` →
//!   `refresh_cascades` → `Renderer::render`) by hand against a shared
//!   `Rc<RefCell<Proteus>>` (see `proteus_sdk_web`'s module doc for why
//!   `ProteusApp` needed to become shared-ownership for this to be sound).
//!
//! ## Asset loading: prefetch, not a new `HostServices` shape
//!
//! `HostServices::load_asset` is synchronous (M13.1) but browser `fetch` is
//! async. Rather than changing that trait now — `PLANNING.md`'s M13.2
//! section defers the general async asset contract to M13.4 — this crate's
//! [`PreloadedHostServices`] fetches a known, fixed list of keys in parallel
//! *before* `Engine::new` / `App::setup` runs, then serves them synchronously
//! from an in-memory map. Mirrors what `proteus-host-winit`'s
//! `DirHostServices` does for a filesystem, just backed by pre-fetched bytes.
//!
//! ## Scope notes (read before assuming more is wired than is)
//!
//! - **Safe-area insets**: [`Viewport::safe_area`] is always
//!   `Insets::default()` (zero) here — no `env(safe-area-inset-*)` probe is
//!   implemented yet. Correct today on every desktop/laptop browser (no
//!   notch to report); a real probe is a follow-up.
//! - **Context loss**: `webglcontextlost` / `webglcontextrestored` are
//!   listened for and logged; on `restored` the surface is reconfigured.
//!   Full "tear down and rebuild everything, re-fetch, re-bake" recovery
//!   (this crate's own `PLANNING.md` section originally sketched) is **not**
//!   implemented — a genuine context loss today logs a warning and the
//!   canvas goes blank until the page reloads. Narrowed scope, stated
//!   plainly rather than claimed working.
//! - **Keyboard / directional navigation**: not wired — `navigation_system`
//!   is itself still a stub.

mod hls_video;
mod services;
mod surface;

pub mod js_app;
pub mod rust_app;

pub use js_app::mount;
pub use rust_app::{run, RustDriver};
pub use services::PreloadedHostServices;
pub use surface::WebSurface;

use std::cell::RefCell;
use std::rc::Rc;

use proteus_runtime::glam::Vec2;
use proteus_runtime::wgpu;
use proteus_runtime::Viewport;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

// ---------------------------------------------------------------------------
// FrameDriver — the seam between WebLoop's event wiring and either front door
// ---------------------------------------------------------------------------

/// What [`WebLoop`] needs from either front door. [`rust_app::RustDriver`]
/// forwards to an `Engine`; [`js_app::JsDriver`] hand-drives a shared
/// `Proteus` + `Renderer`, calling into JS for `update`.
///
/// `pub` (not `pub(crate)`) so a concrete host shell — e.g.
/// `proteus-shell-web`'s `DemoApp`-specific M13.4-debt shim (video / gallery /
/// texture-churn, none of which have a home in the `App` contract yet) — can
/// name [`WebLoop`]'s full type and reach [`rust_app::RustDriver`]'s own
/// accessors without reimplementing this trait itself.
pub trait FrameDriver {
    fn frame(&mut self, dt_secs: f32, target: &wgpu::TextureView);
    fn resize(&mut self, viewport: Viewport);
    fn pointer_moved(&mut self, pos: Option<Vec2>);
    fn pointer_pressed(&mut self);
    fn pointer_released(&mut self);
}

// ---------------------------------------------------------------------------
// WebLoop — shared canvas/rAF/input wiring for both front doors
// ---------------------------------------------------------------------------

/// Owns the live `wgpu` surface and drives `driver` from a
/// `requestAnimationFrame` loop, wired once to the canvas's Pointer Events,
/// a `ResizeObserver`, and `document.visibilitychange`. Both front doors
/// build one of these and call [`WebLoop::start`] to hand control to the
/// browser's event loop (this function returns immediately — wasm has no
/// blocking "run forever" the way native's winit loop does).
pub struct WebLoop<D: FrameDriver + 'static> {
    canvas: HtmlCanvasElement,
    surface: surface::WebSurface,
    driver: D,
    last_frame: Option<f64>,
    /// Set by the `webglcontextlost` listener; checked (and logged, once)
    /// on the next frame. See the crate-root doc's "Context loss" note.
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

    /// Wire every DOM listener and start the `requestAnimationFrame` loop.
    /// Consumes `self` into a shared `Rc<RefCell<_>>` and returns it — the
    /// closures wired below hold their own clone, so the loop keeps running
    /// even if the caller drops the returned handle; [`rust_app::run`]
    /// returns it anyway so a `DemoApp`-shaped M13.4-debt shim can poll
    /// [`Self::driver_mut`] once per its own `requestAnimationFrame` tick
    /// (see `proteus-shell-web`'s crate doc).
    pub(crate) fn start(self) -> Rc<RefCell<Self>> {
        let state = Rc::new(RefCell::new(self));

        wire_resize(&state);
        wire_pointer(&state);
        wire_visibility(&state);
        wire_context_loss(&state);

        start_raf_loop(state.clone());
        state
    }

    /// Mutable access to the front door's own driver — e.g.
    /// `RustDriver::app_mut`/`engine_mut` for a `DemoApp`-specific shim that
    /// needs to poll `Demo::take_pending_*` or reach the GPU pipeline
    /// directly, neither of which the generic `App`/`FrameDriver` contracts
    /// expose.
    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    /// The wgpu device backing this loop's surface — needed by a shim that
    /// registers/writes GPU textures outside the generic bake pass (e.g.
    /// video, texture churn).
    pub fn device(&self) -> &wgpu::Device {
        self.surface.device()
    }

    /// The wgpu queue backing this loop's surface. See [`Self::device`].
    pub fn queue(&self) -> &wgpu::Queue {
        self.surface.queue()
    }

    fn render_frame(&mut self, ts_ms: f64) {
        if self.context_lost {
            // Logged once by the listener itself; every frame after that is
            // a silent no-op until the page reloads — see the crate-root
            // doc's "Context loss" scope note.
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
// Event wiring — each `wire_*` attaches one listener, leaked via
// `Closure::forget` (standard for a listener meant to live as long as the
// page — there is no natural point before page unload to drop it).
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
        // One canvas observed → exactly one entry; contentRect is in CSS px.
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
    // `observer` itself must also outlive the callback; leaking it is the
    // same "lives as long as the page" convention as the closure.
    std::mem::forget(observer);
}

fn wire_pointer<D: FrameDriver + 'static>(state: &Rc<RefCell<WebLoop<D>>>) {
    use web_sys::PointerEvent;

    let canvas = state.borrow().canvas.clone();

    let move_state = state.clone();
    let on_move: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
        let mut s = move_state.borrow_mut();
        // offsetX/Y are CSS px relative to the canvas — the same space
        // `Viewport.logical_size` reports, so no DPR conversion here.
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
            // Dropping the timestamp means the next visible frame computes
            // dt from `None` (0.0) instead of the real, huge, tab-was-
            // backgrounded gap — same intent as `ProteusConfig.frame.
            // dt_clamp_secs`, just for a stall this large rather than a
            // merely long one.
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
        // Required by the WebGL spec for the context to ever come back —
        // an uncancelled contextlost is permanent.
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
