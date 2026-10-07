//! The web host: runs a Proteus app on an HTML canvas, compiled to
//! WebAssembly, drawing with WebGPU where available and WebGL2 otherwise.
//!
//! A Proteus app for the web is written in Rust or in TypeScript. Both share
//! the same canvas setup, frame loop and input handling:
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

mod services;
mod surface;

pub mod js_app;
pub mod rust_app;

pub use js_app::mount;
pub use rust_app::run;
pub use services::PreloadedHostServices;

/// The GPU limits this host requests and checks a config against:
/// WebGL2's, `wgpu::Limits::downlevel_webgl2_defaults()`, since WebGL2 is the
/// fallback in every browser. A WebGPU browser reports higher limits, but a
/// config is still held to these, so it behaves the same in every browser.
/// Pass them to `ProteusConfig::check` to test a config for this host.
pub fn limits() -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults()
}

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
// Inbox: what the event listeners record for the next frame
// ---------------------------------------------------------------------------

/// What the event listeners record for [`WebLoop`] to apply at the start of
/// the next frame.
///
/// The listeners never borrow the `WebLoop` itself. JavaScript's `update`
/// runs while it is borrowed, and anything `update` does that fires a
/// listener synchronously, such as dispatching a `PointerEvent` on the
/// canvas, would borrow it again and panic. The inbox is only borrowed for a
/// moment, never while JavaScript runs.
#[derive(Default)]
struct Inbox {
    /// Pointer input, in the order it happened.
    pointer: Vec<PointerInput>,
    /// The canvas's latest size in CSS pixels, if it changed.
    resize: Option<(f64, f64)>,
    /// The page was hidden, so the next frame's time step starts over.
    hidden: bool,
    /// The WebGL context was lost.
    context_lost: bool,
    /// The WebGL context was restored.
    context_restored: bool,
}

enum PointerInput {
    /// `offsetX` and `offsetY`: CSS pixels from the canvas's corner. `None`
    /// when the pointer left the canvas.
    Moved(Option<(f32, f32)>),
    Pressed,
    Released,
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
    inbox: Rc<RefCell<Inbox>>,
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
            inbox: Rc::default(),
            last_frame: None,
            context_lost: false,
        }
    }

    /// Adds the event listeners and starts the frame loop. The frame loop
    /// owns `self`, so it keeps running after this returns, with nothing for
    /// the caller to keep.
    pub(crate) fn start(self) {
        wire_resize(&self.canvas, &self.inbox);
        wire_pointer(&self.canvas, &self.inbox);
        wire_visibility(&self.inbox);
        wire_context_loss(&self.canvas, &self.inbox);

        start_raf_loop(self);
    }

    fn render_frame(&mut self, ts_ms: f64) {
        self.apply_inbox();

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

    /// Applies what the listeners recorded since the last frame. Pointer
    /// input is still passed on while the context is lost, as it always was.
    fn apply_inbox(&mut self) {
        let inbox = std::mem::take(&mut *self.inbox.borrow_mut());

        if let Some((width, height)) = inbox.resize {
            self.resize(width, height);
        }
        for input in inbox.pointer {
            match input {
                PointerInput::Moved(Some((x, y))) => {
                    // The offsets are in the same units as
                    // `Viewport::logical_size`.
                    let size = self.surface.viewport().logical_size;
                    let world = Vec2::new(x - size.x / 2.0, size.y / 2.0 - y);
                    self.driver.pointer_moved(Some(world));
                }
                PointerInput::Moved(None) => self.driver.pointer_moved(None),
                PointerInput::Pressed => self.driver.pointer_pressed(),
                PointerInput::Released => self.driver.pointer_released(),
            }
        }
        if inbox.hidden {
            // Forget the last frame's time, so the first frame after the tab
            // returns has a time step of zero rather than the whole time it
            // was hidden.
            self.last_frame = None;
        }
        if inbox.context_lost {
            self.context_lost = true;
        }
        if inbox.context_restored {
            self.surface.reconfigure();
            self.context_lost = false;
            self.last_frame = None;
        }
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
// Event listeners. Each `wire_*` adds one that records into the `Inbox`, and
// leaks it with `Closure::forget`, since it lives as long as the page.
// ---------------------------------------------------------------------------

fn start_raf_loop<D: FrameDriver + 'static>(mut web_loop: WebLoop<D>) {
    let f = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::new(move |ts_ms: f64| {
        web_loop.render_frame(ts_ms);
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

fn wire_resize(canvas: &HtmlCanvasElement, inbox: &Rc<RefCell<Inbox>>) {
    let inbox = inbox.clone();
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
        inbox.borrow_mut().resize = Some((rect.width(), rect.height()));
    });
    let observer = web_sys::ResizeObserver::new(closure.as_ref().unchecked_ref())
        .expect("ResizeObserver construction failed");
    observer.observe(canvas);
    closure.forget();
    // The observer must outlive the callback, so it is leaked too.
    std::mem::forget(observer);
}

fn wire_pointer(canvas: &HtmlCanvasElement, inbox: &Rc<RefCell<Inbox>>) {
    use web_sys::PointerEvent;

    let listen = |event: &str, record: fn(&PointerEvent) -> Option<PointerInput>| {
        let inbox = inbox.clone();
        let closure: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
            if let Some(input) = record(&e) {
                inbox.borrow_mut().pointer.push(input);
            }
        });
        canvas
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .unwrap_or_else(|_| panic!("addEventListener({event}) failed"));
        closure.forget();
    };

    listen("pointermove", |e| {
        Some(PointerInput::Moved(Some((
            e.offset_x() as f32,
            e.offset_y() as f32,
        ))))
    });
    listen("pointerleave", |_| Some(PointerInput::Moved(None)));
    // Only the primary button: the left mouse button, a touch or a pen tip.
    listen("pointerdown", |e| {
        (e.button() == 0).then_some(PointerInput::Pressed)
    });
    listen("pointerup", |e| {
        (e.button() == 0).then_some(PointerInput::Released)
    });
}

fn wire_visibility(inbox: &Rc<RefCell<Inbox>>) {
    let inbox = inbox.clone();
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");
    let closure: Closure<dyn FnMut()> = Closure::new(move || {
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .map(|d| d.hidden())
            .unwrap_or(false);
        if hidden {
            inbox.borrow_mut().hidden = true;
        }
    });
    document
        .add_event_listener_with_callback("visibilitychange", closure.as_ref().unchecked_ref())
        .expect("addEventListener(visibilitychange) failed");
    closure.forget();
}

fn wire_context_loss(canvas: &HtmlCanvasElement, inbox: &Rc<RefCell<Inbox>>) {
    let lost_inbox = inbox.clone();
    let on_lost: Closure<dyn FnMut(web_sys::Event)> = Closure::new(move |e: web_sys::Event| {
        // Required for the browser to ever restore the context.
        e.prevent_default();
        log::error!(
            "proteus-host-web: WebGL context lost — rendering paused. Full recovery isn't \
             implemented yet (see this crate's root doc); reload the page."
        );
        lost_inbox.borrow_mut().context_lost = true;
    });
    canvas
        .add_event_listener_with_callback("webglcontextlost", on_lost.as_ref().unchecked_ref())
        .expect("addEventListener(webglcontextlost) failed");
    on_lost.forget();

    let restored_inbox = inbox.clone();
    let on_restored: Closure<dyn FnMut()> = Closure::new(move || {
        log::warn!("proteus-host-web: WebGL context restored — reconfiguring surface");
        restored_inbox.borrow_mut().context_restored = true;
    });
    canvas
        .add_event_listener_with_callback(
            "webglcontextrestored",
            on_restored.as_ref().unchecked_ref(),
        )
        .expect("addEventListener(webglcontextrestored) failed");
    on_restored.forget();
}
