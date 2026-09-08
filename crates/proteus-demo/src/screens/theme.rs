//! The persistent light/dark theme toggle — `sun`/`moon`, a
//! mutual-exclusion pair (only whichever *doesn't* match the current theme
//! is ever `Interactable` — see `Demo::advance_theme`) anchored to the
//! viewport's top-right, mirroring `screens::nav`'s home/back icons in
//! every way (size, fade-in timing, hover) except which edge they sit on.
//!
//! **Asset note**: `sun`'s own light/dark roles are inverted from every
//! other themed pair in this crate. Sun means "light is current," and an
//! alpha-over dark ring can't occlude a solid disc underneath it — so the
//! *base* `sun` entity actually loads the dark-themed art (a thin ring)
//! and its `sun_dark` overlay child loads the light-themed art (a solid
//! disc); `Demo::advance_theme` crosses its alpha `1.0 - theme_progress`
//! instead of the usual straight `theme_progress`. `moon` needs no such
//! inversion. Mirrors `proteus-shell-native`'s own `SUN_ICON_PATH`/
//! `SUN_ICON_DARK_PATH` doc for why.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Glow, Handle, Proteus, QuadState};

/// Same row as `screens::nav`'s home/back icons — see that module's own
/// `ICON_SIZE_PX`.
pub const ICON_SIZE_PX: f32 = 56.0;
pub const ICON_CORNER_RADIUS: f32 = ICON_SIZE_PX / 2.0;
pub const MARGIN_PX: f32 = 20.0;
pub const GAP_PX: f32 = 10.0;
/// Shared with `screens::nav`'s own icon fade-in.
pub const FADE_DURATION_SECS: f32 = 0.3;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct Theme {
    pub sun: Handle,
    /// Overlay child — see the module doc for why this is the *light*-art
    /// layer despite the `_dark` naming convention every other pair here
    /// follows.
    pub sun_dark: Handle,
    pub moon: Handle,
    pub moon_dark: Handle,
}

/// Border+glyph are baked into the icon PNGs themselves (Color-light
/// treatment) — no separate `Border` component, matching
/// `screens::nav`'s own icons. Glow is real (hover), so it's attached.
fn spawn_icon(app: &mut Proteus) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.9),
            size: Vec2::splat(ICON_SIZE_PX),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: ICON_CORNER_RADIUS,
        })
        .glow(Glow {
            radius: 0.0,
            color: violet(),
            intensity: 1.0,
        }),
    )
}

fn spawn_overlay(app: &mut Proteus, parent: Handle) -> Handle {
    let overlay = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::ZERO,
            size: Vec2::splat(ICON_SIZE_PX),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: ICON_CORNER_RADIUS,
        })
        // Display-only — the base icon is the click/hover target.
        .non_interactive(),
    );
    parent.add_child(app, overlay);
    overlay
}

pub fn spawn(app: &mut Proteus) -> Theme {
    let sun = spawn_icon(app);
    let sun_dark = spawn_overlay(app, sun);
    let moon = spawn_icon(app);
    let moon_dark = spawn_overlay(app, moon);
    Theme {
        sun,
        sun_dark,
        moon,
        moon_dark,
    }
}
