//! The full-window background image — persistent across every screen, not
//! specific to any one of them. Spawned once by `Demo::new()`; never hidden
//! or touched by state transitions — the light/dark pair is the only thing
//! that ever changes about it.
//!
//! `dark` crossfades in over `light` as `Demo`'s `theme_progress` ramps
//! toward 1 (`Demo::advance_theme`) — unconditionally, every frame,
//! regardless of `AppState`, mirroring the original exactly (its own doc:
//! "so a component already reflects the current theme by the time it
//! becomes visible is true for free, with no visibility branching needed").

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState};

pub struct Background {
    pub light: Handle,
    /// A `ChildOf(light)` overlay at the same position/size, transparent at
    /// rest — `Demo::advance_theme` drives its alpha straight to
    /// `theme_progress` every frame.
    pub dark: Handle,
}

/// `size` is the initial viewport size in logical pixels — the caller should
/// follow up with [`crate::Demo::set_viewport_size`] on every resize (see
/// its doc), same as this crate's other size-dependent state.
pub fn spawn(app: &mut Proteus, size: Vec2) -> Background {
    let quad = |color: Vec4| QuadState {
        position: Vec3::ZERO,
        size,
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color,
        corner_radius: 0.0,
    };
    // Passive backdrop, not a button, on both — see `.non_interactive()`'s
    // doc: left interactive, this full-viewport quad would win
    // hit-testing's "last hit wins, matches draw order" tie-break against
    // everything underneath it once it (like most content here) ends up in
    // a later-iterated archetype, silently swallowing every click on
    // screen. Exactly what broke Home's "Examples" button.
    let light = app.component(ComponentSpec::new(quad(Vec4::ONE)).non_interactive());
    let dark =
        app.component(ComponentSpec::new(quad(Vec4::new(1.0, 1.0, 1.0, 0.0))).non_interactive());
    let _ = light.add_child(app, dark);
    Background { light, dark }
}
