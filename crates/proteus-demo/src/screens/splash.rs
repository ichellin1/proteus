//! The Splash screen — the demo's entry point. Auto-advances to `Home`
//! after a hold timer once spawned; no click-driven navigation (the
//! original demo has no reverse edge back to Splash — see PLANNING.md's
//! M12.5 research).
//!
//! Placeholder pass (M12.5 Step 2, per an explicit "placeholders first"
//! call): a solid-color button, no logo animation, no fade-in — real asset
//! loading and hand-tweened intro animation are deliberately deferred to a
//! later pass rather than bundled into getting the entity/transition
//! structure right first.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState, Text};

/// How long Splash holds before auto-advancing to Home. Placeholder timing —
/// not the original demo's `INTRO_DELAY`/`INTRO_DURATION`/
/// `SPLASH_HOLD_DURATION` sequence, just a flat hold.
pub const HOLD_SECS: f32 = 2.0;

pub struct Splash {
    pub button: Handle,
    /// Not read directly yet — kept alive by its `ChildOf(button)`
    /// relationship (so it moves/transitions with `button`) and by the
    /// `Text` component it carries.
    #[allow(dead_code)]
    pub wordmark: Handle,
}

pub fn spawn(app: &mut Proteus) -> Splash {
    let button = app.component(ComponentSpec::new(QuadState {
        position: Vec3::ZERO,
        size: Vec2::new(160.0, 160.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.36, 0.31, 0.86, 1.0),
        corner_radius: 28.0,
    }));

    let wordmark = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, -110.0, 0.1),
            size: Vec2::new(160.0, 28.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            // Transparent: this quad exists only to host the baked text
            // overlay (see `collect_instances`'s two-instance-per-text-entity
            // doc). An opaque color here would paint a solid background
            // *underneath* the (by default, opaque white) glyph overlay —
            // white-on-white, invisible.
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: 0.0,
        })
        .text(Text::new("PROTEUS", 20.0)),
    );
    button.add_child(app, wordmark);

    Splash { button, wordmark }
}
