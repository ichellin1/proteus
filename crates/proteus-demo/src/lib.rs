//! `proteus-demo` — the shared, shell-agnostic Proteus reference demo
//! (M12.5).
//!
//! Built once against [`proteus_sdk::Proteus`] and linked by both
//! `proteus-shell-native` and `proteus-shell-web`, replacing what was
//! previously ~17,500 lines of independently hand-duplicated demo logic
//! across the two shells. Content lands screen by screen across M12.5's
//! staged migration (see `PLANNING.md`'s M12.5 entry). Currently live:
//! a persistent background image, then `Splash` (real animated logo +
//! wordmark + intro fade/slide-in — fidelity pass 3 of 3, see
//! `screens::splash`'s/`screens::background`'s own docs) → `Home` (still
//! placeholder colors — see `screens::home`'s doc).
//!
//! ## What stays a shell concern
//!
//! Rendering (GPU device/surface setup, `collect_instances`, the actual
//! draw call, and baking `Text`/`Image` components into the GPU atlas) is
//! **not** this crate's job — `proteus-sdk` itself is headless, and this
//! crate follows suit. A shell drives [`Demo`] with [`Demo::tick`]/pointer
//! input, then reads [`Demo::app`]'s `world()` to render. `examples/
//! native_preview.rs` is a minimal reference for how to wire this up
//! (including the text-baking step) — not part of this crate's public API,
//! just a `cargo run --example native_preview -p proteus-demo` harness for
//! visually confirming each migration step as content lands.
//!
//! Per-platform asset loading (reading files from disk, `fetch()`-ing
//! images, decoding video) also stays a shell concern — later steps add
//! `set_*`/`take_*` injection points to [`Demo`] (mirroring
//! `proteus-shell-web`'s existing wasm-bindgen surface, generalized) that
//! each shell calls with bytes/frames it fetched its own way.

mod screens;

use glam::Vec2;

use proteus_sdk::{
    ease_in_out_quad, ease_out_quad, Handle, Image, Proteus, QuadState, SplitStrategy, Text,
    TextureHandle, TransitionConfig, Visibility,
};

use screens::{background, home, splash};

/// Initial background/viewport size in logical pixels, used only until the
/// shell's first [`Demo::set_viewport_size`] call — see that method's doc.
/// Matches `examples/native_preview.rs`'s own default window size, so the
/// harness never actually shows this placeholder in practice.
const DEFAULT_VIEWPORT_SIZE: Vec2 = Vec2::new(1280.0, 800.0);

/// Config shared by the (currently only) group transition. Placeholder —
/// not the original demo's per-edge `BUTTON_TILES_MORPH_DURATION`/
/// `GALLERY_GRID_MORPH_DURATION` distinction yet.
fn group_transition_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.4,
        delay: 0.0,
        easing: ease_in_out_quad,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Splash,
    Home,
}

/// The shared reference demo application. One instance per running demo.
pub struct Demo {
    app: Proteus,
    state: AppState,
    splash_elapsed: f32,
    /// Elapsed time since the Splash→Home group transition started, or
    /// `None` when no reveal is pending. `split_to`'s `reveal_on_complete`
    /// only knows about the target entities it was given (`home.nav_buttons`)
    /// — it has no idea `home.nav_labels` are their children, so it can't
    /// reveal them too. They're tracked here instead and revealed once the
    /// transition's own duration has elapsed, mirroring what
    /// `reveal_on_complete` does internally for the buttons.
    label_reveal_elapsed: Option<f32>,
    /// Seconds remaining before the intro fade/slide-in starts —
    /// `splash::INTRO_DELAY_SECS` counting down to 0. See `Demo::advance_intro`.
    intro_delay_remaining: f32,
    /// Seconds into the intro fade/slide-in itself, clamped to
    /// `splash::INTRO_DURATION_SECS`. `Demo::advance_state`'s hold countdown
    /// doesn't start until this reaches the cap.
    intro_elapsed: f32,
    /// `splash::INTRO_SLIDE_DISTANCE_PX * (1.0 - eased_intro_progress)` —
    /// recomputed each tick by `advance_intro`, consumed by
    /// `splash::recenter`. 0 once the intro has fully settled.
    intro_slide_offset: f32,
    /// Splash's animated logo mark's pre-baked frames, in order — empty
    /// until [`Demo::set_logo_frames`] is called. Baking the PNGs is shell
    /// I/O (see [`Demo::set_logo_frames`]'s doc); cycling which one is shown
    /// is ordinary app state, tracked here.
    logo_frames: Vec<TextureHandle>,
    logo_frame_index: usize,
    logo_frame_elapsed: f32,
    /// The full-window background image — persistent chrome, not owned by
    /// any one screen. See `screens::background`'s doc for why there's no
    /// dark counterpart yet.
    background: Handle,
    splash: splash::Splash,
    home: home::Home,
}

impl Demo {
    pub fn new() -> Self {
        let mut app = Proteus::new();
        let background = background::spawn(&mut app, DEFAULT_VIEWPORT_SIZE);
        let splash = splash::spawn(&mut app);
        let home = home::spawn(&mut app);

        // Home starts hidden — it's the target of Splash's auto-advance
        // transition, which reveals it. `component()` always spawns
        // visible by default (see `proteus_sdk::Visibility`'s own doc), so
        // this has to be set explicitly up front, via the escape hatch —
        // `ComponentSpec` has no `.hidden()` builder, deliberately: initial
        // visibility for a transition *target* is this state machine's
        // concern, not something the screen's own spawn function should
        // have to know about itself.
        for &button in &home.nav_buttons {
            app.world_mut()
                .entity_mut(button.id())
                .insert(Visibility::HIDDEN);
        }
        for &label in &home.nav_labels {
            app.world_mut()
                .entity_mut(label.id())
                .insert(Visibility::HIDDEN);
        }

        Self {
            app,
            state: AppState::Splash,
            splash_elapsed: 0.0,
            label_reveal_elapsed: None,
            intro_delay_remaining: splash::INTRO_DELAY_SECS,
            intro_elapsed: 0.0,
            intro_slide_offset: splash::INTRO_SLIDE_DISTANCE_PX,
            logo_frames: Vec::new(),
            logo_frame_index: 0,
            logo_frame_elapsed: 0.0,
            background,
            splash,
            home,
        }
    }

    /// Injects the background's image bytes — the shell reads the file (or
    /// `fetch()`s it, on web) its own way and hands over the bytes; baking
    /// them into `main_atlas` is the shell's own per-frame job too (see the
    /// crate-root doc), same convention as `Text`/the logo frames. Call once,
    /// before the first `tick`.
    pub fn set_background_image(&mut self, bytes: Vec<u8>) {
        self.app
            .world_mut()
            .entity_mut(self.background.id())
            .insert(Image::new(bytes));
    }

    /// Resizes the background to cover the new viewport — call on every
    /// resize (and once up front with the real initial size, since
    /// [`Demo::new`] only has a placeholder to spawn with). Logical pixels,
    /// same convention as [`Demo::pointer_moved`].
    pub fn set_viewport_size(&mut self, size: Vec2) {
        if let Some(mut qs) = self
            .app
            .world_mut()
            .get_mut::<QuadState>(self.background.id())
        {
            qs.size = size;
        }
    }

    /// Injects Splash's animated logo mark's pre-baked frames — the shell
    /// loads/decodes/registers the 19 `frame-NN.png` files into `main_atlas`
    /// its own way (`fs::read` natively, `fetch()` on web) and wraps each
    /// with `Proteus::texture`, same shell-does-the-I/O convention as
    /// `Text`/`Image` baking (see the crate-root doc). Call once, before the
    /// first `tick`; shows `frames[0]` immediately if non-empty, matching
    /// `proteus-shell-native`'s own "start on frame 1" behavior.
    pub fn set_logo_frames(&mut self, frames: Vec<TextureHandle>) {
        self.logo_frames = frames;
        self.logo_frame_index = 0;
        self.logo_frame_elapsed = 0.0;
        if let Some(&first) = self.logo_frames.first() {
            self.splash.button.set_texture(&mut self.app, first);
        }
    }

    /// Advance one frame.
    pub fn tick(&mut self, dt: f32) {
        self.app.tick(dt);
        self.advance_intro(dt);
        self.advance_state(dt);
        self.advance_label_reveal(dt);
        self.advance_logo_animation(dt);
        splash::recenter(&mut self.app, &self.splash, self.intro_slide_offset);
        // Splash/Home's initial-visibility setup above, and the
        // split_to()-triggered reveal, both mutate Visibility outside the
        // normal schedule — see Proteus::refresh_cascades's doc for why
        // this second cascade pass is needed before rendering.
        self.app.refresh_cascades();
    }

    /// Intro fade (waits `splash::INTRO_DELAY_SECS`, then plays once,
    /// 0 → 1, never reverses) + slide-in, in lockstep. Burns off the delay
    /// first; any leftover `dt` in the same tick carries into the fade
    /// itself rather than being dropped (same pattern as `ActiveTransition`'s
    /// delay handling). Mirrors
    /// `proteus-shell-native::advance_intro_and_hover`'s fade/slide portion —
    /// hover isn't part of this crate yet.
    fn advance_intro(&mut self, dt: f32) {
        let fade_dt = if self.intro_delay_remaining > 0.0 {
            let burned = dt.min(self.intro_delay_remaining);
            self.intro_delay_remaining -= burned;
            dt - burned
        } else {
            dt
        };
        self.intro_elapsed = (self.intro_elapsed + fade_dt).min(splash::INTRO_DURATION_SECS);
        let raw_t = self.intro_elapsed / splash::INTRO_DURATION_SECS;
        let alpha = ease_out_quad(raw_t);
        self.intro_slide_offset = splash::INTRO_SLIDE_DISTANCE_PX * (1.0 - alpha);

        if let Some(mut qs) = self
            .app
            .world_mut()
            .get_mut::<QuadState>(self.splash.button.id())
        {
            qs.color.w = alpha;
        }
        if let Some(mut text) = self
            .app
            .world_mut()
            .get_mut::<Text>(self.splash.wordmark.id())
        {
            text.color.w = alpha;
        }
    }

    fn advance_state(&mut self, dt: f32) {
        if self.state != AppState::Splash {
            return;
        }
        // The countdown only starts once the intro slide/fade has fully
        // settled — "1.5 seconds to register it" is measured from when the
        // composite is actually done animating in, not from when Splash was
        // spawned.
        if self.intro_elapsed < splash::INTRO_DURATION_SECS {
            return;
        }
        self.splash_elapsed += dt;
        if self.splash_elapsed < splash::HOLD_SECS {
            return;
        }

        let button = self.splash.button;
        let targets = self.home.nav_buttons;
        button.split_to(
            &mut self.app,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        self.state = AppState::Home;
        self.label_reveal_elapsed = Some(0.0);
    }

    /// Reveals `home.nav_labels` once the Splash→Home transition's own
    /// duration has elapsed — see `label_reveal_elapsed`'s doc for why this
    /// can't just be part of `split_to`'s own reveal list.
    fn advance_label_reveal(&mut self, dt: f32) {
        let Some(elapsed) = self.label_reveal_elapsed.as_mut() else {
            return;
        };
        *elapsed += dt;
        if *elapsed < group_transition_config().duration {
            return;
        }
        for &label in &self.home.nav_labels {
            self.app
                .world_mut()
                .entity_mut(label.id())
                .insert(Visibility::VISIBLE);
        }
        self.label_reveal_elapsed = None;
    }

    /// Advances the logo's frame-sweep animation while the button is idle
    /// (waiting for a click) — swaps which pre-baked frame's texture sits on
    /// `splash.button`, wrapping through `logo_frames` every
    /// `splash::LOGO_FRAME_DURATION` seconds. Stops once Splash has handed
    /// off to Home: the button is either mid-morph (its current frame gets
    /// baked into the Slice transition's snapshot, same as any other texture
    /// content) or already hidden, so there's nothing left to animate.
    /// Mirrors `proteus-shell-native::advance_logo_animation` exactly.
    fn advance_logo_animation(&mut self, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Splash {
            return;
        }
        self.logo_frame_elapsed += dt;
        while self.logo_frame_elapsed >= splash::LOGO_FRAME_DURATION {
            self.logo_frame_elapsed -= splash::LOGO_FRAME_DURATION;
            self.logo_frame_index = (self.logo_frame_index + 1) % self.logo_frames.len();
            let frame = self.logo_frames[self.logo_frame_index];
            self.splash.button.set_texture(&mut self.app, frame);
        }
    }

    /// Pointer position in **world-space** (viewport-center origin, Y-up) —
    /// see [`proteus_sdk::Proteus::pointer_moved`]'s doc for the exact
    /// contract and the conversion a caller needs from window/CSS pixels.
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.app.pointer_moved(pos);
    }

    pub fn pointer_pressed(&mut self) {
        self.app.pointer_pressed();
    }

    pub fn pointer_released(&mut self) {
        self.app.pointer_released();
    }

    /// Read-only access to the underlying [`Proteus`] app — for reading
    /// component state (`Proteus::get`) or, from a shell, rendering via
    /// `Proteus::world()`.
    pub fn app(&self) -> &Proteus {
        &self.app
    }

    /// Mutable access to the underlying [`Proteus`] app — for the escape
    /// hatch (`Proteus::world_mut()`) this crate itself uses internally,
    /// and that a shell needs for GPU resource setup (`GpuContext`/
    /// `QuadPipeline`), baking `Text`/`Image` components, and calling
    /// `refresh_cascades()` before rendering.
    pub fn app_mut(&mut self) -> &mut Proteus {
        &mut self.app
    }
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}
