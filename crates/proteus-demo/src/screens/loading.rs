//! The `Loading` screen — reached from `Home`'s "Gallery" nav button (and
//! from `Gallery`'s own "Fetch New Images" button, a refetch). Just the
//! animated logo mark (the same 19-frame loop `Splash` uses, looping
//! forever here instead of playing once — see `Demo::advance_loading_logo`)
//! plus an error message shown if the fetch doesn't finish in time. The
//! actual 12-image fetch is a `Demo`-level concern (`Demo::gallery_fetch_*`
//! fields/`take_pending_gallery_fetch`), not this screen's — this module
//! only owns the three entities.
//!
//! `logo_dark` — a second, *separate* 19-frame set (`frame-NN-dark.png`,
//! injected via `Demo::set_loading_logo_frames_dark`), not a color-tinted
//! copy of the light set: `screens::splash`'s own `button`/`logo_frames`
//! never theme-crossfades (Splash always resolves before any theme choice
//! is even visible), so only this screen needs a dark set at all. Cross-
//! faded in via `theme_progress` alone (`Demo::advance_theme`), same shape
//! as `screens::nav`'s dark overlays.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState, Text};

/// The logo mark's fixed pixel footprint (not viewport-relative) — matches
/// `screens::splash`'s own logo geometry, since it's the same art.
const LOGO_SIZE: Vec2 = Vec2::new(144.44, 200.0);

pub const ERROR_TEXT: &str = "Couldn't load images";

pub struct Loading {
    pub logo: Handle,
    /// `ChildOf(logo)` — see the module doc.
    pub logo_dark: Handle,
    pub error_text: Handle,
}

pub fn spawn(app: &mut Proteus) -> Loading {
    let logo = app.component(ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: LOGO_SIZE,
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: 0.0,
    }));
    let logo_dark = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::ZERO,
            size: LOGO_SIZE,
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: 0.0,
        })
        .non_interactive(),
    );
    logo.add_child(app, logo_dark);

    let error_text = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(ERROR_TEXT, 18.0).with_color(Vec4::ONE))
        // Decorative — not a click target.
        .non_interactive(),
    );

    Loading {
        logo,
        logo_dark,
        error_text,
    }
}
