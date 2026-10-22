//! The browser benchmark's Proteus app written in Rust, compiled to
//! WebAssembly: the same scenes as the TypeScript page, drawn by the same web
//! host and renderer. The difference it measures is the calls. A TypeScript
//! app's `component` and `animateTo` each cross from JavaScript into
//! WebAssembly, with their arguments converted on the way; here they're
//! ordinary Rust calls.
//!
//! The page drives it through the exports below: `start` on a canvas, then
//! `show` a scene, `on_frame` to be called once a frame while it's timed, and
//! `clear`. (Plain names, not links: the crate is empty natively, where its
//! docs are built.) The scenes and moves come from the same random numbers as
//! the page's `scene.ts` and `motion.ts`, so every renderer draws and moves
//! the same components.
//!
//! It only builds for WebAssembly; natively the crate is empty.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use proteus_runtime::glam::{Vec2, Vec3, Vec4};
use proteus_runtime::{App, Frame, ProteusConfig};
use proteus_sdk::{ComponentSpec, Easing, Handle, QuadState, TransitionConfig};
use wasm_bindgen::prelude::*;

const WIDTH: f64 = 1280.0;
const HEIGHT: f64 = 800.0;
/// How long each move takes, and how often a new one starts, in milliseconds.
const MOVE_MS: f64 = 500.0;

/// The same xorshift generator as the page's `scene.ts`, giving the same
/// numbers in the same order.
struct Random(u64);

impl Random {
    fn new() -> Self {
        Self(0x9e37_79b9_7f4a_7c15)
    }

    /// A number from 0 to 1.
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f64 / (1u64 << 24) as f64
    }

    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }
}

/// A scene: each component's size range, from and to, and corner radius, in
/// CSS pixels, as `scene.ts`'s `SCENES`.
fn scene_sizes(kind: &str) -> Option<(f64, f64, f32)> {
    match kind {
        "small" => Some((4.0, 8.0, 2.0)),
        "large" => Some((20.0, 200.0, 8.0)),
        "moving" => Some((10.0, 40.0, 4.0)),
        _ => None,
    }
}

/// One component's geometry, centered at (`x`, `y`) in CSS pixels from the
/// canvas's top-left. Proteus measures from the center, with y up.
fn geometry(x: f64, y: f64, size: Vec2, color: Vec4, radius: f32) -> QuadState {
    QuadState {
        position: Vec3::new((x - WIDTH / 2.0) as f32, (HEIGHT / 2.0 - y) as f32, 0.0),
        size,
        color,
        corner_radius: radius,
        ..Default::default()
    }
}

/// What the page has asked for, read by the app each frame.
#[derive(Default)]
struct Requests {
    /// A scene to show: its kind and component count.
    show: Option<(String, usize)>,
    clear: bool,
    /// Called with `performance.now()` at the end of each frame's update.
    on_frame: Option<js_sys::Function>,
}

thread_local! {
    static REQUESTS: RefCell<Requests> = RefCell::default();
}

/// A component's fixed look: its size, color and corner radius.
struct Look {
    size: Vec2,
    color: Vec4,
    radius: f32,
}

#[derive(Default)]
struct Bench {
    components: Vec<(Handle, Look)>,
    /// The moving scene's own random numbers, for its targets, and when the
    /// last move started; `None` in a still scene.
    motion: Option<(Random, f64)>,
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

impl Bench {
    fn clear(&mut self, f: &mut Frame) {
        for (handle, _) in self.components.drain(..) {
            let _ = handle.destroy(f.proteus);
        }
        self.motion = None;
    }

    fn show(&mut self, f: &mut Frame, kind: &str, count: usize) {
        self.clear(f);
        let Some((min, max, radius)) = scene_sizes(kind) else {
            return;
        };
        let mut random = Random::new();
        for _ in 0..count {
            // The order `scene.ts` draws them in.
            let size = Vec2::new(
                random.between(min, max) as f32,
                random.between(min, max) as f32,
            );
            let (x, y) = (random.between(0.0, WIDTH), random.between(0.0, HEIGHT));
            let color = Vec4::new(
                random.next() as f32,
                random.next() as f32,
                random.next() as f32,
                1.0,
            );
            let handle = f
                .proteus
                .component(ComponentSpec::new(geometry(x, y, size, color, radius)));
            self.components.push((
                handle,
                Look {
                    size,
                    color,
                    radius,
                },
            ));
        }
        if kind == "moving" {
            self.motion = Some((Random::new(), f64::NEG_INFINITY));
        }
    }

    /// Every half second, gives each component a new random place to move to,
    /// as `motion.ts` does.
    fn move_components(&mut self, f: &mut Frame, now: f64) {
        let Some((random, moved_at)) = &mut self.motion else {
            return;
        };
        if now - *moved_at < MOVE_MS {
            return;
        }
        *moved_at = now;
        let config = TransitionConfig {
            duration: (MOVE_MS / 1000.0) as f32,
            delay: 0.0,
            easing: Easing::EaseInOutQuad,
        };
        for (handle, look) in &self.components {
            let (x, y) = (random.between(0.0, WIDTH), random.between(0.0, HEIGHT));
            let to = geometry(x, y, look.size, look.color, look.radius);
            let _ = handle.animate_to(f.proteus, to, config);
        }
    }
}

impl App for Bench {
    fn setup(&mut self, _f: &mut Frame) {}

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        let (show, clear, on_frame) = REQUESTS.with_borrow_mut(|r| {
            (
                r.show.take(),
                std::mem::take(&mut r.clear),
                r.on_frame.clone(),
            )
        });
        if clear {
            self.clear(f);
        }
        if let Some((kind, count)) = show {
            self.show(f, &kind, count);
        }
        let now = now();
        self.move_components(f, now);
        if let Some(callback) = on_frame {
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_f64(now));
        }
    }
}

/// Starts the app on the `<canvas>` with the id `canvas_id`, with room for
/// `max_instances` components.
#[wasm_bindgen]
pub async fn start(canvas_id: String, max_instances: u32) -> Result<(), JsValue> {
    let mut config = ProteusConfig::web();
    config.render.clear_color = [0.0, 0.0, 0.0, 1.0];
    config.memory.max_instances = max_instances;
    // Draw every frame, as the other renderers do. A still scene would
    // otherwise be drawn once, and its frame rate would mean nothing.
    config.frame.skip_idle_frames = false;
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Bench::default(), &canvas_id, config, services).await
}

/// Shows `count` components of the scene `kind`: "small", "large" or "moving".
#[wasm_bindgen]
pub fn show(kind: String, count: u32) {
    REQUESTS.with_borrow_mut(|r| r.show = Some((kind, count as usize)));
}

/// Removes the scene's components.
#[wasm_bindgen]
pub fn clear() {
    REQUESTS.with_borrow_mut(|r| r.clear = true);
}

/// Calls `callback` with `performance.now()` once a frame, until
/// `stop_frames`.
#[wasm_bindgen]
pub fn on_frame(callback: js_sys::Function) {
    REQUESTS.with_borrow_mut(|r| r.on_frame = Some(callback));
}

/// Stops the calls `on_frame` started.
#[wasm_bindgen]
pub fn stop_frames() {
    REQUESTS.with_borrow_mut(|r| r.on_frame = None);
}
