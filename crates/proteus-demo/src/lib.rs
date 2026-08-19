//! `proteus-demo` — the shared, shell-agnostic Proteus reference demo
//! (M12.5).
//!
//! Built once against [`proteus_sdk::Proteus`] and linked by both
//! `proteus-shell-native` and `proteus-shell-web`, replacing what was
//! previously ~17,500 lines of independently hand-duplicated demo logic
//! across the two shells. Content lands screen by screen across M12.5's
//! staged migration (see `PLANNING.md`'s M12.5 entry). Currently live:
//! `Splash` → `Home` (placeholder colors, no real images yet — see
//! `screens::splash`/`screens::home`'s own docs).
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

use proteus_sdk::{ease_in_out_quad, Proteus, SplitStrategy, TransitionConfig, Visibility};

use screens::{home, splash};

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
    splash: splash::Splash,
    home: home::Home,
}

impl Demo {
    pub fn new() -> Self {
        let mut app = Proteus::new();
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
            splash,
            home,
        }
    }

    /// Advance one frame.
    pub fn tick(&mut self, dt: f32) {
        self.app.tick(dt);
        self.advance_state(dt);
        self.advance_label_reveal(dt);
        // Splash/Home's initial-visibility setup above, and the
        // split_to()-triggered reveal, both mutate Visibility outside the
        // normal schedule — see Proteus::refresh_cascades's doc for why
        // this second cascade pass is needed before rendering.
        self.app.refresh_cascades();
    }

    fn advance_state(&mut self, dt: f32) {
        if self.state != AppState::Splash {
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
