//! The full-window background image — persistent across every screen, not
//! specific to any one of them. Spawned once by `Demo::new()`; never hidden
//! or touched by state transitions (matches
//! `proteus-shell-native`'s own `background` entity).
//!
//! Fidelity pass 3 scope: light treatment only. The original demo also
//! crossfades in a dark counterpart via a `theme_progress` value driven by a
//! toggle icon that lives among Home's nav icons. That icon (and any other
//! theme-toggle UI) hasn't been migrated yet, so there's no way to ever move
//! `theme_progress` off 0 — a dark layer would be permanently-invisible dead
//! weight. Deferred to whichever step migrates that icon, rather than wired
//! up now with nothing to drive it.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState};

/// `size` is the initial viewport size in logical pixels — the caller should
/// follow up with [`crate::Demo::set_viewport_size`] on every resize (see
/// its doc), same as this crate's other size-dependent state.
pub fn spawn(app: &mut Proteus, size: Vec2) -> Handle {
    app.component(ComponentSpec::new(QuadState {
        position: Vec3::ZERO,
        size,
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: 0.0,
    }))
}
