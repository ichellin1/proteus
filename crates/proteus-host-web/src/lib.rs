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
//! The canvas follows the device's pixel density and its CSS size, pointer
//! input comes from Pointer Events (mouse, touch and pen), and keys from the
//! canvas's keyboard events. The canvas can take keyboard focus: a click gives
//! it focus, and `ProteusConfig`'s `input.autofocus` gives it focus at the
//! start. The browser doesn't act on a key the app's key map binds, so Space
//! and the arrows don't scroll the page, except for Tab and Shift+Tab, which
//! can leave the canvas unless `input.keep_tab` is set. A key the map doesn't
//! bind is left to the browser, even if a key callback claims it, since key
//! callbacks run at the next frame. While the tab is hidden, the
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

#![warn(missing_docs)]

mod services;
mod surface;

pub mod js_app;
pub mod rust_app;

pub use js_app::mount;
pub use rust_app::run;
pub use services::PreloadedHostServices;

/// An `Error` with `message`, to throw to JavaScript. Unlike a thrown string,
/// it carries a stack trace and passes `instanceof Error`.
pub(crate) fn js_error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

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
use proteus_runtime::{Action, Code, Key, KeyEvent, KeyMap, KeyState, Modifiers, Viewport};
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

// ---------------------------------------------------------------------------
// FrameDriver: what WebLoop calls, for either way in
// ---------------------------------------------------------------------------

/// What [`WebLoop`] needs from each way in: `rust_app` forwards to an
/// `Engine`, and `js_app` runs `Proteus` and a `Renderer` itself, calling
/// JavaScript for `update`.
pub(crate) trait FrameDriver {
    /// Runs a frame up to drawing it, and returns whether it looks different
    /// from the last one; see `Engine::update`.
    fn update(&mut self, dt_secs: f32) -> bool;
    /// Draws the frame `update` prepared into `target`.
    fn draw(&mut self, target: &wgpu::TextureView);
    fn resize(&mut self, viewport: Viewport);
    fn pointer_moved(&mut self, pos: Option<Vec2>);
    fn pointer_pressed(&mut self);
    fn pointer_released(&mut self);
    fn key_event(&mut self, event: KeyEvent);
    fn release_keys(&mut self);
    fn set_tab_wraps(&mut self, wraps: bool);
    /// The key map's [`KeyMap::version`](proteus_runtime::KeyMap::version).
    fn key_map_version(&self) -> u64;
    fn key_map(&self) -> KeyMap;
}

// ---------------------------------------------------------------------------
// BoundKeys: the bindings the key listener reads
// ---------------------------------------------------------------------------

/// The key map's bindings, copied where the key listener can read them. It
/// has to tell the browser whether to act on a key while the event is being
/// handled, but key callbacks only run at the next frame, so it goes by the
/// bindings: a bound key is the app's.
#[derive(Default)]
struct BoundKeys {
    key_map: KeyMap,
    // The key map's version it was copied at.
    version: Option<u64>,
    keep_tab: bool,
}

impl BoundKeys {
    /// Whether the browser should leave `event` to the app. Tab and Shift+Tab
    /// are left to the browser unless the app keeps them, so they can move
    /// focus out of the canvas.
    fn keeps(&self, event: &KeyEvent) -> bool {
        match self.key_map.action_for(event) {
            Some(Action::Next | Action::Previous) => self.keep_tab,
            Some(_) => true,
            None => false,
        }
    }
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
    /// Pointer and keyboard input, in the order it happened.
    input: Vec<Input>,
    /// The canvas's latest size in CSS pixels, if it changed.
    resize: Option<(f64, f64)>,
    /// The page was hidden, so the next frame's time step starts over.
    hidden: bool,
    /// The WebGL context was lost.
    context_lost: bool,
    /// The WebGL context was restored.
    context_restored: bool,
}

enum Input {
    /// `offsetX` and `offsetY`: CSS pixels from the canvas's corner. `None`
    /// when the pointer left the canvas.
    PointerMoved(Option<(f32, f32)>),
    PointerPressed,
    PointerReleased,
    Key(KeyEvent),
    /// The canvas lost focus, or the page was hidden, so the keys held won't
    /// report their releases.
    ReleaseKeys,
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
    bound_keys: Rc<RefCell<BoundKeys>>,
    last_frame: Option<f64>,
    // Set when the WebGL context is lost; frames are skipped while it is.
    context_lost: bool,
    // Draw the next frame even if nothing in it changed: the canvas is new,
    // was resized, or its surface or context was lost, or a changed frame
    // couldn't be drawn. A frame that isn't drawn leaves the canvas showing
    // the last one.
    must_draw: bool,
}

impl<D: FrameDriver + 'static> WebLoop<D> {
    pub(crate) fn new(canvas: HtmlCanvasElement, surface: surface::WebSurface, driver: D) -> Self {
        Self {
            canvas,
            surface,
            driver,
            inbox: Rc::default(),
            bound_keys: Rc::default(),
            last_frame: None,
            context_lost: false,
            must_draw: true,
        }
    }

    /// Adds the event listeners and starts the frame loop, giving the canvas
    /// keyboard focus first if `autofocus` is set. Unless `keep_tab` is set,
    /// Tab and Shift+Tab can leave the canvas. The frame loop owns `self`, so
    /// it keeps running after this returns, with nothing for the caller to
    /// keep.
    pub(crate) fn start(mut self, autofocus: bool, keep_tab: bool) {
        self.driver.set_tab_wraps(keep_tab);
        self.bound_keys.borrow_mut().keep_tab = keep_tab;
        self.copy_bindings();
        wire_resize(&self.canvas, &self.inbox);
        wire_pointer(&self.canvas, &self.inbox);
        wire_keys(&self.canvas, &self.inbox, &self.bound_keys);
        wire_visibility(&self.inbox);
        wire_context_loss(&self.canvas, &self.inbox);

        if autofocus {
            if let Err(e) = self.canvas.focus() {
                log::warn!("proteus-host-web: couldn't focus the canvas: {e:?}");
            }
        }

        start_raf_loop(self);
    }

    /// Copies the key map's bindings for the key listener, if they changed.
    fn copy_bindings(&mut self) {
        let version = self.driver.key_map_version();
        let mut bound = self.bound_keys.borrow_mut();
        if bound.version != Some(version) {
            bound.key_map = self.driver.key_map();
            bound.version = Some(version);
        }
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

        let changed = self.driver.update(dt);
        // After `update`, where app code may have changed the key map.
        self.copy_bindings();
        if !changed && !self.must_draw {
            return;
        }
        // Until it's presented, the canvas doesn't show this frame.
        self.must_draw = true;

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
        self.driver.draw(&view);
        frame.present();
        self.must_draw = false;
    }

    /// Applies what the listeners recorded since the last frame. Pointer
    /// input is still passed on while the context is lost, as it always was.
    fn apply_inbox(&mut self) {
        let inbox = std::mem::take(&mut *self.inbox.borrow_mut());

        if let Some((width, height)) = inbox.resize {
            self.resize(width, height);
        }
        for input in inbox.input {
            match input {
                Input::PointerMoved(Some((x, y))) => {
                    // The offsets are from the canvas's top-left, in the
                    // same units as `Viewport::logical_size`.
                    let world = self.surface.viewport().top_left_to_world(Vec2::new(x, y));
                    self.driver.pointer_moved(Some(world));
                }
                Input::PointerMoved(None) => self.driver.pointer_moved(None),
                Input::PointerPressed => self.driver.pointer_pressed(),
                Input::PointerReleased => self.driver.pointer_released(),
                Input::Key(event) => self.driver.key_event(event),
                Input::ReleaseKeys => self.driver.release_keys(),
            }
        }
        if inbox.hidden {
            // Forget the last frame's time, so the first frame after the tab
            // returns has a time step of zero rather than the whole time it
            // was hidden.
            self.last_frame = None;
            // The browser may have discarded the canvas's image meanwhile.
            self.must_draw = true;
        }
        if inbox.context_lost {
            self.context_lost = true;
        }
        if inbox.context_restored {
            self.surface.reconfigure();
            self.context_lost = false;
            self.last_frame = None;
            self.must_draw = true;
        }
    }

    fn resize(&mut self, css_width: f64, css_height: f64) {
        if css_width <= 0.0 || css_height <= 0.0 {
            return;
        }
        let viewport = self.surface.resize(css_width, css_height);
        self.driver.resize(viewport);
        // Resizing the canvas clears it.
        self.must_draw = true;
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

    let listen = |event: &str, record: fn(&PointerEvent) -> Option<Input>| {
        let inbox = inbox.clone();
        let closure: Closure<dyn FnMut(PointerEvent)> = Closure::new(move |e: PointerEvent| {
            if let Some(input) = record(&e) {
                inbox.borrow_mut().input.push(input);
            }
        });
        canvas
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .unwrap_or_else(|_| panic!("addEventListener({event}) failed"));
        closure.forget();
    };

    listen("pointermove", |e| {
        Some(Input::PointerMoved(Some((
            e.offset_x() as f32,
            e.offset_y() as f32,
        ))))
    });
    listen("pointerleave", |_| Some(Input::PointerMoved(None)));
    // Only the primary button: the left mouse button, a touch or a pen tip.
    listen("pointerdown", |e| {
        (e.button() == 0).then_some(Input::PointerPressed)
    });
    listen("pointerup", |e| {
        (e.button() == 0).then_some(Input::PointerReleased)
    });
}

fn wire_keys(
    canvas: &HtmlCanvasElement,
    inbox: &Rc<RefCell<Inbox>>,
    bound_keys: &Rc<RefCell<BoundKeys>>,
) {
    use web_sys::KeyboardEvent;

    // A canvas only takes keyboard focus, and so key events, with a tabindex.
    // One the page set is kept.
    if !canvas.has_attribute("tabindex") {
        let _ = canvas.set_attribute("tabindex", "0");
    }

    let listen = |event: &str, pressed: bool| {
        let inbox = inbox.clone();
        let bound_keys = bound_keys.clone();
        let closure: Closure<dyn FnMut(KeyboardEvent)> = Closure::new(move |e: KeyboardEvent| {
            // Keys pressed while an input method composes text are the input
            // method's, not the app's.
            if e.is_composing() {
                return;
            }
            let event = key_event(&e, pressed);
            // So that, for example, Space and the arrows don't scroll the
            // page.
            if bound_keys.borrow().keeps(&event) {
                e.prevent_default();
            }
            inbox.borrow_mut().input.push(Input::Key(event));
        });
        canvas
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .unwrap_or_else(|_| panic!("addEventListener({event}) failed"));
        closure.forget();
    };
    listen("keydown", true);
    listen("keyup", false);

    let blur_inbox = inbox.clone();
    let on_blur: Closure<dyn FnMut()> = Closure::new(move || {
        blur_inbox.borrow_mut().input.push(Input::ReleaseKeys);
    });
    canvas
        .add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref())
        .expect("addEventListener(blur) failed");
    on_blur.forget();
}

/// The [`KeyEvent`] for the browser's `e`, a `keydown` if `pressed` and a
/// `keyup` otherwise.
fn key_event(e: &web_sys::KeyboardEvent, pressed: bool) -> KeyEvent {
    let key = Key::from_web_name(&e.key());
    let modifiers = Modifiers {
        shift: e.shift_key(),
        control: e.ctrl_key(),
        alt: e.alt_key(),
        meta: e.meta_key(),
    };
    let state = match (pressed, e.repeat()) {
        (true, false) => KeyState::Pressed,
        (true, true) => KeyState::Repeated,
        (false, _) => KeyState::Released,
    };
    // The browser doesn't say what text a key types. A character typed with
    // Control or Meta held is a shortcut, not text.
    let text = match &key {
        Key::Character(c) if pressed && !modifiers.control && !modifiers.meta => Some(c.clone()),
        _ => None,
    };
    KeyEvent::new(key, Code::from_web_name(&e.code()), state, modifiers, text)
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
            let mut inbox = inbox.borrow_mut();
            inbox.hidden = true;
            inbox.input.push(Input::ReleaseKeys);
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
