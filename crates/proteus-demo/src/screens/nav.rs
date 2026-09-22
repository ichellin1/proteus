//! Persistent chrome, top-left: the brand lockup (mark + "PROTEUS"
//! wordmark) and the home/back nav icons — real PNG assets (border+glyph
//! baked into the art itself, no separate `Border` component), each with a
//! `_dark` overlay child cross-faded in via `Demo::advance_theme`. `home`
//! also carries a "selected" overlay pair (`home_selected`/
//! `home_selected_dark`), shown only while resting on `Home`.
//!
//! Fade-in/position/hover: `Demo::advance_nav_icons`. Mirrors
//! `proteus-shell-native`'s own `nav_icons`/`logo_lockup`/
//! `home_icon_selected` spawn code and `advance_nav_icons` exactly.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Glow, Handle, Proteus, QuadState};

pub const ICON_SIZE_PX: f32 = 56.0;
pub const ICON_CORNER_RADIUS: f32 = ICON_SIZE_PX / 2.0;
pub const MARGIN_PX: f32 = 20.0;
pub const GAP_PX: f32 = 10.0;
pub const FADE_DURATION_SECS: f32 = 0.3;

/// `lockup.png`'s native size (420×92) — its aspect ratio derives the
/// on-screen width from `LOGO_HEIGHT_PX`, since `Image` doesn't carry
/// intrinsic size into `QuadState`.
const LOGO_NATIVE_WIDTH_PX: f32 = 420.0;
const LOGO_NATIVE_HEIGHT_PX: f32 = 92.0;
/// The rightmost column (measured) of the "S" in "PROTEUS" within
/// `lockup.png` — well short of `LOGO_NATIVE_WIDTH_PX` since the image
/// carries trailing whitespace. The icon row's gap is measured from here,
/// not the image's own bounding box, so it reads as a fixed gap from the
/// visible text rather than the text plus whatever blank margin the PNG
/// happens to have baked in.
const LOGO_NATIVE_TEXT_RIGHT_PX: f32 = 299.0;
/// Matches the nav icons' height so the two rows align.
pub const LOGO_HEIGHT_PX: f32 = ICON_SIZE_PX;
pub const LOGO_WIDTH_PX: f32 = LOGO_HEIGHT_PX * (LOGO_NATIVE_WIDTH_PX / LOGO_NATIVE_HEIGHT_PX);
pub const LOGO_TEXT_RIGHT_PX: f32 =
    LOGO_HEIGHT_PX * (LOGO_NATIVE_TEXT_RIGHT_PX / LOGO_NATIVE_HEIGHT_PX);
/// Gap between the visible edge of the "S" in "PROTEUS" and the home icon's
/// left edge.
pub const LOGO_ICONS_GAP_PX: f32 = 45.0;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct Nav {
    pub home: Handle,
    /// `ChildOf(home)` — cross-faded via `theme_progress` alone.
    pub home_dark: Handle,
    /// `ChildOf(home)` — "selected" art (solid-fill background, contrast
    /// glyph), shown only while resting on `Home`, hard-gated by
    /// `dark_target` against its own dark sibling (not a continuous theme
    /// lerp — see `Demo::advance_theme`'s doc).
    pub home_selected: Handle,
    pub home_selected_dark: Handle,
    pub back: Handle,
    pub back_dark: Handle,
    /// Persistent brand lockup — hidden until Splash finishes, then stays
    /// up through every other state (including `Home` itself).
    pub lockup: Handle,
    pub lockup_dark: Handle,
}

/// Border+glyph are baked into the icon PNGs themselves (Color-light
/// treatment) — no separate `Border` component. Glow is real (hover), so
/// it's attached; `Interactable` too, since these two entities are the
/// click targets (their overlay children below are display-only).
fn spawn_icon(app: &mut Proteus) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.9), // above every screen's own content (z<=0.51)
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
    let _ = parent.add_child(app, overlay);
    overlay
}

pub fn spawn(app: &mut Proteus) -> Nav {
    let home = spawn_icon(app);
    let home_dark = spawn_overlay(app, home);
    let home_selected = spawn_overlay(app, home);
    let home_selected_dark = spawn_overlay(app, home);
    let back = spawn_icon(app);
    let back_dark = spawn_overlay(app, back);

    let lockup = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.9),
            size: Vec2::new(LOGO_WIDTH_PX, LOGO_HEIGHT_PX),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: 0.0,
        })
        // Decorative brand mark, not a click target — matching
        // `proteus-shell-native::logo_lockup`'s own spawn (no `Interactable`
        // there at all). Its bounding box (the full `lockup.png`, including
        // the trailing whitespace `LOGO_TEXT_RIGHT_PX`'s doc mentions)
        // overlaps `home`'s own hit region — real fallout from a missing
        // call here, not a hypothetical: without this, clicking `home`
        // right where the two visually-adjacent-but-invisibly-overlapping
        // quads meet hits `lockup` instead, since it's spawned later and
        // hit-testing is last-hit-wins. See
        // `ComponentSpec::non_interactive`'s doc.
        .non_interactive(),
    );
    let lockup_dark = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::ZERO,
            size: Vec2::new(LOGO_WIDTH_PX, LOGO_HEIGHT_PX),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: 0.0,
        })
        .non_interactive(),
    );
    let _ = lockup.add_child(app, lockup_dark);

    Nav {
        home,
        home_dark,
        home_selected,
        home_selected_dark,
        back,
        back_dark,
        lockup,
        lockup_dark,
    }
}
