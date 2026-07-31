//! `proteus-shell-native` — native desktop entry point.
//!
//! ## Demo redesign (in progress)
//!
//! The reference demo is being rebuilt from scratch. This is step 1: the
//! entry screen — the animated Proteus mark (brand/animated-logo,
//! color-light treatment) plus the "PROTEUS" wordmark, treated as one
//! composite standing in for a "START" button.
//!
//! - Renders at `COMPOSITE_SCALE` (75%) and is centered on screen as a whole
//!   (mark + wordmark together, not just the mark).
//! - Slides in from the left while it fades in on load (opacity 0 → 1, same
//!   eased curve driving both — see `advance_intro_and_hover`).
//! - Continuously sweeps through its 19-frame hatch animation while idle
//!   (see `advance_logo_animation`) — the loading-animation treatment from
//!   the brand spec, repurposed as an idle/waiting-for-click affordance.
//! - Glows on hover: halo radius animates 0 → 15 px over 1 s while the
//!   pointer is over the button, and reverses (from wherever it currently is)
//!   over 1 s when the pointer leaves.
//! - Click behavior is unchanged from the previous navy-circle "START"
//!   button: it still spreads into the three video tiles via the same
//!   `OneToNRequest` Slice transition.
//!
//! Subsequent steps will add the next scene(s) on top of this.
//!
//! ## Frame order each tick
//!
//! 1. Compute delta time (capped at 50 ms).
//! 2. Flush staged pointer events → `PointerInput`.
//! 3. `ui_world.update(dt)` — full ECS schedule (hit test, transitions).
//! 4. `bake_pending_text()` — rasterise any Text entities that lack BakedText.
//! 5. `bake_pending_images()` — decode any Image entities that lack BakedImage.
//! 6. Advance intro fade + hover glow (drives `QuadState`/`Glow` directly) and
//!    the logo's frame-sweep animation (drives `BakedImage`/`TextureRef`).
//! 7. Collect visible `QuadState`s → `QuadInstance`s.
//! 8. GPU render pass.

mod gallery_fetch;
mod mp4_player;

use std::sync::Arc;
use std::time::Instant;

use glam::{Vec2, Vec3, Vec4};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

use proteus_render::{FontAtlas, GpuContext, QuadPipeline, TextureId, MAIN_ATLAS_SIZE};
use proteus_ui::{
    collect_instances, ease_in_out_quad, ease_out_quad, transition::TransitionConfig, BakedImage,
    BakedText, Border, ChildOf, Entity, Glow, GroupSource, GroupTarget, HoveredEntity, Image,
    Interactable, InteractionEvents, Lifecycle, NToOneRequest, OneToNRequest, PointerInput,
    ProteusWorld, QuadState, SplitStrategy, Text, TextureRef, TransitionRequest, VideoCrossfade,
    VideoPlayer, Visibility,
};

// ---------------------------------------------------------------------------
// Design tokens
// ---------------------------------------------------------------------------

/// App background — #cdc7ed. Only visible at the very edges/before
/// `BG_IMAGE_PATH` loads — the background image is sized to cover the whole
/// window every frame (see `advance_background`).
const BG_COLOR: wgpu::Color = wgpu::Color {
    r: 0xCD as f64 / 255.0,
    g: 0xC7 as f64 / 255.0,
    b: 0xED as f64 / 255.0,
    a: 1.0,
};

/// Full-window background image, behind every other entity (z=0.0, lower
/// than everything else's 0.5+ — see `collect_instances`' z-ordering).
/// Stretched to exactly fill the window on every resize; distorting off its
/// native aspect ratio is intentional, not a bug.
const BG_IMAGE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/bg/ocean-blur.jpg");
/// Color-dark counterpart, cross-faded in via `background_dark` — see
/// `advance_theme`.
const BG_IMAGE_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/bg/ocean-blur-dark.jpg");

/// Design System — Color-light treatment primary (#735acc). Border, glow,
/// and idle text/icon color all draw from this one value, per the UI Design
/// System Spec ("there are no separate per-component colors"). Also the
/// mark/wordmark's own color (see `violet()`'s original use), and the video
/// tiles/screen's border/glow color.
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// Design System — Color-dark treatment primary (#b6a8ff). Used as the video
/// tile's hover-label color: the "opposite treatment's primary" for
/// Color-light is Color-dark.
fn violet_dark() -> Vec4 {
    Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
}

/// Border and label color — white.
fn white() -> Vec4 {
    Vec4::ONE
}

const BORDER_WIDTH: f32 = 5.0;

/// Seconds to wait before the entry fade begins.
const INTRO_DELAY: f32 = 1.0;
/// Seconds for the entry fade (opacity 0 → 1).
const INTRO_DURATION: f32 = 0.6;
/// Seconds the splash holds, fully settled, before auto-advancing to tiles.
const SPLASH_HOLD_DURATION: f32 = 1.5;
/// Seconds for a full 0 → 30 px (or 30 → 0 px) hover glow sweep.
const GLOW_DURATION: f32 = 0.33;
const GLOW_MAX_RADIUS: f32 = 15.0;
/// Design System: "5% scale-up" on hover/focus for buttons, tiles, and icon buttons.
const HOVER_SCALE_BOOST: f32 = 0.05;

/// Seconds for the button ↔ tiles morph, either direction.
const BUTTON_TILES_MORPH_DURATION: f32 = 0.4;

// ---------------------------------------------------------------------------
// Animated logo (replaces the old navy-circle "START" button)
// ---------------------------------------------------------------------------
//
// brand/animated-logo/Loading Animation Spec.dc.html: a 19-frame loop, one
// hatch band fading to 60% opacity at a time, sweeping the diagonal. Frames
// are pre-baked into main_atlas once at startup (eternal — see the
// eviction-safety note on `TextureRegistry::register_static`, this content
// must never be evicted mid-loop) and cycled by swapping which frame's
// `BakedImage`/`TextureRef` sits on the button entity.

/// color-light treatment (Deep Violet mark, white hatch) — reads with the
/// most contrast against this app's mid-gray (#BBBBBB) background; see the
/// Brand Spec's color table for the other three treatments.
const LOGO_FRAME_COUNT: usize = 19;

/// Brand spec's suggested playback rate (~11fps).
const LOGO_FRAME_DURATION: f32 = 0.09;

/// Source frame art (208×288) is noticeably larger than the mark's on-screen
/// footprint (144×200, `LOGO_MARK_WIDTH`/`LOGO_MARK_HEIGHT`) — downscale
/// before packing into `main_atlas`, same reasoning as `MAX_TILE_IMAGE_SIDE`.
/// Matters much more here than for a single tile: adding the color-dark
/// frame set (for the Loading screen's theme-crossfaded logo) doubled this
/// animation's atlas footprint to 38 eternal entries, which at native
/// resolution alone exceeded the atlas's remaining capacity (confirmed via
/// a real `main_atlas full` failure on an unrelated tile at startup).
const LOGO_FRAME_MAX_SIDE: u32 = 220;

/// The mark's native aspect ratio is 104:144 (13:18, portrait) — the button
/// keeps roughly the same on-screen footprint as the old 200px-diameter
/// circle by matching its height.
const LOGO_MARK_HEIGHT: f32 = 200.0;
const LOGO_MARK_WIDTH: f32 = LOGO_MARK_HEIGHT * 104.0 / 144.0;

/// `images/logo/frame-01.png` … `frame-19.png`, resolved relative to the
/// crate manifest so `cargo run` works from any working directory.
fn logo_frame_path(n: usize) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo"))
        .join(format!("frame-{n:02}.png"))
}

/// Color-dark counterpart of `logo_frame_path`, used only by the Loading
/// screen's `loading_logo_dark` overlay (Splash's own `button`/`logo_frames`
/// never theme-crossfades, so it only ever needed the light set).
fn logo_frame_dark_path(n: usize) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo"))
        .join(format!("frame-{n:02}-dark.png"))
}

// ---------------------------------------------------------------------------
// Wordmark — "PROTEUS", to the right of the animated mark
// ---------------------------------------------------------------------------
//
// A `Text` child of the button (M10 composition — same pattern as the old
// "START" label, tile titles, etc.), not a replacement for the mark itself.
// Brand Spec's Typography section: Inter weight 700 (the embedded font is
// exactly this — see font_atlas.rs), all caps, letter-spacing 0.06em, same
// color as the mark. Sized by scaling the pre-built reference lockup
// (`brand/logo/assets/lockup-color-light.svg`, mark 144×0.305≈43.9px tall,
// wordmark font-size 20px, gap 14.3px) up to this demo's LOGO_MARK_HEIGHT —
// preserves the reference's proportions rather than guessing new ones.

const WORDMARK_TEXT: &str = "PROTEUS";
const WORDMARK_SIZE_PX: f32 = 90.0;
/// Brand Spec: "letter-spacing 0.06em" — 0.06 × font size.
const WORDMARK_LETTER_SPACING_PX: f32 = WORDMARK_SIZE_PX * 0.06;
/// Gap between the mark's right edge and the wordmark's left edge.
const WORDMARK_GAP_PX: f32 = 65.0;

/// Mark + wordmark render as one composite (see `advance_intro_and_hover`'s
/// centering/slide-in logic) at this fraction of their natural size.
/// Applied as `button`'s own `QuadState::scale` rather than shrinking every
/// size/gap constant above: the wordmark is a *child* of `button`, and
/// `hierarchy::compose_with_parent` already scales a child's local
/// position/size by the parent's composed scale, so one `scale` factor moves
/// and shrinks both the mark and the wordmark together, correctly, for free.
/// It also means the wordmark still bakes at its full `WORDMARK_SIZE_PX`
/// resolution and is only *displayed* smaller — crisper than rasterizing
/// directly at the smaller size.
const COMPOSITE_SCALE: f32 = 0.75;

/// How far left of its resting (centered) position the composite starts,
/// sliding in as it fades in — see `advance_intro_and_hover`.
const INTRO_SLIDE_DISTANCE_PX: f32 = 250.0;

// ---------------------------------------------------------------------------
// Video tiles — placeholder "box cover" art
// ---------------------------------------------------------------------------
//
// No image-loading pipeline exists yet (no PNG/JPEG decode, no static-texture
// atlas upload — only solid colors, baked SDF text, offscreen bakes, and
// streamed video frames). Until that's built, each tile is a solid-color
// placeholder standing in for real box art, labeled with the video's title.

const TILE_WIDTH: f32 = 200.0;
/// Placeholder "poster" aspect ratio (2:3 width:height) — uniform across all
/// three tiles since there's no real per-title box art to derive it from yet.
const TILE_HEIGHT: f32 = TILE_WIDTH * 1.5;
const TILE_GAP: f32 = 100.0;
const TILE_CORNER_RADIUS: f32 = 20.0;
/// Design System — Color-dark treatment corner radius. See `advance_theme`.
const TILE_CORNER_RADIUS_DARK: f32 = 30.0;

const TILE_COLORS: [Vec4; 3] = [
    Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Big Buck Bunny
    Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
    Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
];

/// `.mp4` files backing each tile's playback (M9.5). Place these under
/// `crates/proteus-shell-native/assets/videos/` with these exact filenames —
/// resolved relative to the crate manifest so `cargo run` works from any
/// working directory. Missing files degrade gracefully: `start_video_playback`
/// logs a warning and the tile↔screen morph still runs, just without video.
const TILE_VIDEO_PATHS: [&str; 3] = [
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/videos/big_buck_bunny_fixed.mp4"
    ),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/videos/sintel_fixed.mp4"
    ),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/videos/jellyfish_fixed.mp4"
    ),
];

/// Box-cover images backing each tile (M9.7). Place PNG/JPEG files under
/// `crates/proteus-shell-native/images/` with these exact filenames —
/// resolved relative to the crate manifest so `cargo run` works from any
/// working directory. Missing/unreadable files degrade gracefully: the tile
/// keeps its solid `TILE_COLORS` fill instead of an `Image` component.
const TILE_IMAGE_PATHS: [&str; 3] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/Big_buck_bunny.jpg"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/sintel.jpg"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/jellyfish.jpg"),
];

/// Title shown in the hover overlay (M10) — each tile's `tile_labels[idx]`
/// `Text` child.
const TILE_TITLES: [&str; 3] = ["Big Buck Bunny", "Sintel", "Jellyfish"];

/// Hover-overlay label size.
const TILE_LABEL_SIZE_PX: f32 = 18.0;
const TILE_LABEL_LETTER_SPACING_PX: f32 = TILE_LABEL_SIZE_PX * 0.02;

/// Extra multiplier applied to the title label's `QuadState::scale` (M10)
/// while its tile is showing as the video screen rather than a grid tile —
/// the baked glyph run itself stays the same size (re-baking text at a
/// different size isn't something the render path does per-frame), but
/// `scale` composes multiplicatively down the hierarchy same as position/
/// rotation (see `hierarchy::compose_with_parent`), so bumping the label
/// child's own local `scale` is enough to render it visibly larger against
/// the much bigger screen without touching the baked texture at all.
const TILE_LABEL_SCREEN_SCALE: f32 = 1.8;

/// Fully-faded-in opacity of the black hover overlay (M10). The overlay's own
/// alpha animates `0.0 → TILE_OVERLAY_MAX_ALPHA` on hover-enter and back on
/// hover-exit, driven by the same `tile_hover_progress` sweep that already
/// drives the hover glow — same duration, same easing (linear step), just a
/// different destination component.
const TILE_OVERLAY_MAX_ALPHA: f32 = 0.5;

/// Real box-cover photos are routinely far larger than the tiles' on-screen
/// footprint (200×300) — cap decoded images to this before packing them into
/// `main_atlas` (2048×2048, shared with baked text), which they'd otherwise
/// not fit in at all or would starve of remaining space. Also shared by the
/// full-window background crossfade layers (`advance_background`) for the
/// same reason. Was 600 (1.5x the tile height) — dropped to 400 once the
/// light/dark theme toggle added two more background layers plus a full
/// icon set competing for the same atlas: at 600, the two backgrounds plus
/// the three tile images alone didn't fit, and `bake_pending_images` has no
/// failure backoff, so an entity that can't fit retries its full
/// decode+resize+register attempt every single frame forever (a `main_atlas
/// full` warning spamming every frame is the tell). The atlas itself can't
/// be grown to compensate — see `DEFAULT_MAIN_ATLAS_SIZE`'s doc.
const MAX_TILE_IMAGE_SIDE: u32 = 400;

// ---------------------------------------------------------------------------
// Photo gallery — fetched from picsum.photos, shown as a 4x3 grid
// ---------------------------------------------------------------------------

const GALLERY_COLS: usize = 4;
const GALLERY_ROWS: usize = 3;
const GALLERY_MARGIN_LEFT: f32 = 40.0;
const GALLERY_MARGIN_RIGHT: f32 = 40.0;
const GALLERY_MARGIN_TOP: f32 = 40.0;
/// Extra room below the grid for the "Fetch New Images" button.
const GALLERY_MARGIN_BOTTOM: f32 = 100.0;
/// Fixed, not "at least" — the spec's "at least 20px" is satisfied as a
/// floor by always using exactly this value; cell size is derived from
/// whatever space remains after margins + these fixed gaps.
const GALLERY_GAP_PX: f32 = 20.0;
const GALLERY_CORNER_RADIUS: f32 = 12.0;
const GALLERY_CORNER_RADIUS_DARK: f32 = 18.0;
/// A touch slower than `BUTTON_TILES_MORPH_DURATION` — a bigger, more
/// deliberate fan-out (1↔12 vs 1↔3).
const GALLERY_GRID_MORPH_DURATION: f32 = 0.6;
/// How long to wait for all 12 images to finish fetching/baking before
/// giving up and showing `LOADING_LOGO_ERROR_TEXT` instead.
const GALLERY_FETCH_TIMEOUT: f32 = 10.0;
const GALLERY_FETCH_BUTTON_LABEL: &str = "Fetch New Images";
const LOADING_LOGO_ERROR_TEXT: &str = "Couldn't load images";
/// Separate, much smaller cap than `MAX_TILE_IMAGE_SIDE` — that constant's
/// budget was sized for a handful of images resident at once (2 backgrounds
/// plus 3 tiles); 12 gallery images all need to be simultaneously resident
/// too, and requesting them at 400px each (confirmed empirically: the
/// on-screen cell size at typical/retina window sizes clamps to exactly
/// 400) filled `main_atlas` before even the first one could register — the
/// same no-backoff retry-every-frame failure mode `MAX_TILE_IMAGE_SIDE`'s
/// own doc describes, reproduced via a temporary auto-trigger and `ps` CPU
/// sampling (pegged at ~100%, matching the earlier session's hang bug
/// signature).
const GALLERY_IMAGE_MAX_SIDE: u32 = 150;

// ---------------------------------------------------------------------------
// Nav buttons — splash morphs into these; clicking "Video Demo" morphs into tiles
// ---------------------------------------------------------------------------

const NAV_BUTTON_TITLES: [&str; 3] = ["Video Demo", "Photo Gallery", "Examples & Tests"];
const NAV_BUTTON_LABEL_SIZE_PX: f32 = 24.0;
const NAV_BUTTON_LETTER_SPACING_PX: f32 = NAV_BUTTON_LABEL_SIZE_PX * 0.02;
const NAV_BUTTON_PADDING_PX: f32 = 15.0;
const NAV_BUTTON_GAP_PX: f32 = 50.0;
const NAV_BUTTON_CORNER_RADIUS: f32 = 20.0;
/// Design System — Color-dark treatment corner radius. See `advance_theme`.
const NAV_BUTTON_CORNER_RADIUS_DARK: f32 = 30.0;
/// Fallback size if a button's label somehow hasn't baked yet when the
/// splash→nav transition fires (shouldn't happen in practice — text bakes
/// within the first frame or two, long before the splash's hold timer fires).
const NAV_BUTTON_FALLBACK_SIZE: Vec2 = Vec2::new(150.0, 46.0);

// ---------------------------------------------------------------------------
// Home/back nav icons — top-left, visible whenever tiles/screen are showing
// ---------------------------------------------------------------------------

const NAV_ICON_SIZE_PX: f32 = 56.0;
const NAV_ICON_CORNER_RADIUS: f32 = NAV_ICON_SIZE_PX / 2.0;
const NAV_ICON_MARGIN_PX: f32 = 20.0;
const NAV_ICON_GAP_PX: f32 = 10.0;
const NAV_ICON_FADE_DURATION: f32 = 0.3;
/// Gap between the icon row and the video screen — the screen must resize to
/// respect this, see `video_screen_quad`.
const SCREEN_NAV_CLEARANCE_PX: f32 = 20.0;
/// Vertical space the icon row + its clearance reserve at the top of the
/// window — used to cap the video screen's height so it never overlaps them.
const ICON_ROW_RESERVED_PX: f32 = NAV_ICON_MARGIN_PX + NAV_ICON_SIZE_PX + SCREEN_NAV_CLEARANCE_PX;

const HOME_ICON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/home-idle.png");
const BACK_ICON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/back-idle.png");
/// "Selected" home icon art (solid-fill background, contrast glyph) — a
/// child of `nav_icons[0]`, cross-faded in over the idle art via its own
/// alpha rather than swapping the base entity's texture. See
/// `advance_nav_icons`'s `home_selected_target`.
const HOME_ICON_SELECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-selected.png"
);
/// Color-dark counterparts, cross-faded in via `home_icon_dark`/
/// `back_icon_dark`/`home_icon_selected_dark` — see `advance_theme`.
const HOME_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-idle-dark.png"
);
const BACK_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/back-idle-dark.png"
);
const HOME_ICON_SELECTED_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-selected-dark.png"
);

// ---------------------------------------------------------------------------
// Persistent brand lockup — mark + "PROTEUS" wordmark, top-left
// ---------------------------------------------------------------------------

/// `lockup.png`'s native size (420×92) — its aspect ratio derives the
/// on-screen width from `LOGO_HEIGHT_PX`, since `Image` doesn't carry
/// intrinsic size into `QuadState`.
const LOGO_NATIVE_WIDTH_PX: f32 = 420.0;
const LOGO_NATIVE_HEIGHT_PX: f32 = 92.0;
/// The rightmost column (measured) of the "S" in "PROTEUS" within
/// `lockup.png` — well short of `LOGO_NATIVE_WIDTH_PX` since the image
/// carries trailing whitespace. The icon row's gap is measured from here,
/// not from the image's own bounding box, so it reads as 30px from the
/// visible text rather than 30px plus however much blank margin the PNG
/// happens to have baked in.
const LOGO_NATIVE_TEXT_RIGHT_PX: f32 = 299.0;
/// Matches the nav icons' height so the two rows align.
const LOGO_HEIGHT_PX: f32 = NAV_ICON_SIZE_PX;
const LOGO_WIDTH_PX: f32 = LOGO_HEIGHT_PX * (LOGO_NATIVE_WIDTH_PX / LOGO_NATIVE_HEIGHT_PX);
const LOGO_TEXT_RIGHT_PX: f32 =
    LOGO_HEIGHT_PX * (LOGO_NATIVE_TEXT_RIGHT_PX / LOGO_NATIVE_HEIGHT_PX);
/// Gap between the visible edge of the "S" in "PROTEUS" and the home icon's
/// left edge.
const LOGO_ICONS_GAP_PX: f32 = 45.0;

const LOGO_LOCKUP_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo/lockup.png");
/// Color-dark counterpart, cross-faded in via `logo_lockup_dark` — see
/// `advance_theme`.
const LOGO_LOCKUP_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo/lockup-dark.png");

// ---------------------------------------------------------------------------
// Light/dark theme toggle — sun/moon icons, top-right
// ---------------------------------------------------------------------------

/// Sun/moon reuse the home/back icons' exact size/margin/gap — same row,
/// mirrored to the right edge (see `advance_theme`'s positioning step).
const THEME_ICON_SIZE_PX: f32 = NAV_ICON_SIZE_PX;
const THEME_ICON_MARGIN_PX: f32 = NAV_ICON_MARGIN_PX;
const THEME_ICON_GAP_PX: f32 = NAV_ICON_GAP_PX;
/// A touch slower than `BUTTON_TILES_MORPH_DURATION` — this is a bigger,
/// more deliberate showcase moment (the whole app re-themes at once), not
/// just one shape morphing into another.
const THEME_MORPH_DURATION: f32 = 0.6;

/// Sun is always "selected" while the light theme is current (it's
/// non-interactive there — see the Design Spec, sun/moon "selected" marks
/// the active mode) — so its permanent light-theme look is `sun-selected.png`
/// (a solid-fill disc), and its permanent dark-theme look is `sun-idle.png`'s
/// dark counterpart (a thin, mostly-transparent ring).
///
/// Because of that shape mismatch, sun is the one icon where `sun_icon`
/// (the base) and `sun_icon_dark` (the `ChildOf` overlay, alpha-crossfaded by
/// `theme_progress` in `advance_theme`) can't just hold "light art"/"dark
/// art" respectively the way every other themed pair in this file does.
/// Alpha-over compositing can only *occlude* what's beneath it where the top
/// layer's own pixels are opaque — a mostly-transparent ring drawn on top of
/// a solid disc still lets that disc show through, no matter what the
/// overlay's *entity*-level alpha is. So here the roles are inverted from
/// what the names suggest: `SUN_ICON_PATH` (loaded onto the always-visible
/// `sun_icon` base) holds the thin dark-theme ring, and `SUN_ICON_DARK_PATH`
/// (loaded onto the `sun_icon_dark` overlay) holds the solid light-theme
/// disc — with its crossfade driven by `1.0 - theme_progress` instead of
/// `theme_progress` directly, so it's fully opaque (correctly hiding the
/// ring beneath) while light, and fades away to reveal the ring as the
/// theme goes dark. Moon doesn't need this: its solid "selected" art is
/// already the dark-theme look, so it's already the one sitting in the
/// overlay slot on top, occluding correctly with the ordinary
/// `theme_progress`-driven alpha below.
const SUN_ICON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/sun-idle-dark.png"
);
const SUN_ICON_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/sun-selected.png");
/// Moon's permanent light-theme look: idle-but-hoverable (light theme makes
/// moon the active, clickable icon).
const MOON_ICON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/moon-idle.png");
/// Moon is always "selected" while the dark theme is current (non-interactive
/// there), mirroring sun's role in light. Already the solid-fill art, so
/// (unlike sun) it can sit in the overlay slot with the ordinary
/// `theme_progress`-driven alpha — see `SUN_ICON_PATH`'s doc for why sun
/// needs the inverted treatment and this one doesn't.
const MOON_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/moon-selected-dark.png"
);

// ---------------------------------------------------------------------------
// Demo scene geometry
// ---------------------------------------------------------------------------

/// The animated logo mark, standing in for the old "START" button — alpha 0
/// (fades in via `advance_intro_and_hover`), rendered at `COMPOSITE_SCALE`.
/// `position.x` starts at 0 but is corrected every frame once the wordmark's
/// baked width is known, to center the mark+wordmark composite as a whole
/// (see `advance_intro_and_hover`) — not just the mark. No added
/// fill/border/corner-radius: the Brand Spec says not to add outlines around
/// the mark, and the mark's own 13:18 rectangle already fills its whole quad
/// (two triangles meeting at the diagonal — see the Brand Spec's "Geometry"
/// section), so an untinted, unrounded quad renders it as-is.
fn start_button_quad() -> QuadState {
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(LOGO_MARK_WIDTH, LOGO_MARK_HEIGHT),
        rotation: 0.0,
        scale: COMPOSITE_SCALE,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 1.0, 1.0, 0.0), // untinted; starts transparent
        corner_radius: 0.0,
    }
}

/// Color-light treatment hover glow — nav buttons and nav/back icons.
fn nav_hover_glow() -> Glow {
    Glow {
        radius: 0.0, // animated by advance_nav_hover / advance_nav_icons
        color: violet(),
        intensity: 1.0,
    }
}

/// Color-light treatment hover glow — video tiles/screen.
fn tile_hover_glow() -> Glow {
    Glow {
        radius: 0.0, // animated by advance_tile_hover
        color: violet(),
        intensity: 1.0,
    }
}

/// One of the three video tiles the button spreads into.
/// `idx` 0 = left, 1 = center, 2 = right. `theme_progress` must be the
/// caller's current theme progress (0=light, 1=dark) — a Slice group
/// transition bakes its target's appearance once, at setup time, and that
/// bake is never revisited, so a hardcoded light-theme radius here would
/// freeze a stale corner radius into the bake; the real tile (corrected
/// every frame by `advance_theme`) then shows the correct dark radius the
/// instant the transition completes, producing a visible "snap." Passing the
/// live value keeps the bake theme-correct from the start.
fn tile_quad(idx: usize, theme_progress: f32) -> QuadState {
    // Center-to-center spacing = tile width + the requested 100px edge gap.
    let spacing = TILE_WIDTH + TILE_GAP;
    let x = (idx as f32 - 1.0) * spacing;
    QuadState {
        position: Vec3::new(x, 0.0, 0.5),
        size: Vec2::new(TILE_WIDTH, TILE_HEIGHT),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: TILE_COLORS[idx],
        corner_radius: TILE_CORNER_RADIUS
            + (TILE_CORNER_RADIUS_DARK - TILE_CORNER_RADIUS) * theme_progress,
    }
}

/// Square cell size for the photo gallery's 4x3 grid, given the current
/// window and the fixed margins/gaps — the smaller of the two axis-derived
/// sizes, so neither axis's margin/gap spec is ever violated by the tighter
/// axis (same "derive from whichever axis is more constraining" approach as
/// `video_screen_quad`).
fn gallery_cell_size(window_width: f32, window_height: f32) -> f32 {
    let usable_w = (window_width
        - GALLERY_MARGIN_LEFT
        - GALLERY_MARGIN_RIGHT
        - (GALLERY_COLS - 1) as f32 * GALLERY_GAP_PX)
        .max(0.0);
    let usable_h = (window_height
        - GALLERY_MARGIN_TOP
        - GALLERY_MARGIN_BOTTOM
        - (GALLERY_ROWS - 1) as f32 * GALLERY_GAP_PX)
        .max(0.0);
    (usable_w / GALLERY_COLS as f32).min(usable_h / GALLERY_ROWS as f32)
}

/// One of the 12 photo gallery tiles. `idx` 0..12, row-major (row = idx/4,
/// col = idx%4; row 0 = top). The whole grid block is centered on the
/// window (same centering convention as `tile_quad`/`layout_nav_buttons`),
/// so the margins act as a floor on cell size, not a literal offset.
fn gallery_cell_quad(
    idx: usize,
    window_width: f32,
    window_height: f32,
    theme_progress: f32,
) -> QuadState {
    let cell = gallery_cell_size(window_width, window_height);
    let (row, col) = (idx / GALLERY_COLS, idx % GALLERY_COLS);
    let grid_w = GALLERY_COLS as f32 * cell + (GALLERY_COLS - 1) as f32 * GALLERY_GAP_PX;
    let grid_h = GALLERY_ROWS as f32 * cell + (GALLERY_ROWS - 1) as f32 * GALLERY_GAP_PX;
    let x = -grid_w / 2.0 + cell / 2.0 + col as f32 * (cell + GALLERY_GAP_PX);
    let y = grid_h / 2.0 - cell / 2.0 - row as f32 * (cell + GALLERY_GAP_PX);
    QuadState {
        position: Vec3::new(x, y, 0.5),
        size: Vec2::splat(cell),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: GALLERY_CORNER_RADIUS
            + (GALLERY_CORNER_RADIUS_DARK - GALLERY_CORNER_RADIUS) * theme_progress,
    }
}

/// Color-light inner border for each tile. Full alpha immediately — tiles
/// appear via the button-spread morph, not a separate fade.
fn tile_border() -> Border {
    Border {
        width: BORDER_WIDTH,
        color: violet(),
        offset: -1.0,
    }
}

/// Color-light inner border for the nav buttons.
fn nav_button_border() -> Border {
    Border {
        width: BORDER_WIDTH,
        color: violet(),
        offset: -1.0,
    }
}

/// Hover-overlay `Text` + black-tint children (M10) — a `Quad` (tile) parent
/// with two `Text`/plain-`Quad` children, composed via `ChildOf` rather than
/// the M5 single-entity shortcut. Both children declare a zero relative
/// offset (centered on the tile, same coordinate space the tile itself is
/// declared in) and start fully transparent; `advance_tile_hover` animates
/// their alpha in lockstep with the existing hover glow sweep. Cascading
/// visibility (M10) means neither child needs its own hide/reveal logic when
/// the button↔tiles morph hides or reveals the parent tile — that falls out
/// of `EffectiveVisibility` automatically.
///
/// Inset from the tile's own border by `BORDER_WIDTH` on every side so the
/// overlay sits inside the border ring rather than covering it.
///
/// This is only the *starting* size/`corner_radius` — `tiles[i]` is the same
/// entity throughout the tile↔screen morph, so its geometry keeps changing
/// (tile-sized in grid view, the video screen's very different proportions
/// once settled, anything in between mid-morph). Since a child's size/
/// `corner_radius` are its own local values, not composed from the parent
/// (see `hierarchy::compose_with_parent`), `advance_tile_hover` recomputes
/// both every frame from the parent's *current* geometry so the overlay
/// keeps matching it continuously rather than staying stuck at its tile-sized
/// footprint once the tile becomes the video screen.
fn tile_overlay_quad() -> QuadState {
    QuadState {
        position: Vec3::ZERO,
        size: Vec2::new(
            TILE_WIDTH - 2.0 * BORDER_WIDTH,
            TILE_HEIGHT - 2.0 * BORDER_WIDTH,
        ),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.0, 0.0, 0.0, 0.0), // alpha animated by advance_tile_hover
        corner_radius: (TILE_CORNER_RADIUS - BORDER_WIDTH).max(0.0),
    }
}

// ---------------------------------------------------------------------------
// Video screen — MP4 playback surface (M9.5)
// ---------------------------------------------------------------------------
//
// Sized proportionally to 720p (16:9) rather than rendered at that
// resolution — the actual decode resolution comes from the source file (see
// `mp4_player::probe`) and is whatever `QuadPipeline::init_video` was called
// with.
//
// Unlike the button↔tiles morph, this isn't a group transition: clicking a
// tile morphs *that one tile* directly into the screen shape (a plain 1→1
// `TransitionRequest`, same border/geometry machinery any single entity
// uses) while the other two tiles simply fade out in place. Reversed the
// same way on click. No slicing, no baking — the tile clicked keeps its own
// identity throughout and just becomes the screen.

/// Fraction of the window width the screen occupies.
const SCREEN_WIDTH_FRACTION: f32 = 0.9;
/// 720p (1280×720) height:width ratio.
const SCREEN_ASPECT: f32 = 720.0 / 1280.0;
/// Deliberately smaller than `TILE_CORNER_RADIUS` — reusing the tile's value
/// read as "nicely rounded" on a ~200px-wide tile but nearly square at the
/// screen's ~900px width, the same absolute pixel radius being a much
/// smaller fraction of the much larger shape. See `advance_theme`'s corner
/// radius step for how this now actually eases into place (rather than
/// popping) as a tile grows into the screen.
const SCREEN_CORNER_RADIUS: f32 = 12.0;
/// Design System — Color-dark treatment corner radius, same 1.5× ratio as
/// `TILE_CORNER_RADIUS_DARK`/`TILE_CORNER_RADIUS`.
const SCREEN_CORNER_RADIUS_DARK: f32 = 18.0;

/// The video screen shape, sized to `SCREEN_WIDTH_FRACTION` of the current
/// window width at a 720p aspect ratio. Recomputed (not cached) each time a
/// tiles→screen transition starts, so a resize between visits isn't stale.
///
/// `color` is white (untinted) rather than black: once `VideoPlayer` is
/// attached, `QuadState.color` multiplies the sampled video texture (see
/// `proteus_ui::video`), so a black target would render real video frames as
/// solid black. Before the first decoded frame arrives the video texture is
/// zero-initialized (transparent), so the screen is briefly see-through
/// rather than a black card — an acceptable startup blip for this demo.
fn video_screen_quad(window_width: f32, window_height: f32) -> QuadState {
    let uncapped_height = window_width * SCREEN_WIDTH_FRACTION * SCREEN_ASPECT;
    // Never let the screen (vertically centered) overlap the top-left icon
    // row + its clearance — cap height, then re-derive width to keep the
    // 720p aspect ratio rather than stretching.
    let max_height = (window_height - 2.0 * ICON_ROW_RESERVED_PX).max(0.0);
    let height = uncapped_height.min(max_height);
    let width = height / SCREEN_ASPECT;
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(width, height),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: SCREEN_CORNER_RADIUS,
    }
}

// ---------------------------------------------------------------------------
// App state machine
// ---------------------------------------------------------------------------

/// Every resting state the demo can land in. `Splash` is the only one
/// nothing ever transitions *to* — it's the initial state and its own timer
/// carries it forward, see `advance_demo`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AppState {
    /// Mark + wordmark visible, not yet interactive.
    Splash,
    /// Three nav buttons visible — click "Video Demo" (index 0) to converge
    /// into the video tiles; the other two do nothing yet.
    Home,
    /// Three tiles visible — click any tile to converge into the video
    /// screen, or home to converge back into the nav buttons.
    VideoTiles,
    /// Video screen visible (as `tiles[screen_idx]`) — no longer interactive
    /// itself; the home/back icons drive navigation from here.
    VideoScreen(usize),
    /// Fetching 12 picsum.photos images in the background — the animated
    /// logo (light/dark theme-crossfaded) loops on `loading_logo` while
    /// waiting. Auto-advances to `Gallery` once every gallery tile has a
    /// `BakedImage`, or shows an inline error after `GALLERY_FETCH_TIMEOUT`
    /// if they don't all arrive in time — see `advance_gallery_fetch`.
    Loading,
    /// 4×3 grid of fetched images visible — click home to converge back to
    /// the nav buttons (three row-grouped `NToOneRequest`s at once).
    Gallery,
}

/// An in-flight move between two resting `AppState`s, keyed by the
/// `(from, to)` pair. `begin_transition` creates one and kicks off whatever
/// entity setup that pair needs; `drive_transition` ticks it once per frame
/// and reports completion; `settle` then unconditionally forces every
/// affected entity into `to`'s correct resting configuration — regardless of
/// which `from` it arrived from — before `self.state` actually becomes `to`.
/// This is the piece that keeps a demo replayed many times from ever leaving
/// stale geometry/visibility behind: no match arm needs to remember to clean
/// up after itself on the way out, `settle` always cleans up on the way in.
struct Transition {
    from: AppState,
    to: AppState,
    elapsed: f32,
}

// ---------------------------------------------------------------------------
// Staged pointer — accumulates OS events between frames
// ---------------------------------------------------------------------------

/// Pointer state accumulated from winit events. Flushed to the ECS
/// `PointerInput` resource at the start of each `render()` call.
#[derive(Default)]
struct StagedPointer {
    position: Option<Vec2>,
    just_pressed: bool,
    just_released: bool,
    is_pressed: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    env_logger::init();
    log::info!("Proteus reference demo — native shell");

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = ProteusApp::default();
    event_loop.run_app(&mut app).expect("event loop error");
}

// ---------------------------------------------------------------------------
// Application (winit handler)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ProteusApp {
    state: Option<RenderState>,
}

impl ApplicationHandler for ProteusApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            state.window.request_redraw();
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Proteus — Reference Demo")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32)),
                )
                .expect("failed to create window"),
        );
        let state = pollster::block_on(RenderState::new(window));
        self.state = Some(state);
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window closed — exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.resize(size);
            }
            WindowEvent::CursorMoved { position, .. } => {
                // `position` (like `surface_config.width`/`height`) is in
                // physical pixels; the world-space coordinate system is
                // logical pixels (see the comment in `RenderState::new`), so
                // both need the same scale_factor division to land in the
                // same space the render projection uses.
                let scale_factor = state.window.scale_factor() as f32;
                let w = state.surface_config.width as f32 / scale_factor;
                let h = state.surface_config.height as f32 / scale_factor;
                let wx = (position.x as f32 / scale_factor) - w / 2.0;
                let wy = h / 2.0 - (position.y as f32 / scale_factor);
                state.staged_pointer.position = Some(Vec2::new(wx, wy));
            }
            WindowEvent::CursorLeft { .. } => {
                state.staged_pointer.position = None;
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button: MouseButton::Left,
                ..
            } => match btn_state {
                ElementState::Pressed => {
                    state.staged_pointer.is_pressed = true;
                    state.staged_pointer.just_pressed = true;
                }
                ElementState::Released => {
                    state.staged_pointer.is_pressed = false;
                    state.staged_pointer.just_released = true;
                }
            },
            WindowEvent::RedrawRequested => {
                state.render();
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Render state
// ---------------------------------------------------------------------------

/// All GPU resources, ECS world, and demo state for one window.
///
/// `QuadPipeline`, `GpuContext`, and `FontAtlas` live *inside* `ui_world.world`
/// as ECS resources, not as fields here — `topology::one_to_n_setup_system` /
/// `n_to_one_setup_system` need to reach the first two to bake Slice
/// transitions automatically, and (M10.5) `bake::bake_system` needs
/// `FontAtlas` to bake `Baked` composites. `device`/`queue` stay as fields too
/// (cheap clones) for the swapchain/surface work below, which the ECS has no
/// business touching.
struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,

    ui_world: ProteusWorld,

    /// Full-window backdrop image, visible in every state (see
    /// `advance_background`).
    background: Entity,
    /// Color-dark counterpart, a child of `background` — cross-faded over it
    /// via `theme_progress` (see `advance_theme`).
    background_dark: Entity,

    button: Entity,
    /// The "PROTEUS" wordmark — a `Text` child of `button` (M10 composition),
    /// positioned to the right of the mark once baked. See its spawn site.
    wordmark: Entity,
    /// The three nav buttons ("Video Demo", "Image Gallery Demo", "Video
    /// Tests") the splash morphs into.
    nav_buttons: [Entity; 3],
    /// Per-button label — a `Text` child of `nav_buttons[i]`.
    nav_labels: [Entity; 3],
    tiles: [Entity; 3],
    /// Per-tile black hover overlay — a `Quad` child of `tiles[i]` (M10).
    tile_overlays: [Entity; 3],
    /// Per-tile title label — a `Text` child of `tiles[i]` (M10).
    tile_labels: [Entity; 3],
    /// Home/back nav icons, top-left — `nav_icons[0]` = home, `[1]` = back.
    nav_icons: [Entity; 2],
    /// "Selected" home icon art — a `Quad` child of `nav_icons[0]`, alpha
    /// cross-faded over the idle art (see `advance_nav_icons`).
    home_icon_selected: Entity,
    /// Color-dark counterparts of `nav_icons`/`home_icon_selected`, each a
    /// child of its light counterpart — cross-faded via `theme_progress`
    /// (`home_icon_dark`/`back_icon_dark`) or the split-alpha step function
    /// tied to `home_selected_fade`/`dark_target` (`home_icon_selected_dark`)
    /// — see `advance_theme`.
    home_icon_dark: Entity,
    back_icon_dark: Entity,
    home_icon_selected_dark: Entity,
    /// Persistent brand mark + "PROTEUS" wordmark, top-left of `nav_icons`.
    logo_lockup: Entity,
    /// Color-dark counterpart, a child of `logo_lockup` — cross-faded over it
    /// via `theme_progress` (see `advance_theme`).
    logo_lockup_dark: Entity,
    /// Light/dark theme toggle icons, top-right — `sun_icon` is the "inner"
    /// (closer to center) icon, `moon_icon` the "outer" one (closest to the
    /// right edge). Only whichever one does *not* match the current theme
    /// carries `Interactable` at any given moment (see `advance_theme`).
    sun_icon: Entity,
    moon_icon: Entity,
    /// Each icon's permanent dark-theme look, cross-faded in via
    /// `theme_progress` — see `advance_theme`'s doc for why sun/moon only
    /// ever need a 2-layer crossfade (their "selected" state is fully
    /// determined by which theme is current, not an independent axis, unlike
    /// `home_icon_selected`).
    sun_icon_dark: Entity,
    moon_icon_dark: Entity,

    // ── Photo gallery (M12) ─────────────────────────────────────────────────
    /// 4×3 grid, row-major (idx = row*4 + col; row 0 = top row, mapping 1:1
    /// to `nav_buttons[0..3]` for the Gallery→Home row-grouped merge).
    gallery_tiles: [Entity; 12],
    /// Visual placeholder only this pass — see `advance_gallery_button_fade`.
    gallery_fetch_button: Entity,
    gallery_fetch_button_label: Entity,
    /// Shown only if the fetch times out — see `advance_gallery_fetch`.
    gallery_error_text: Entity,
    /// Separate from Splash's `button`/`logo_frames` — this one theme-
    /// crossfades (see `loading_logo_dark`), which Splash's never had to.
    loading_logo: Entity,
    /// Color-dark counterpart, a child of `loading_logo` — cross-faded via
    /// `theme_progress` (added to `advance_theme`'s dark-overlay array).
    loading_logo_dark: Entity,
    /// Loaded synchronously at startup, same shape as `logo_frames` (its
    /// light-layer counterpart, reused directly rather than duplicated).
    loading_logo_frames_dark: Vec<(TextureId, BakedImage)>,
    loading_logo_frame_index: usize,
    loading_logo_frame_elapsed: f32,
    /// Seconds since the fetch started, while resting in `Loading`. Past
    /// `GALLERY_FETCH_TIMEOUT`, `advance_gallery_fetch` gives up and shows
    /// `gallery_error_text` instead of proceeding to `Gallery`.
    gallery_fetch_elapsed: f32,
    /// `true` once the timeout has fired this Loading visit — latched so the
    /// error stays visible rather than re-triggering every frame.
    gallery_error_shown: bool,
    gallery_tile_hover_progress: [f32; 12],
    gallery_tile_is_hovering: [bool; 12],
    /// Fade-in (after the grid morph settles) / fade-out (in sync with the
    /// Gallery→Home morph) progress for `gallery_fetch_button`.
    gallery_button_fade: f32,
    /// Draining side of `gallery_fetch::spawn`'s channel — `None` when no
    /// fetch is in flight (including right after the user bails via the
    /// error escape hatch, which drops this to abandon the background
    /// thread's remaining sends).
    gallery_fetch_rx: Option<std::sync::mpsc::Receiver<gallery_fetch::FetchResult>>,

    state: AppState,
    /// `Some` while a transition between two `AppState`s is in flight.
    transition: Option<Transition>,

    // ── demo animation state ───────────────────────────────────────────────
    intro_delay_remaining: f32,
    intro_elapsed: f32,
    splash_hold_remaining: f32,
    nav_hover_progress: [f32; 3],
    nav_is_hovering: [bool; 3],
    tile_hover_progress: [f32; 3],
    tile_is_hovering: [bool; 3],
    /// Fade-in/out progress (0..1) for `nav_icons`.
    nav_icon_fade: [f32; 2],
    nav_icon_hover_progress: [f32; 2],
    nav_icon_is_hovering: [bool; 2],
    /// Cross-fade progress (0..1) between `home_icon_selected` (1.0) and the
    /// idle home icon art beneath it (0.0).
    home_selected_fade: f32,
    /// Fade-in progress (0..1) for `logo_lockup` — shares `home_target`'s
    /// timing (see `advance_nav_icons`) but is tracked separately.
    logo_fade: f32,

    // ── Light/dark theme toggle ─────────────────────────────────────────────
    /// `true` once the user has clicked toward dark — flips instantly on
    /// click (the other icon becomes clickable right away; the morph itself
    /// is purely cosmetic catch-up, see `theme_progress`).
    dark_target: bool,
    /// 0.0 = fully light, 1.0 = fully dark. Ramped toward `dark_target`'s
    /// value every frame by `advance_theme`, which derives every themed
    /// entity's corner radius/color/image-crossfade alpha from this one
    /// scalar — the whole point of the feature.
    theme_progress: f32,
    /// Fade-in progress (0..1) for `[sun_icon, moon_icon]` — shares
    /// `home_target`'s timing, tracked separately, same as `logo_fade`.
    theme_icon_fade: [f32; 2],
    /// Hover state for whichever of `sun_icon`/`moon_icon` currently carries
    /// `Interactable` — a scalar, not a per-icon array, since only one is
    /// ever interactive at a time.
    theme_icon_hover_progress: f32,
    theme_icon_is_hovering: bool,

    // ── Animated logo (replaces the old "START" text label) ────────────────
    /// `(TextureId, BakedImage)` per frame, pre-baked into `main_atlas` once
    /// at startup, in `logo_frame_path` order. Shorter than
    /// `LOGO_FRAME_COUNT` if any frame failed to load/decode/register — the
    /// animation just cycles through whichever frames are actually present.
    logo_frames: Vec<(TextureId, BakedImage)>,
    /// Index into `logo_frames` currently shown on `button`.
    logo_frame_index: usize,
    /// Seconds accumulated since the last frame advance.
    logo_frame_elapsed: f32,

    // ── MP4 playback (M9.5) ─────────────────────────────────────────────────
    playing_video: Option<PlayingVideo>,
    present_timing: PresentTiming,

    staged_pointer: StagedPointer,
    last_frame: Instant,
}

/// Diagnostic: measures the actual wall-clock gap between successive
/// `frame.present()` calls while a video is playing, to check whether GPU
/// presentation itself is landing at even intervals — independent of
/// `mp4_player`'s decode-thread pacing (already verified separately). Only
/// accumulates while `playing_video.is_some()`; logs and resets every
/// `LOG_INTERVAL` presents.
#[derive(Default)]
struct PresentTiming {
    last_present: Option<Instant>,
    count: u32,
    sum: std::time::Duration,
    max: std::time::Duration,
}

impl PresentTiming {
    const LOG_INTERVAL: u32 = 90;

    /// Call once per `frame.present()` while a video is playing. Logs and
    /// resets its window every `LOG_INTERVAL` calls.
    fn record(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_present {
            let gap = now.duration_since(last);
            self.count += 1;
            self.sum += gap;
            self.max = self.max.max(gap);
            if self.count >= Self::LOG_INTERVAL {
                log::info!(
                    "present timing: {} frames — avg {:.2}ms, max {:.2}ms between presents",
                    self.count,
                    self.sum.as_secs_f64() * 1000.0 / self.count as f64,
                    self.max.as_secs_f64() * 1000.0,
                );
                *self = Self {
                    last_present: Some(now),
                    ..Default::default()
                };
                return;
            }
        }
        self.last_present = Some(now);
    }

    /// Call when playback stops so the next playback session starts a fresh
    /// window instead of measuring the gap across the idle period in between.
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Tracks the one video currently playing (at most one — the video screen is
/// always a single tile at a time). Torn down by `stop_video_playback` when
/// the user clicks the screen.
struct PlayingVideo {
    tile_idx: usize,
    texture_id: TextureId,
    handle: mp4_player::PlaybackHandle,
}

impl RenderState {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter found");

        log::info!("GPU adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("proteus-native"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await
            .expect("failed to create GPU device");

        let surface_caps = surface.get_capabilities(&adapter);
        // Explicitly avoid an sRGB-tagged surface format: every color in this
        // app (QuadState::color, Text::color, etc.) is authored as a flat,
        // already-gamma-space value (e.g. navy's blue channel is 0.502 —
        // plainly 128/255, a standard 8-bit color pick, not linear light).
        // The fragment shader passes these through with no linear/gamma
        // conversion of its own, so an sRGB swapchain format would make the
        // GPU apply an unwanted *second* gamma encode on top of values that
        // are already gamma-encoded — washing out colors and lightening
        // blacks. This isn't hypothetical: native was previously picking
        // `Bgra8UnormSrgb` here while the web build's WebGL2 surface offers
        // no sRGB-capable format at all and fell back to plain `Bgra8Unorm`
        // (no encode) — that mismatch was the actual cause of native's
        // duller colors relative to the browser.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096);
        // The window was requested at a *logical* size (`with_inner_size`
        // takes `LogicalSize`), but `window.inner_size()` always returns the
        // *physical* size winit actually created — e.g. a 1280×800 logical
        // window becomes a 2560×1600 physical surface at a 2x (Retina) scale
        // factor. The demo's own geometry constants (`LOGO_MARK_HEIGHT`,
        // `TILE_WIDTH`, etc.) were sized assuming 1 world unit ≈ 1 logical
        // pixel, so the projection needs to divide by `scale_factor` here —
        // otherwise everything renders at `1/scale_factor` its intended size
        // on any HiDPI display, while the surface itself stays at the full
        // physical resolution for a sharp (non-blurry) render.
        let scale_factor = window.scale_factor() as f32;
        pipeline.set_view_projection(
            &queue,
            QuadPipeline::ortho(
                size.width as f32 / scale_factor,
                size.height as f32 / scale_factor,
            ),
        );

        log::info!(
            "Render state ready — {}×{} px, format {:?}",
            size.width,
            size.height,
            surface_format,
        );

        let font_atlas = FontAtlas::with_embedded_font();

        let mut ui_world = ProteusWorld::new();
        // GpuContext + QuadPipeline + FontAtlas live in the ECS world, not as
        // RenderState fields — this is what lets transition-setup systems
        // bake Slice transitions automatically (see
        // topology::one_to_n_setup_system), and (M10.5) lets bake_system
        // reach the one shelf packer for main_atlas to bake Baked composites.
        ui_world.world.insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        ui_world.world.insert_resource(pipeline);
        ui_world.world.insert_resource(font_atlas);

        // Pre-bake all 19 logo animation frames into main_atlas, eternal —
        // they must survive the whole idle loop, not just whichever frame is
        // currently referenced (see the eviction-safety note on
        // `TextureRegistry::register_static`: an unreferenced ephemeral
        // region is fair game for LRU eviction, which would otherwise let a
        // later tile/text bake silently steal a not-currently-shown frame's
        // atlas region out from under this animation). Missing/unreadable
        // frames degrade gracefully — same pattern as `TILE_IMAGE_PATHS`.
        let mut logo_frames: Vec<(TextureId, BakedImage)> = Vec::with_capacity(LOGO_FRAME_COUNT);
        for n in 1..=LOGO_FRAME_COUNT {
            let path = logo_frame_path(n);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("logo frame {n}: could not read {path:?}: {e}");
                    continue;
                }
            };
            let decoded = match proteus_render::decode_image(&bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("logo frame {n}: could not decode {path:?}: {e}");
                    continue;
                }
            };
            let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);
            let Some(texture_id) = ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "logo frame {n}: main_atlas full — could not register {}x{}",
                    decoded.width,
                    decoded.height,
                );
                continue;
            };
            let pipeline = ui_world.world.resource::<QuadPipeline>();
            let (x, y, w, h) = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&queue, x, y, w, h, &decoded.rgba_pixels);
            let (uv_offset, uv_scale) = pipeline
                .texture_registry
                .main_atlas_uv(texture_id, MAIN_ATLAS_SIZE)
                .expect("just registered");
            logo_frames.push((
                texture_id,
                BakedImage {
                    uv_offset,
                    uv_scale,
                    pixel_size: [decoded.width as f32, decoded.height as f32],
                },
            ));
        }
        if logo_frames.is_empty() {
            log::warn!(
                "logo animation: no frames loaded from {:?} — button renders as a blank transparent quad",
                logo_frame_path(1).parent().unwrap()
            );
        }

        // Color-dark counterpart of the same 19-frame animation, used only by
        // the Loading screen's `loading_logo_dark` overlay (Splash's own
        // `button`/`logo_frames` never theme-crossfades). Same
        // load/decode/register loop as above, just `logo_frame_dark_path` and
        // its own Vec.
        let mut loading_logo_frames_dark: Vec<(TextureId, BakedImage)> =
            Vec::with_capacity(LOGO_FRAME_COUNT);
        for n in 1..=LOGO_FRAME_COUNT {
            let path = logo_frame_dark_path(n);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    log::warn!("logo frame dark {n}: could not read {path:?}: {e}");
                    continue;
                }
            };
            let decoded = match proteus_render::decode_image(&bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("logo frame dark {n}: could not decode {path:?}: {e}");
                    continue;
                }
            };
            let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);
            let Some(texture_id) = ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "logo frame dark {n}: main_atlas full — could not register {}x{}",
                    decoded.width,
                    decoded.height,
                );
                continue;
            };
            let pipeline = ui_world.world.resource::<QuadPipeline>();
            let (x, y, w, h) = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&queue, x, y, w, h, &decoded.rgba_pixels);
            let (uv_offset, uv_scale) = pipeline
                .texture_registry
                .main_atlas_uv(texture_id, MAIN_ATLAS_SIZE)
                .expect("just registered");
            loading_logo_frames_dark.push((
                texture_id,
                BakedImage {
                    uv_offset,
                    uv_scale,
                    pixel_size: [decoded.width as f32, decoded.height as f32],
                },
            ));
        }

        // Full-window backdrop (see `advance_background`) — z=0.0 so every
        // other entity draws over it. Sized properly on the very first
        // `advance_background` call; the size here just avoids a zero-size
        // flash before that runs.
        let background = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.0),
                    size: Vec2::new(
                        surface_config.width as f32 / scale_factor,
                        surface_config.height as f32 / scale_factor,
                    ),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: white(),
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
            ))
            .id();
        match std::fs::read(BG_IMAGE_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(background)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("background: could not read {BG_IMAGE_PATH:?}: {e}"),
        }
        // Color-dark counterpart — a child of `background` (zero relative
        // offset, so position/size compose for free from the parent),
        // cross-faded in via `theme_progress` alone (see `advance_theme`).
        let background_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::new(
                        surface_config.width as f32 / scale_factor,
                        surface_config.height as f32 / scale_factor,
                    ),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(background),
            ))
            .id();
        match std::fs::read(BG_IMAGE_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(background_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("background: could not read {BG_IMAGE_DARK_PATH:?}: {e}"),
        }

        let button = ui_world
            .world
            .spawn((start_button_quad(), Lifecycle::Idle, Visibility::VISIBLE))
            .id();
        // Start on frame 1 — `advance_logo_animation` cycles through the rest.
        if let Some(&(texture_id, ref baked)) = logo_frames.first() {
            ui_world
                .world
                .entity_mut(button)
                .insert((baked.clone(), TextureRef(texture_id)));
        }

        // The "PROTEUS" wordmark (M10 composition, same pattern as the old
        // "START" label) — a `Text` child of the button. Local X starts at 0;
        // `advance_intro_and_hover` moves it to sit just right of the mark
        // once `bake_pending_text` has measured the glyph run's actual width
        // (needed for pixel-accurate placement — see WORDMARK_GAP_PX's doc).
        let wordmark = ui_world
            .world
            .spawn((
                QuadState {
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    ..Default::default()
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                Text::new(WORDMARK_TEXT, WORDMARK_SIZE_PX)
                    .with_color(Vec4::new(violet().x, violet().y, violet().z, 0.0)) // fades in with the mark
                    .with_letter_spacing(WORDMARK_LETTER_SPACING_PX),
                ChildOf(button),
            ))
            .id();

        // Loading screen's animated logo — separate from `button`/
        // `logo_frames` (the Splash-only composite, which never has to
        // theme-crossfade) since this one does. Just the animated mark, no
        // wordmark — centered on the window; `advance_loading_logo_animation`
        // populates its first frame on the first tick it's needed.
        let loading_logo = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.5),
                    size: Vec2::new(LOGO_MARK_WIDTH, LOGO_MARK_HEIGHT),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::ONE,
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
            ))
            .id();
        // Color-dark counterpart — a child of `loading_logo` (zero relative
        // offset), cross-faded in via `theme_progress` alone, same shape as
        // `logo_lockup`/`logo_lockup_dark` (see `advance_theme`).
        let loading_logo_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::new(LOGO_MARK_WIDTH, LOGO_MARK_HEIGHT),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(loading_logo),
            ))
            .id();

        // Nav buttons — start hidden, revealed by the splash→nav Slice
        // transition. Placeholder geometry; `start_splash_to_nav` overwrites
        // position/size once each label's baked width is known.
        let mut nav_buttons = [Entity::PLACEHOLDER; 3];
        let mut nav_labels = [Entity::PLACEHOLDER; 3];
        for i in 0..3 {
            let btn = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.5),
                        size: NAV_BUTTON_FALLBACK_SIZE,
                        rotation: 0.0,
                        scale: 1.0,
                        anchor: Vec2::new(0.5, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        corner_radius: NAV_BUTTON_CORNER_RADIUS,
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    nav_button_border(),
                    nav_hover_glow(),
                ))
                .id();
            nav_buttons[i] = btn;
            nav_labels[i] = ui_world
                .world
                .spawn((
                    QuadState {
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        ..Default::default()
                    },
                    Lifecycle::Idle,
                    Visibility::VISIBLE,
                    Text::new(NAV_BUTTON_TITLES[i], NAV_BUTTON_LABEL_SIZE_PX)
                        .with_color(violet())
                        .with_letter_spacing(NAV_BUTTON_LETTER_SPACING_PX),
                    ChildOf(btn),
                ))
                .id();
        }

        // Tiles start hidden; box-cover art (below) makes a text label
        // redundant, so tiles carry no Text component.
        let tiles = [
            ui_world
                .world
                .spawn((
                    tile_quad(0, 0.0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    tile_quad(1, 0.0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    tile_quad(2, 0.0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id(),
        ];

        // Box-cover art (M9.7) — attached after spawn (rather than in the
        // tuple above) so a missing/unreadable file just leaves the tile on
        // its solid `TILE_COLORS` fill instead of failing to spawn at all.
        for (idx, &tile) in tiles.iter().enumerate() {
            match std::fs::read(TILE_IMAGE_PATHS[idx]) {
                Ok(bytes) => {
                    ui_world.world.entity_mut(tile).insert(Image::new(bytes));
                    // Untinted — the real box art replaces the placeholder
                    // TILE_COLORS fill, so it shouldn't be tinted by it.
                    if let Some(mut qs) = ui_world.world.get_mut::<QuadState>(tile) {
                        qs.color = white();
                    }
                }
                Err(e) => {
                    log::warn!(
                        "tile {idx}: could not read box-cover image {:?}: {e}",
                        TILE_IMAGE_PATHS[idx]
                    );
                }
            }
        }

        // Hover overlay + title label (M10) — two children per tile, spawned
        // after (so they draw on top of) the tile's own box-art background.
        // Order: overlay first, label second, matching "layered above the
        // overlay" — draw order follows insertion order (see collect.rs).
        let mut tile_overlays = [Entity::PLACEHOLDER; 3];
        let mut tile_labels = [Entity::PLACEHOLDER; 3];
        for (idx, &tile) in tiles.iter().enumerate() {
            tile_overlays[idx] = ui_world
                .world
                .spawn((
                    tile_overlay_quad(),
                    Lifecycle::Idle,
                    Visibility::VISIBLE,
                    ChildOf(tile),
                ))
                .id();
            tile_labels[idx] = ui_world
                .world
                .spawn((
                    QuadState {
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        ..Default::default()
                    },
                    Lifecycle::Idle,
                    Visibility::VISIBLE,
                    Text::new(TILE_TITLES[idx], TILE_LABEL_SIZE_PX)
                        .with_color(Vec4::new(
                            violet_dark().x,
                            violet_dark().y,
                            violet_dark().z,
                            0.0, // alpha animated by advance_tile_hover
                        ))
                        .with_letter_spacing(TILE_LABEL_LETTER_SPACING_PX),
                    ChildOf(tile),
                ))
                .id();
        }

        // Photo gallery grid — "tile w/o label" recipe (border + hover glow,
        // no overlay-tint/title-label children, unlike the video tiles
        // above). Placeholder geometry; `layout_gallery_tiles` overwrites it
        // once the window size is known/on each Home→Loading entry. Images
        // are attached later, as each picsum fetch completes (native:
        // `drain_gallery_fetch`; web: `set_gallery_image`).
        let gallery_tiles: [Entity; 12] = std::array::from_fn(|i| {
            ui_world
                .world
                .spawn((
                    gallery_cell_quad(i, 1280.0, 800.0, 0.0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id()
        });

        // "Fetch New Images" — same visual shape as a nav button; a visual
        // placeholder only this pass, never wired into advance_demo's click
        // handling (see advance_gallery_button_fade for its fade in/out).
        let gallery_fetch_button = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.5),
                    size: Vec2::new(220.0, 46.0),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_BUTTON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
                Interactable,
                nav_button_border(),
                nav_hover_glow(),
            ))
            .id();
        let gallery_fetch_button_label = ui_world
            .world
            .spawn((
                QuadState {
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    ..Default::default()
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                Text::new(GALLERY_FETCH_BUTTON_LABEL, NAV_BUTTON_LABEL_SIZE_PX)
                    .with_color(violet())
                    .with_letter_spacing(NAV_BUTTON_LETTER_SPACING_PX),
                ChildOf(gallery_fetch_button),
            ))
            .id();

        // Inline error text shown only if the fetch times out (see
        // advance_gallery_fetch) — positioned just below the looping logo.
        let gallery_error_text = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, -LOGO_MARK_HEIGHT / 2.0 - 40.0, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    ..Default::default()
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
                Text::new(LOADING_LOGO_ERROR_TEXT, 18.0).with_color(white()),
            ))
            .id();

        // Home/back nav icons — top-left, hidden until tiles/screen show.
        // Border+glyph are baked into the PNGs themselves (Color-light
        // treatment), so no separate Border component is needed here.
        let nav_icons = [HOME_ICON_PATH, BACK_ICON_PATH].map(|path| {
            let icon = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.9), // above tiles/screen (z=0.5)
                        size: Vec2::splat(NAV_ICON_SIZE_PX),
                        rotation: 0.0,
                        scale: 1.0,
                        anchor: Vec2::new(0.5, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        corner_radius: NAV_ICON_CORNER_RADIUS,
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    nav_hover_glow(),
                ))
                .id();
            match std::fs::read(path) {
                Ok(bytes) => {
                    ui_world.world.entity_mut(icon).insert(Image::new(bytes));
                }
                Err(e) => log::warn!("nav icon: could not read {path:?}: {e}"),
            }
            icon
        });

        // Color-dark counterparts of nav_icons — children layered on top,
        // cross-faded in via `theme_progress` alone (see `advance_theme`).
        let home_icon_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(NAV_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(nav_icons[0]),
            ))
            .id();
        match std::fs::read(HOME_ICON_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(home_icon_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("home icon dark: could not read {HOME_ICON_DARK_PATH:?}: {e}"),
        }
        let back_icon_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(NAV_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(nav_icons[1]),
            ))
            .id();
        match std::fs::read(BACK_ICON_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(back_icon_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("back icon dark: could not read {BACK_ICON_DARK_PATH:?}: {e}"),
        }

        // "Selected" home icon art — a child of nav_icons[0], layered on top
        // and cross-faded in via its own alpha (see `advance_nav_icons`)
        // rather than swapping the base entity's texture: both PNGs already
        // bake in their own opaque fill, so a plain alpha blend between the
        // two stacked quads reads as a real crossfade.
        let home_icon_selected = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(NAV_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(nav_icons[0]),
            ))
            .id();
        match std::fs::read(HOME_ICON_SELECTED_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(home_icon_selected)
                    .insert(Image::new(bytes));
            }
            Err(e) => {
                log::warn!("home icon selected: could not read {HOME_ICON_SELECTED_PATH:?}: {e}")
            }
        }
        // Color-dark counterpart of `home_icon_selected` — same shape, but
        // driven by a split-alpha step function (not a continuous crossfade)
        // tied to `dark_target`, since it shares the `home_selected_fade`
        // envelope with its light sibling rather than fading independently.
        // See `advance_theme`'s doc for why this is a deliberate
        // simplification rather than true bilinear (page-selected × theme)
        // compositing.
        let home_icon_selected_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(NAV_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(nav_icons[0]),
            ))
            .id();
        match std::fs::read(HOME_ICON_SELECTED_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(home_icon_selected_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!(
                "home icon selected dark: could not read {HOME_ICON_SELECTED_DARK_PATH:?}: {e}"
            ),
        }

        // Light/dark theme toggle — sun/moon, top-right. `sun_icon` holds
        // sun's *dark*-theme ring art and `sun_icon_dark` holds its *light*-
        // theme solid-disc art — an inversion of what the names suggest, see
        // SUN_ICON_PATH's doc for why. Moon's roles match its name
        // (`moon_icon` = light art, `moon_icon_dark` = dark art) as usual.
        // Only moon starts `Interactable`; `advance_theme` moves it to
        // whichever icon doesn't match the current theme.
        let sun_icon = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.9),
                    size: Vec2::splat(THEME_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
                nav_hover_glow(),
            ))
            .id();
        match std::fs::read(SUN_ICON_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(sun_icon)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("sun icon: could not read {SUN_ICON_PATH:?}: {e}"),
        }
        let sun_icon_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(THEME_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(sun_icon),
            ))
            .id();
        match std::fs::read(SUN_ICON_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(sun_icon_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("sun icon dark: could not read {SUN_ICON_DARK_PATH:?}: {e}"),
        }

        let moon_icon = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.9),
                    size: Vec2::splat(THEME_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
                Interactable,
                nav_hover_glow(),
            ))
            .id();
        match std::fs::read(MOON_ICON_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(moon_icon)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("moon icon: could not read {MOON_ICON_PATH:?}: {e}"),
        }
        let moon_icon_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::splat(THEME_ICON_SIZE_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_ICON_CORNER_RADIUS,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(moon_icon),
            ))
            .id();
        match std::fs::read(MOON_ICON_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(moon_icon_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("moon icon dark: could not read {MOON_ICON_DARK_PATH:?}: {e}"),
        }

        // Persistent brand lockup (mark + "PROTEUS" wordmark), top-left —
        // hidden until the splash finishes morphing into Home, then stays up
        // through every other state (see `settle`).
        let logo_lockup = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::new(0.0, 0.0, 0.9),
                    size: Vec2::new(LOGO_WIDTH_PX, LOGO_HEIGHT_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0), // alpha animated by advance_nav_icons
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::HIDDEN,
            ))
            .id();
        match std::fs::read(LOGO_LOCKUP_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(logo_lockup)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("logo lockup: could not read {LOGO_LOCKUP_PATH:?}: {e}"),
        }
        // Color-dark counterpart — a child of `logo_lockup` (zero relative
        // offset), cross-faded in via `theme_progress` alone.
        let logo_lockup_dark = ui_world
            .world
            .spawn((
                QuadState {
                    position: Vec3::ZERO,
                    size: Vec2::new(LOGO_WIDTH_PX, LOGO_HEIGHT_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Visibility::VISIBLE,
                ChildOf(logo_lockup),
            ))
            .id();
        match std::fs::read(LOGO_LOCKUP_DARK_PATH) {
            Ok(bytes) => {
                ui_world
                    .world
                    .entity_mut(logo_lockup_dark)
                    .insert(Image::new(bytes));
            }
            Err(e) => log::warn!("logo lockup dark: could not read {LOGO_LOCKUP_DARK_PATH:?}: {e}"),
        }

        log::info!(
            "Demo entities — button {:?} ({} logo frames loaded), wordmark {:?}, nav_buttons {:?}, tiles {:?}, tile_overlays {:?}, tile_labels {:?}, nav_icons {:?}, logo_lockup {:?}",
            button,
            logo_frames.len(),
            wordmark,
            nav_buttons,
            tiles,
            tile_overlays,
            tile_labels,
            nav_icons,
            logo_lockup
        );

        Self {
            window,
            surface,
            surface_config,
            device,
            queue,
            ui_world,
            background,
            background_dark,
            button,
            wordmark,
            nav_buttons,
            nav_labels,
            tiles,
            tile_overlays,
            tile_labels,
            nav_icons,
            home_icon_selected,
            home_icon_dark,
            back_icon_dark,
            home_icon_selected_dark,
            logo_lockup,
            logo_lockup_dark,
            sun_icon,
            moon_icon,
            sun_icon_dark,
            moon_icon_dark,
            gallery_tiles,
            gallery_fetch_button,
            gallery_fetch_button_label,
            gallery_error_text,
            loading_logo,
            loading_logo_dark,
            loading_logo_frames_dark,
            loading_logo_frame_index: 0,
            loading_logo_frame_elapsed: 0.0,
            gallery_fetch_elapsed: 0.0,
            gallery_error_shown: false,
            gallery_tile_hover_progress: [0.0; 12],
            gallery_tile_is_hovering: [false; 12],
            gallery_button_fade: 0.0,
            gallery_fetch_rx: None,
            state: AppState::Splash,
            transition: None,
            intro_delay_remaining: INTRO_DELAY,
            intro_elapsed: 0.0,
            splash_hold_remaining: SPLASH_HOLD_DURATION,
            nav_hover_progress: [0.0; 3],
            nav_is_hovering: [false; 3],
            tile_hover_progress: [0.0; 3],
            tile_is_hovering: [false; 3],
            nav_icon_fade: [0.0; 2],
            nav_icon_hover_progress: [0.0; 2],
            nav_icon_is_hovering: [false; 2],
            home_selected_fade: 0.0,
            logo_fade: 0.0,
            dark_target: false,
            theme_progress: 0.0,
            theme_icon_fade: [0.0; 2],
            theme_icon_hover_progress: 0.0,
            theme_icon_is_hovering: false,
            logo_frames,
            logo_frame_index: 0,
            logo_frame_elapsed: 0.0,
            playing_video: None,
            present_timing: PresentTiming::default(),
            staged_pointer: StagedPointer::default(),
            last_frame: Instant::now(),
        }
    }

    // -------------------------------------------------------------------------
    // Resize
    // -------------------------------------------------------------------------

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        // See the matching comment in `RenderState::new` — `size` is
        // physical pixels; the projection needs logical ones.
        let scale_factor = self.window.scale_factor() as f32;
        self.ui_world
            .world
            .resource::<QuadPipeline>()
            .set_view_projection(
                &self.queue,
                QuadPipeline::ortho(
                    size.width as f32 / scale_factor,
                    size.height as f32 / scale_factor,
                ),
            );
    }

    // -------------------------------------------------------------------------
    // Text bake pass
    // -------------------------------------------------------------------------

    /// For every entity with `Text` but no `BakedText`: rasterise → upload to
    /// main_atlas → insert `BakedText`.
    fn bake_pending_text(&mut self) {
        let all_text: Vec<(Entity, String, f32, f32)> = {
            let mut q = self.ui_world.world.query::<(Entity, &Text)>();
            q.iter(&self.ui_world.world)
                .map(|(e, t)| (e, t.content.clone(), t.size_px, t.letter_spacing_px))
                .collect()
        };

        let pending: Vec<(Entity, String, f32, f32)> = all_text
            .into_iter()
            .filter(|(e, _, _, _)| self.ui_world.world.get::<BakedText>(*e).is_none())
            .collect();

        for (entity, content, size_px, letter_spacing_px) in pending {
            // FontAtlas is a real ECS resource (M10.5), reached here via
            // resource_scope — bevy's pattern for needing a specific resource
            // and general World access (for the entity_mut insert below)
            // without a borrow conflict.
            let glyphs =
                self.ui_world
                    .world
                    .resource_scope::<FontAtlas, _>(|_world, mut font_atlas| {
                        font_atlas.rasterize_text_tracked(&content, size_px, letter_spacing_px)
                    });
            let Some(glyphs) = glyphs else {
                log::warn!("FontAtlas: could not rasterize '{content}' for entity {entity:?}");
                continue;
            };

            // M11: allocation moved from FontAtlas's old shelf packer to the
            // real TextureRegistry — register the region, then upload.
            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(glyphs.width, glyphs.height, false)
            else {
                log::warn!(
                    "bake_pending_text: main_atlas full — could not register {}x{} for entity {entity:?}",
                    glyphs.width,
                    glyphs.height,
                );
                continue;
            };

            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let (x, y, w, h) = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, x, y, w, h, &glyphs.rgba_pixels);
            let (uv_offset, uv_scale) = pipeline
                .texture_registry
                .main_atlas_uv(texture_id, MAIN_ATLAS_SIZE)
                .expect("just registered");

            self.ui_world.world.entity_mut(entity).insert((
                BakedText {
                    uv_offset,
                    uv_scale,
                    pixel_size: [glyphs.width as f32, glyphs.height as f32],
                },
                TextureRef(texture_id),
            ));
        }
    }

    // -------------------------------------------------------------------------
    // Image bake pass (M9.7)
    // -------------------------------------------------------------------------

    /// For every entity with `Image` but no `BakedImage`: decode → upload to
    /// main_atlas (via the same shelf packer `bake_pending_text` uses — see
    /// `FontAtlas::bake_image`) → insert `BakedImage`. Mirrors
    /// `bake_pending_text` exactly, one atlas-baking pass per component kind.
    fn bake_pending_images(&mut self) {
        let all_images: Vec<(Entity, std::sync::Arc<[u8]>)> = {
            let mut q = self.ui_world.world.query::<(Entity, &Image)>();
            q.iter(&self.ui_world.world)
                .map(|(e, img)| (e, img.bytes.clone()))
                .collect()
        };

        let pending: Vec<(Entity, std::sync::Arc<[u8]>)> = all_images
            .into_iter()
            .filter(|(e, _)| self.ui_world.world.get::<BakedImage>(*e).is_none())
            .collect();

        for (entity, bytes) in pending {
            let decoded = match proteus_render::decode_image(&bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("bake_pending_images: entity {entity:?}: {e}");
                    continue;
                }
            };
            // Real photos routinely arrive far larger than main_atlas
            // (2048×2048, shared with baked text) can sensibly hold — cap
            // to a size comfortably above the tiles' on-screen footprint.
            let decoded = proteus_render::resize_to_fit(decoded, MAX_TILE_IMAGE_SIDE);

            // M11: decode_image already produced pixels — no FontAtlas
            // involvement needed. Register the region, then upload.
            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, false)
            else {
                log::warn!(
                    "bake_pending_images: main_atlas full — could not register {}x{} image for entity {entity:?}",
                    decoded.width,
                    decoded.height,
                );
                continue;
            };

            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let (x, y, w, h) = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, x, y, w, h, &decoded.rgba_pixels);
            let (uv_offset, uv_scale) = pipeline
                .texture_registry
                .main_atlas_uv(texture_id, MAIN_ATLAS_SIZE)
                .expect("just registered");

            self.ui_world.world.entity_mut(entity).insert((
                BakedImage {
                    uv_offset,
                    uv_scale,
                    pixel_size: [decoded.width as f32, decoded.height as f32],
                },
                TextureRef(texture_id),
            ));
        }
    }

    // -------------------------------------------------------------------------
    // Background
    // -------------------------------------------------------------------------

    /// Keeps the full-window backdrop sized to the current window every
    /// frame, so a resize doesn't leave it under/oversized — no animation,
    /// state, or hover involved, just a permanent, unconditional fill.
    fn advance_background(&mut self) {
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.background) {
            qs.size = Vec2::new(window_width, window_height);
        }
        // `background_dark` is a child with zero relative offset, but size
        // isn't inherited from the parent automatically — keep it in sync
        // too, same as the parent above.
        if let Some(mut qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.background_dark)
        {
            qs.size = Vec2::new(window_width, window_height);
        }
    }

    // -------------------------------------------------------------------------
    // Intro fade + hover glow
    // -------------------------------------------------------------------------

    /// Advances the one-shot entry fade and the hover glow sweep, then writes
    /// the results directly onto the button's `QuadState`/`Glow` components.
    ///
    /// Neither animation goes through `TransitionRequest` — they aren't
    /// morphs between two declared forms, just continuous alpha/radius sweeps
    /// driven by elapsed time and hover state, so it's simpler to drive them
    /// directly here than to route them through the transition system.
    fn advance_intro_and_hover(&mut self, dt: f32) {
        // --- Intro fade (waits INTRO_DELAY, then plays once, 0 → 1, never reverses) ---
        // Burn off the delay first; any leftover dt in the same tick carries
        // into the fade itself rather than being dropped (same pattern as
        // ActiveTransition's delay handling in transition.rs).
        let fade_dt = if self.intro_delay_remaining > 0.0 {
            let burned = dt.min(self.intro_delay_remaining);
            self.intro_delay_remaining -= burned;
            dt - burned
        } else {
            dt
        };
        self.intro_elapsed = (self.intro_elapsed + fade_dt).min(INTRO_DURATION);
        let raw_t = self.intro_elapsed / INTRO_DURATION;
        let alpha = ease_out_quad(raw_t);

        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.button) {
            qs.color.w = alpha;
        }
        if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.wordmark) {
            text.color.w = alpha;
        }
        // Mark + wordmark read as one composite, not "a mark with a label" —
        // centering just the mark (as before) would leave the wordmark
        // trailing off-center to the right. Once the wordmark's actual baked
        // width is known (see the doc on WORDMARK_GAP_PX's neighbor consts),
        // shift the mark left by half of the width the wordmark *adds*, so
        // the whole assembly is centered — then slide the whole thing in
        // from the left in lockstep with the same eased fade curve `alpha`
        // already drives, so the two effects read as one motion rather than
        // a fade and a slide happening to overlap.
        //
        // This offset is computed in the same "unscaled" units as
        // WORDMARK_GAP_PX/LOGO_MARK_WIDTH — `hierarchy::compose_with_parent`
        // scales a child's local position by the parent's own composed
        // scale, so `button`'s own COMPOSITE_SCALE factor is applied once,
        // here, rather than needing to be baked into this formula too.
        if let Some(baked) = self.ui_world.world.get::<BakedText>(self.wordmark) {
            let wordmark_width = baked.pixel_size[0];
            let rest_x = -COMPOSITE_SCALE * (WORDMARK_GAP_PX + wordmark_width) / 2.0;
            let slide_offset = INTRO_SLIDE_DISTANCE_PX * (1.0 - alpha);
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.button) {
                qs.position.x = rest_x - slide_offset;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.wordmark) {
                qs.position.x = LOGO_MARK_WIDTH / 2.0 + WORDMARK_GAP_PX + wordmark_width / 2.0;
            }
        }
    }

    /// Advances the logo's frame-sweep animation while the button is idle
    /// (waiting for a click) — swaps which pre-baked frame's `BakedImage`/
    /// `TextureRef` sits on `button`, wrapping through `logo_frames` every
    /// `LOGO_FRAME_DURATION` seconds. Stops advancing once `ButtonToTiles`
    /// begins: the button is either mid-morph (its current frame gets baked
    /// into the Slice transition's snapshot, same as any other `BakedImage`
    /// content) or already hidden, so there's nothing left to animate.
    fn advance_logo_animation(&mut self, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Splash {
            return;
        }
        self.logo_frame_elapsed += dt;
        while self.logo_frame_elapsed >= LOGO_FRAME_DURATION {
            self.logo_frame_elapsed -= LOGO_FRAME_DURATION;
            self.logo_frame_index = (self.logo_frame_index + 1) % self.logo_frames.len();
            let (texture_id, baked) = self.logo_frames[self.logo_frame_index].clone();
            self.ui_world
                .world
                .entity_mut(self.button)
                .insert((baked, TextureRef(texture_id)));
        }
    }

    /// Same hover glow sweep as the button, applied to each of the three
    /// tiles. Tiles are always fully opaque once visible (they arrive via
    /// the button-spread morph, not a fade), so unlike
    /// `advance_intro_and_hover` there's no alpha to suppress the glow with.
    ///
    /// M10: also drives the hover overlay + title label children's alpha off
    /// the same `tile_hover_progress` sweep — same duration as the glow, just
    /// a different destination component. Neither child needs its own
    /// hide/reveal logic when the parent tile itself is hidden/revealed by
    /// the button↔tiles morph — cascading `EffectiveVisibility` handles that.
    ///
    /// While the tile↔screen morph is in flight (`TilesToScreen`/
    /// `ScreenToTiles`), the hover effect targets 0 regardless of the
    /// tracked hover state — hover shouldn't compete visually with an
    /// in-flight geometry morph. This forces the *target*, not
    /// `tile_is_hovering` itself: `tile_is_hovering` keeps tracking the
    /// pointer's real state (updated below from `hover_entered`/
    /// `hover_exited`), so once the morph settles back into `TilesIdle`/
    /// `ScreenIdle`, hover immediately reflects wherever the pointer actually
    /// is — including "still hovering, ramp back up" if it never left.
    /// Forcing `tile_is_hovering` itself instead would desync from
    /// `hit_test_system`'s own edge-triggered hover tracking: if the pointer
    /// never moves off the tile during the whole morph, no new
    /// `hover_entered` event would ever fire afterward to correct it, leaving
    /// hover stuck off until the pointer jiggles.
    fn advance_tile_hover(&mut self, dt: f32) {
        let transition_pair = self.transition.as_ref().map(|t| (t.from, t.to));
        let transitioning = matches!(
            transition_pair,
            Some((AppState::VideoTiles, AppState::VideoScreen(_)))
                | Some((AppState::VideoScreen(_), AppState::VideoTiles))
        );
        // The tile shape-morph's endpoint index, whichever side of the
        // transition it's on — `TilesToScreen`'s `to`, `ScreenToTiles`'s
        // `from`, or (once settled) `self.state` itself. Used below both to
        // suppress hover on the screen tile and to size its title label.
        let screen_focus_idx = match transition_pair {
            Some((AppState::VideoScreen(idx), _)) | Some((_, AppState::VideoScreen(idx))) => {
                Some(idx)
            }
            None => match self.state {
                AppState::VideoScreen(idx) => Some(idx),
                _ => None,
            },
            _ => None,
        };
        for i in 0..3 {
            let entity = self.tiles[i];
            // The video screen is static while playing — no hover glow/scale
            // reaction, even though `Interactable` stays on the entity
            // (harmless: `advance_demo`'s `VideoScreen` arm never checks for
            // clicks on `tiles[screen_idx]` itself, only the nav icons).
            let is_idle_screen = self.transition.is_none() && screen_focus_idx == Some(i);
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&entity) {
                    self.tile_is_hovering[i] = true;
                } else if events.hover_exited.contains(&entity) {
                    self.tile_is_hovering[i] = false;
                }
            }
            let target = if transitioning || is_idle_screen {
                0.0
            } else if self.tile_is_hovering[i] {
                1.0
            } else {
                0.0
            };
            let step = dt / GLOW_DURATION;
            if self.tile_hover_progress[i] < target {
                self.tile_hover_progress[i] = (self.tile_hover_progress[i] + step).min(target);
            } else if self.tile_hover_progress[i] > target {
                self.tile_hover_progress[i] = (self.tile_hover_progress[i] - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.tile_hover_progress[i] * GLOW_MAX_RADIUS;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                qs.scale = 1.0 + self.tile_hover_progress[i] * HOVER_SCALE_BOOST;
            }
            // M10: `tiles[i]` is the same entity throughout the tile↔screen
            // morph (see the module doc — clicking a tile morphs *that one
            // tile* directly into the screen shape), so its `QuadState.size`/
            // `corner_radius` are whatever the in-flight `TransitionRequest`
            // has lerped them to right now — tile-sized in grid view, the
            // screen's 720p-ish proportions once settled as the video player,
            // and anything in between mid-morph. The overlay/label are
            // separate child entities with their own fixed local size, so
            // without this they'd stay stuck at their tile-sized footprint
            // and look wrong pasted onto the much larger/differently-shaped
            // video screen. Recomputed fresh every frame (not just at spawn)
            // so they track the parent continuously, matching it exactly at
            // every point of the morph rather than snapping at the end.
            let tile_geometry = self
                .ui_world
                .world
                .get::<QuadState>(entity)
                .map(|qs| (qs.size, qs.corner_radius));
            if let Some(mut overlay_qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.tile_overlays[i])
            {
                if let Some((tile_size, tile_corner_radius)) = tile_geometry {
                    overlay_qs.size = (tile_size - Vec2::splat(2.0 * BORDER_WIDTH)).max(Vec2::ZERO);
                    overlay_qs.corner_radius = (tile_corner_radius - BORDER_WIDTH).max(0.0);
                }
                overlay_qs.color.w = self.tile_hover_progress[i] * TILE_OVERLAY_MAX_ALPHA;
            }
            if let Some(mut label) = self.ui_world.world.get_mut::<Text>(self.tile_labels[i]) {
                label.color.w = self.tile_hover_progress[i];
            }
            // Larger title on the video screen than in grid view. The baked
            // glyph run itself is a fixed size, but `scale` composes down the
            // hierarchy multiplicatively, so bumping the label child's own
            // local scale renders the same glyphs visibly bigger. Tied to
            // `screen_focus_idx` rather than tile geometry directly: the
            // label's alpha is already forced to zero for this tile during
            // the in-flight morph — hard-zeroed by `advance_tiles_to_screen_fade`
            // going forward, already zero (no hover on the static screen) and
            // held there by `transitioning`'s target-0 ramp coming back — so
            // there's nothing to interpolate here, it only needs the right
            // value once settled into `VideoTiles` or `VideoScreen`.
            let label_scale = if screen_focus_idx == Some(i) {
                TILE_LABEL_SCREEN_SCALE
            } else {
                1.0
            };
            if let Some(mut label_qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.tile_labels[i])
            {
                label_qs.scale = label_scale;
            }
        }
    }

    /// Design-System hover: 15px glow + 5% scale, no fill/text-color change.
    /// Labels stay fully opaque throughout — unlike tile labels, they're the
    /// button's primary content, not a hover reveal.
    fn advance_nav_hover(&mut self, dt: f32) {
        let transitioning = matches!(
            self.transition.as_ref().map(|t| (t.from, t.to)),
            Some((AppState::Home, AppState::VideoTiles))
        );
        for i in 0..3 {
            let entity = self.nav_buttons[i];
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&entity) {
                    self.nav_is_hovering[i] = true;
                } else if events.hover_exited.contains(&entity) {
                    self.nav_is_hovering[i] = false;
                }
            }
            let target = if transitioning {
                0.0
            } else if self.nav_is_hovering[i] {
                1.0
            } else {
                0.0
            };
            let step = dt / GLOW_DURATION;
            if self.nav_hover_progress[i] < target {
                self.nav_hover_progress[i] = (self.nav_hover_progress[i] + step).min(target);
            } else if self.nav_hover_progress[i] > target {
                self.nav_hover_progress[i] = (self.nav_hover_progress[i] - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.nav_hover_progress[i] * GLOW_MAX_RADIUS;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                qs.scale = 1.0 + self.nav_hover_progress[i] * HOVER_SCALE_BOOST;
            }
        }
    }

    /// Fades home/back icons and the logo lockup in/out based on phase,
    /// positions the logo lockup and both icons top-left (recomputed every
    /// frame so window resizes track correctly), drives the icons'
    /// Design-System hover (a 15px glow plus a 5% scale bump), and
    /// cross-fades the home icon's "selected" art in/out of its idle art
    /// depending on whether `Home` is the current or target state.
    fn advance_nav_icons(&mut self, dt: f32) {
        // The home icon is now up on every non-splash state (not just once
        // away from Home, like "back") — hidden only during `Splash` itself
        // and the splash→home morph, so it's already in place the instant
        // Home first appears rather than popping in a beat later.
        let home_target: f32 = match (&self.transition, self.state) {
            (Some(t), _) if t.from == AppState::Splash => 0.0,
            (None, AppState::Splash) => 0.0,
            _ => 1.0,
        };
        // "back" only ever fades in once idle on the video screen — clicking
        // home from there skips straight to a (VideoScreen, Home) transition
        // (see `start_screen_to_nav`), which falls into the `_` arm below
        // without needing a `then_home` flag.
        let back_target: f32 = match (&self.transition, self.state) {
            (None, AppState::VideoScreen(_)) => 1.0,
            _ => 0.0,
        };
        let targets = [home_target, back_target];
        // The home icon shows no hover reaction while already resting on
        // `Home` — clicking it there would be a no-op, so there's nothing to
        // invite hover feedback for (mirrors `advance_tile_hover`'s
        // `is_idle_screen` suppression for the same reason).
        let home_icon_hover_suppressed =
            self.transition.is_none() && matches!(self.state, AppState::Home);

        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let logo_left_edge = -window_width / 2.0 + NAV_ICON_MARGIN_PX;
        let base_x =
            logo_left_edge + LOGO_TEXT_RIGHT_PX + LOGO_ICONS_GAP_PX + NAV_ICON_SIZE_PX / 2.0;
        let y = window_height / 2.0 - NAV_ICON_MARGIN_PX - NAV_ICON_SIZE_PX / 2.0;
        let xs = [base_x, base_x + NAV_ICON_SIZE_PX + NAV_ICON_GAP_PX];

        // The logo fades in alongside the home icon (same `home_target`
        // signal, same duration) the first time either appears, right after
        // the splash morph — and then, since that target never returns to
        // 0, stays up through every other state.
        if home_target > 0.0 {
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(self.logo_lockup) {
                vis.visible = true;
            }
        }
        let logo_step = dt / NAV_ICON_FADE_DURATION;
        if self.logo_fade < home_target {
            self.logo_fade = (self.logo_fade + logo_step).min(home_target);
        } else if self.logo_fade > home_target {
            self.logo_fade = (self.logo_fade - logo_step).max(home_target);
        }
        if let Some(mut logo_qs) = self.ui_world.world.get_mut::<QuadState>(self.logo_lockup) {
            logo_qs.position.x = logo_left_edge + LOGO_WIDTH_PX / 2.0;
            logo_qs.position.y = y;
            logo_qs.color.w = self.logo_fade;
        }

        // Home icon "selected" while Home is the current resting state *or*
        // the transition's destination — flipping the instant a transition
        // begins (not once it lands) is what makes this a real crossfade
        // rather than an instant swap at the very end of the button/tile
        // morph, which runs on its own much longer timeline.
        let home_selected_target: f32 = match (&self.transition, self.state) {
            (Some(t), _) => {
                if t.to == AppState::Home {
                    1.0
                } else {
                    0.0
                }
            }
            (None, AppState::Home) => 1.0,
            (None, _) => 0.0,
        };
        let home_selected_step = dt / NAV_ICON_FADE_DURATION;
        if self.home_selected_fade < home_selected_target {
            self.home_selected_fade =
                (self.home_selected_fade + home_selected_step).min(home_selected_target);
        } else if self.home_selected_fade > home_selected_target {
            self.home_selected_fade =
                (self.home_selected_fade - home_selected_step).max(home_selected_target);
        }
        if let Some(mut qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.home_icon_selected)
        {
            qs.color.w = self.home_selected_fade;
        }

        for i in 0..2 {
            let icon = self.nav_icons[i];
            let target = targets[i];
            if target > 0.0 {
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(icon) {
                    vis.visible = true;
                }
            }
            let step = dt / NAV_ICON_FADE_DURATION;
            if self.nav_icon_fade[i] < target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] + step).min(target);
            } else if self.nav_icon_fade[i] > target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] - step).max(target);
            }
            if target <= 0.0 && self.nav_icon_fade[i] <= 0.0 {
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(icon) {
                    vis.visible = false;
                }
            }

            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&icon) {
                    self.nav_icon_is_hovering[i] = true;
                } else if events.hover_exited.contains(&icon) {
                    self.nav_icon_is_hovering[i] = false;
                }
            }
            let suppress_hover = i == 0 && home_icon_hover_suppressed;
            let hover_target = if target > 0.0 && self.nav_icon_is_hovering[i] && !suppress_hover {
                1.0
            } else {
                0.0
            };
            let hover_step = dt / GLOW_DURATION;
            if self.nav_icon_hover_progress[i] < hover_target {
                self.nav_icon_hover_progress[i] =
                    (self.nav_icon_hover_progress[i] + hover_step).min(hover_target);
            } else if self.nav_icon_hover_progress[i] > hover_target {
                self.nav_icon_hover_progress[i] =
                    (self.nav_icon_hover_progress[i] - hover_step).max(hover_target);
            }

            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(icon) {
                qs.position.x = xs[i];
                qs.position.y = y;
                qs.color.w = self.nav_icon_fade[i];
                qs.scale = 1.0 + self.nav_icon_hover_progress[i] * HOVER_SCALE_BOOST;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(icon) {
                glow.radius = self.nav_icon_hover_progress[i] * GLOW_MAX_RADIUS;
                glow.color.w = self.nav_icon_fade[i];
            }
        }
    }

    // -------------------------------------------------------------------------
    // Light/dark theme toggle
    // -------------------------------------------------------------------------

    /// Advances the light/dark theme morph: reads sun/moon clicks, ramps
    /// `theme_progress` toward `dark_target`, and writes the resulting
    /// corner-radius/color/image-crossfade values onto every themed entity —
    /// unconditionally, every frame, regardless of `AppState` or visibility.
    /// That unconditional-every-frame property is what makes "a component
    /// already reflects the current theme by the time it becomes visible"
    /// true for free, with no visibility branching needed here at all.
    ///
    /// Runs after `advance_demo`/`advance_nav_icons` so it always has the
    /// final say for the frame — `settle` (inside `advance_demo`) resets
    /// tiles/buttons to hardcoded light-treatment values on every scene
    /// arrival, and this overwrites that with the current theme's actual
    /// values immediately after. One consequence: during the
    /// `VideoTiles ↔ VideoScreen` `TransitionRequest`, the framework's own
    /// transition-tick system eases `corner_radius` toward
    /// `video_screen_quad`'s hardcoded value, but this function overwrites it
    /// again right after — so corner radius never actually rides that
    /// transition's own eased curve, it's pinned flat to the current theme's
    /// value throughout. Harmless today since `TILE_CORNER_RADIUS` and
    /// `SCREEN_CORNER_RADIUS` are the same 20.0 (both theme-driven
    /// identically) — don't "fix" this later by adding corner_radius back
    /// into a `TransitionRequest` target expecting it to interpolate.
    fn advance_theme(&mut self, dt: f32) {
        // 1. Click handling — orthogonal to AppState, reads the same
        // InteractionEvents resource advance_demo already consumed this
        // frame (read-only, no staleness risk).
        let clicked: Vec<Entity> = self
            .ui_world
            .world
            .resource::<InteractionEvents>()
            .clicked
            .clone();
        if clicked.contains(&self.moon_icon) {
            self.dark_target = true;
        }
        if clicked.contains(&self.sun_icon) {
            self.dark_target = false;
        }

        // 2. Ramp theme_progress toward dark_target.
        let target = if self.dark_target { 1.0 } else { 0.0 };
        let step = dt / THEME_MORPH_DURATION;
        if self.theme_progress < target {
            self.theme_progress = (self.theme_progress + step).min(target);
        } else if self.theme_progress > target {
            self.theme_progress = (self.theme_progress - step).max(target);
        }
        let p = self.theme_progress;

        // 3. Active icon = whichever does NOT match the current theme.
        // Idempotent inserts/removes — cheap, no need to track "did I
        // already do this."
        if self.dark_target {
            self.ui_world
                .world
                .entity_mut(self.sun_icon)
                .insert(Interactable);
            self.ui_world
                .world
                .entity_mut(self.moon_icon)
                .remove::<Interactable>();
        } else {
            self.ui_world
                .world
                .entity_mut(self.moon_icon)
                .insert(Interactable);
            self.ui_world
                .world
                .entity_mut(self.sun_icon)
                .remove::<Interactable>();
        }

        // 4. Corner radius — nav_buttons always theme-driven directly.
        // Tiles/screen are trickier: `tiles[i]` reuses the same entity for
        // both shapes, and while it's actively mid the VideoTiles ↔
        // VideoScreen `TransitionRequest`, its corner_radius is already
        // being eased by the framework's own transition-tick system (from
        // whichever shape it's leaving to whichever it's entering — see
        // `start_tiles_to_screen`/`start_screen_to_tiles`). Reasserting a
        // flat value here every frame would stomp that mid-flight, so the
        // corner radius would never actually round up/down as the shape
        // grows/shrinks — it'd just pop once `settle` lands. So this only
        // reasserts a theme-blended radius for tiles at rest (either
        // grid-shaped or fully the screen); whichever one entity is actively
        // morphing right now is left alone until it settles.
        for i in 0..3 {
            let btn = self.nav_buttons[i];
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(btn) {
                qs.corner_radius = NAV_BUTTON_CORNER_RADIUS
                    + (NAV_BUTTON_CORNER_RADIUS_DARK - NAV_BUTTON_CORNER_RADIUS) * p;
            }
        }
        let transition_pair = self.transition.as_ref().map(|t| (t.from, t.to));
        let screen_focus_idx = match transition_pair {
            Some((AppState::VideoScreen(idx), _)) | Some((_, AppState::VideoScreen(idx))) => {
                Some(idx)
            }
            None => match self.state {
                AppState::VideoScreen(idx) => Some(idx),
                _ => None,
            },
            _ => None,
        };
        for i in 0..3 {
            if self.transition.is_some() && screen_focus_idx == Some(i) {
                continue; // actively morphing — the transition tick owns this one
            }
            let tile = self.tiles[i];
            let (light_r, dark_r) = if screen_focus_idx == Some(i) {
                (SCREEN_CORNER_RADIUS, SCREEN_CORNER_RADIUS_DARK)
            } else {
                (TILE_CORNER_RADIUS, TILE_CORNER_RADIUS_DARK)
            };
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
                qs.corner_radius = light_r + (dark_r - light_r) * p;
            }
        }

        // 5. Border/Glow RGB lerp — alpha stays independently owned by
        // existing hover/fade code, never touched here.
        let primary = violet().lerp(violet_dark(), p);
        for i in 0..3 {
            let btn = self.nav_buttons[i];
            if let Some(mut b) = self.ui_world.world.get_mut::<Border>(btn) {
                b.color.x = primary.x;
                b.color.y = primary.y;
                b.color.z = primary.z;
            }
            if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(btn) {
                g.color.x = primary.x;
                g.color.y = primary.y;
                g.color.z = primary.z;
            }
            let label = self.nav_labels[i];
            if let Some(mut t) = self.ui_world.world.get_mut::<Text>(label) {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            let tile = self.tiles[i];
            if let Some(mut b) = self.ui_world.world.get_mut::<Border>(tile) {
                b.color.x = primary.x;
                b.color.y = primary.y;
                b.color.z = primary.z;
            }
            if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(tile) {
                g.color.x = primary.x;
                g.color.y = primary.y;
                g.color.z = primary.z;
            }
        }
        for i in 0..2 {
            let icon = self.nav_icons[i];
            if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(icon) {
                g.color.x = primary.x;
                g.color.y = primary.y;
                g.color.z = primary.z;
            }
        }
        if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(self.sun_icon) {
            g.color.x = primary.x;
            g.color.y = primary.y;
            g.color.z = primary.z;
        }
        if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(self.moon_icon) {
            g.color.x = primary.x;
            g.color.y = primary.y;
            g.color.z = primary.z;
        }

        // 7. Dark-overlay crossfades (continuous, theme_progress-driven).
        for e in [
            self.logo_lockup_dark,
            self.home_icon_dark,
            self.back_icon_dark,
            self.background_dark,
            self.moon_icon_dark,
            self.loading_logo_dark,
        ] {
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(e) {
                qs.color.w = p;
            }
        }
        // `sun_icon_dark` holds the solid light-theme disc (see
        // `SUN_ICON_PATH`'s doc) — it must be the fully-opaque one while
        // light (correctly occluding the thin ring underneath) and fade
        // *away* going dark, so its alpha runs inverted from every other
        // overlay above.
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.sun_icon_dark) {
            qs.color.w = 1.0 - p;
        }

        // 8. home_icon_selected/_dark share the home_selected_fade envelope,
        // split by a hard dark_target gate rather than true bilinear
        // (page-selected × theme) compositing — see this fn's doc.
        let (light_w, dark_w) = if self.dark_target {
            (0.0, 1.0)
        } else {
            (1.0, 0.0)
        };
        if let Some(mut qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.home_icon_selected)
        {
            qs.color.w = self.home_selected_fade * light_w;
        }
        if let Some(mut qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.home_icon_selected_dark)
        {
            qs.color.w = self.home_selected_fade * dark_w;
        }

        // 9. Fade sun/moon in — same "visible once past Splash, forever
        // after" signal advance_nav_icons computes for the logo/home icon,
        // recomputed locally here (each advance_* function in this file
        // independently recomputes its own derived state rather than
        // sharing a field, matching the existing style).
        let chrome_visible: f32 = match (&self.transition, self.state) {
            (Some(t), _) if t.from == AppState::Splash => 0.0,
            (None, AppState::Splash) => 0.0,
            _ => 1.0,
        };
        if chrome_visible > 0.0 {
            if let Some(mut v) = self.ui_world.world.get_mut::<Visibility>(self.sun_icon) {
                v.visible = true;
            }
            if let Some(mut v) = self.ui_world.world.get_mut::<Visibility>(self.moon_icon) {
                v.visible = true;
            }
        }
        let fade_step = dt / NAV_ICON_FADE_DURATION;
        for fade in &mut self.theme_icon_fade {
            if *fade < chrome_visible {
                *fade = (*fade + fade_step).min(chrome_visible);
            } else if *fade > chrome_visible {
                *fade = (*fade - fade_step).max(chrome_visible);
            }
        }

        // 10. Position — mirror image of advance_nav_icons' home/back
        // layout, anchored to the right edge. Moon = outer (closest to
        // edge), sun = inner.
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let right_edge = window_width / 2.0 - THEME_ICON_MARGIN_PX;
        let moon_x = right_edge - THEME_ICON_SIZE_PX / 2.0;
        let sun_x = moon_x - THEME_ICON_SIZE_PX - THEME_ICON_GAP_PX;
        let y = window_height / 2.0 - THEME_ICON_MARGIN_PX - THEME_ICON_SIZE_PX / 2.0;
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.sun_icon) {
            qs.position.x = sun_x;
            qs.position.y = y;
            qs.color.w = self.theme_icon_fade[0];
        }
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.moon_icon) {
            qs.position.x = moon_x;
            qs.position.y = y;
            qs.color.w = self.theme_icon_fade[1];
        }

        // 11. Hover glow/scale for whichever icon is currently active; force
        // the inactive one back to rest, in case it was mid-hover-animation
        // the instant it flipped inactive.
        let active_icon = if self.dark_target {
            self.sun_icon
        } else {
            self.moon_icon
        };
        // Recomputed from `HoveredEntity` (ground truth for "what's under the
        // pointer right now") rather than this frame's enter/exit events —
        // `active_icon`'s *identity* can flip the instant a click lands (step
        // 3, above), and the newly-active icon won't necessarily get its own
        // hover_entered/hover_exited event on that same frame (the pointer
        // hasn't moved, it's still sitting over the icon that was just
        // clicked, which is now the *inactive* one). Trusting only this
        // frame's event vecs left `theme_icon_is_hovering` stuck at
        // whichever value the old active icon last had — showing a glow on
        // the new active icon until the pointer physically entered and left
        // it once. Comparing directly against the live cursor target is
        // correct regardless of which icon just became active.
        self.theme_icon_is_hovering =
            self.ui_world.world.resource::<HoveredEntity>().0 == Some(active_icon);
        let hover_target = if self.theme_icon_is_hovering {
            1.0
        } else {
            0.0
        };
        let hover_step = dt / GLOW_DURATION;
        if self.theme_icon_hover_progress < hover_target {
            self.theme_icon_hover_progress =
                (self.theme_icon_hover_progress + hover_step).min(hover_target);
        } else if self.theme_icon_hover_progress > hover_target {
            self.theme_icon_hover_progress =
                (self.theme_icon_hover_progress - hover_step).max(hover_target);
        }
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(active_icon) {
            qs.scale = 1.0 + self.theme_icon_hover_progress * HOVER_SCALE_BOOST;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(active_icon) {
            glow.radius = self.theme_icon_hover_progress * GLOW_MAX_RADIUS;
        }
        let inactive_icon = if self.dark_target {
            self.moon_icon
        } else {
            self.sun_icon
        };
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(inactive_icon) {
            qs.scale = 1.0;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(inactive_icon) {
            glow.radius = 0.0;
        }
    }

    // -------------------------------------------------------------------------
    // Demo state machine
    // -------------------------------------------------------------------------

    /// Advance the demo one frame: read `InteractionEvents` (populated by
    /// `hit_test_system` during `ui_world.update()`) and drive `AppState`.
    /// While a `Transition` is in flight, tick it (`drive_transition`) and,
    /// once it reports done, `settle` into the destination before handing
    /// off `self.state` to it. Otherwise, dispatch on the current resting
    /// `AppState` — the splash state's "input" is just its own hold timer,
    /// everything else is click-driven. `dt` drives the splash hold countdown
    /// and the manual tile↔screen fade timers.
    fn advance_demo(&mut self, dt: f32) {
        let clicked: Vec<Entity> = self
            .ui_world
            .world
            .resource::<InteractionEvents>()
            .clicked
            .clone();

        if let Some(mut transition) = self.transition.take() {
            transition.elapsed += dt;
            if self.drive_transition(&transition) {
                self.settle(transition.to);
                self.state = transition.to;
            } else {
                self.transition = Some(transition);
            }
            return;
        }

        match self.state {
            // Not interactive — no `clicked` check. The countdown only
            // starts once the intro slide/fade has fully settled
            // (`intro_elapsed >= INTRO_DURATION`), so "2 seconds to register
            // it" is measured from when the composite is actually done
            // animating in, not from when the whole demo started.
            AppState::Splash => {
                if self.intro_elapsed >= INTRO_DURATION {
                    self.splash_hold_remaining -= dt;
                    if self.splash_hold_remaining <= 0.0 {
                        self.begin_transition(AppState::Splash, AppState::Home);
                    }
                }
            }

            // Click "Video Demo" to converge into the tiles, or "Photo
            // Gallery" to converge into the loading screen. The third
            // button is interactable but intentionally does nothing yet.
            AppState::Home => {
                if clicked.contains(&self.nav_buttons[0]) {
                    self.begin_transition(AppState::Home, AppState::VideoTiles);
                } else if clicked.contains(&self.nav_buttons[1]) {
                    self.begin_transition(AppState::Home, AppState::Loading);
                }
            }

            // Click a tile to morph into the video screen, or click home to
            // converge back into the nav buttons.
            AppState::VideoTiles => {
                if clicked.contains(&self.nav_icons[0]) {
                    self.begin_transition(AppState::VideoTiles, AppState::Home);
                } else if let Some(clicked_idx) =
                    self.tiles.iter().position(|e| clicked.contains(e))
                {
                    self.begin_transition(AppState::VideoTiles, AppState::VideoScreen(clicked_idx));
                }
            }

            // No longer interactive itself — click back to morph into the
            // tile grid, or home to morph straight to the nav buttons; both
            // are 1→N Slice splits (see `start_screen_to_tiles`/
            // `start_screen_to_nav`), so the flat single-tile shape is never
            // shown on the way to either. Both stop playback immediately (in
            // `begin_transition`) rather than carrying it through the morph
            // — a Slice transition bakes a frozen frame of whatever the
            // source looks like at setup time, so there's no "live" video to
            // crossfade through in the first place.
            AppState::VideoScreen(screen_idx) => {
                if clicked.contains(&self.nav_icons[0]) {
                    self.begin_transition(AppState::VideoScreen(screen_idx), AppState::Home);
                } else if clicked.contains(&self.nav_icons[1]) {
                    self.begin_transition(AppState::VideoScreen(screen_idx), AppState::VideoTiles);
                }
            }

            // Home icon is the escape hatch if the fetch is slow/erroring;
            // otherwise auto-advance to Gallery once every tile has baked.
            AppState::Loading => {
                if clicked.contains(&self.nav_icons[0]) {
                    self.begin_transition(AppState::Loading, AppState::Home);
                } else if self
                    .gallery_tiles
                    .iter()
                    .all(|&e| self.ui_world.world.get::<BakedImage>(e).is_some())
                {
                    self.begin_transition(AppState::Loading, AppState::Gallery);
                }
            }

            AppState::Gallery => {
                if clicked.contains(&self.nav_icons[0]) {
                    self.begin_transition(AppState::Gallery, AppState::Home);
                }
            }
        }
    }

    /// Starts whatever entity setup a given `(from, to)` pair needs, then
    /// records it as the in-flight `Transition`. Every arm here dispatches to
    /// the matching `start_*` function's `TransitionRequest`/`OneToNRequest`
    /// setup — adding a new reachable state only means adding one arm here
    /// (and its mirror in `drive_transition`/`settle`).
    fn begin_transition(&mut self, from: AppState, to: AppState) {
        match (from, to) {
            (AppState::Splash, AppState::Home) => self.start_splash_to_nav(),
            (AppState::Home, AppState::VideoTiles) => self.start_nav_to_tiles(),
            (AppState::VideoTiles, AppState::Home) => self.start_tiles_to_nav(),
            (AppState::VideoTiles, AppState::VideoScreen(idx)) => {
                self.start_tiles_to_screen(idx);
                // Playback starts the instant the tile is clicked, not when
                // the morph finishes — it plays underneath the morph.
                self.start_video_playback(idx);
            }
            (AppState::VideoScreen(idx), AppState::VideoTiles) => {
                self.stop_video_playback();
                self.start_screen_to_tiles(idx);
            }
            (AppState::VideoScreen(idx), AppState::Home) => {
                self.stop_video_playback();
                self.start_screen_to_nav(idx);
            }
            (AppState::Home, AppState::Loading) => self.start_home_to_loading(),
            (AppState::Loading, AppState::Gallery) => self.start_loading_to_gallery(),
            (AppState::Loading, AppState::Home) => self.start_loading_to_home(),
            (AppState::Gallery, AppState::Home) => self.start_gallery_to_home(),
            (from, to) => unreachable!("no transition defined for {from:?} -> {to:?}"),
        }
        self.transition = Some(Transition {
            from,
            to,
            elapsed: 0.0,
        });
    }

    /// Ticks a transition's manual fades and reports whether it has
    /// completed. Mirrors `begin_transition`'s `(from, to)` dispatch, driving
    /// the same fade function and completion condition the old per-phase
    /// code used for each pair.
    fn drive_transition(&mut self, t: &Transition) -> bool {
        match (t.from, t.to) {
            (AppState::Splash, AppState::Home) => self
                .nav_buttons
                .iter()
                .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
            (AppState::Home, AppState::VideoTiles) => self
                .tiles
                .iter()
                .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
            // Both `VideoTiles`'s and `VideoScreen`'s "home" click land here —
            // `start_tiles_to_nav` fires one 1→1 crossfade per tile (tile i
            // onto nav_buttons[i]) while `start_screen_to_nav` fires one 1→3
            // split (the screen tile onto all three buttons), but both are
            // framework-driven `OneToNRequest`s that reveal their own targets
            // on completion — no manual fade needed either way, just wait for
            // every button to come back.
            (AppState::VideoTiles, AppState::Home) | (AppState::VideoScreen(_), AppState::Home) => {
                self.nav_buttons.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                )
            }
            (AppState::VideoTiles, AppState::VideoScreen(idx)) => {
                self.advance_tiles_to_screen_fade(idx, t.elapsed);
                let lifecycle = self.ui_world.world.get::<Lifecycle>(self.tiles[idx]);
                matches!(lifecycle, Some(Lifecycle::Idle))
                    && t.elapsed >= BUTTON_TILES_MORPH_DURATION
            }
            (AppState::VideoScreen(_), AppState::VideoTiles) => self
                .tiles
                .iter()
                .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
            (AppState::Home, AppState::Loading) => {
                matches!(self.ui_world.world.get::<Visibility>(self.loading_logo), Some(v) if v.visible)
            }
            (AppState::Loading, AppState::Gallery) => self
                .gallery_tiles
                .iter()
                .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
            (AppState::Loading, AppState::Home) | (AppState::Gallery, AppState::Home) => self
                .nav_buttons
                .iter()
                .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
            (from, to) => unreachable!("no transition defined for {from:?} -> {to:?}"),
        }
    }

    /// Unconditionally forces every entity this demo touches into `state`'s
    /// correct resting configuration, regardless of which state it just
    /// transitioned from. A `(from, to)` match table alone can't guarantee
    /// this: each arm only ever changes what its own transition touches, so
    /// anything an earlier visit left dirty — stale geometry a group
    /// transition never wrote back (see `settle_tile_geometry`'s doc), a
    /// manual fade that zeroed alpha but never hid the entity — stays dirty
    /// until something re-asserts the whole picture on arrival. `settle` is
    /// that something, called once per landing after the transition's own
    /// animation has finished.
    fn settle(&mut self, state: AppState) {
        match state {
            AppState::Splash => {}

            AppState::Home => {
                self.stop_video_playback();
                self.set_nav_buttons_visible(true);
                for i in 0..3 {
                    self.settle_tile_idle(i, false);
                }
                self.settle_gallery_hidden();
            }

            AppState::VideoTiles => {
                self.stop_video_playback();
                self.set_nav_buttons_visible(false);
                for i in 0..3 {
                    self.settle_tile_idle(i, true);
                }
            }

            AppState::VideoScreen(screen_idx) => {
                self.set_nav_buttons_visible(false);
                for i in 0..3 {
                    if i == screen_idx {
                        self.settle_tile_screen(i);
                    } else {
                        self.settle_tile_idle(i, false);
                    }
                }
            }

            AppState::Loading => {
                self.set_nav_buttons_visible(false);
                for i in 0..12 {
                    self.settle_gallery_tile(i, false);
                }
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(self.loading_logo)
                {
                    vis.visible = true;
                }
                self.hide_gallery_error();
            }

            AppState::Gallery => {
                self.set_nav_buttons_visible(false);
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(self.loading_logo)
                {
                    vis.visible = false;
                }
                for i in 0..12 {
                    self.settle_gallery_tile(i, true);
                }
            }
        }
    }

    /// Forces all three nav buttons' `Visibility` and, when shown, their full
    /// idle appearance (transparent fill per the Design System spec, opaque
    /// border/glow/label) — the resting picture for `AppState::Home` from any
    /// `from`, whether this is the first-ever `Splash → Home` reveal or a
    /// return trip whose crossfade left alpha/border/glow at some
    /// intermediate value.
    fn set_nav_buttons_visible(&mut self, visible: bool) {
        for i in 0..3 {
            let btn = self.nav_buttons[i];
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(btn) {
                vis.visible = visible;
            }
            if !visible {
                continue;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(btn) {
                qs.color.w = 0.0; // transparent idle fill — Design System spec
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(btn) {
                border.color.w = 1.0;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(btn) {
                glow.radius = 0.0;
                glow.color.w = 1.0;
            }
            if let Some(mut label) = self.ui_world.world.get_mut::<Text>(self.nav_labels[i]) {
                label.color.w = 1.0;
            }
        }
    }

    /// Forces `tiles[i]` back to its resting grid shape, either hidden
    /// (`AppState::Home`, or any other tile while one is the video screen) or
    /// visible (`AppState::VideoTiles`) — hover reset to idle either way, so
    /// a hover ramp frozen mid-transition never lingers into the next scene.
    fn settle_tile_idle(&mut self, i: usize, visible: bool) {
        let tile = self.tiles[i];
        if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(tile) {
            vis.visible = visible;
        }
        self.settle_tile_geometry(i);
        if let Some(mut border) = self.ui_world.world.get_mut::<Border>(tile) {
            border.color.w = 1.0;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
            glow.radius = 0.0;
            glow.color.w = 1.0;
        }
        self.tile_hover_progress[i] = 0.0;
        if let Some(mut overlay_qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.tile_overlays[i])
        {
            overlay_qs.color.w = 0.0;
        }
        if let Some(mut label) = self.ui_world.world.get_mut::<Text>(self.tile_labels[i]) {
            label.color.w = 0.0;
        }
    }

    /// Forces `tiles[screen_idx]` visible as the static, non-interactive
    /// video screen — recomputed from the current window size rather than
    /// trusting wherever the `TransitionRequest` left it lerped to, so a
    /// resize mid-morph can't leave it slightly off.
    fn settle_tile_screen(&mut self, i: usize) {
        let tile = self.tiles[i];
        if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(tile) {
            vis.visible = true;
        }
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let screen = video_screen_quad(window_width, window_height);
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
            qs.position = screen.position;
            qs.size = screen.size;
            qs.corner_radius = screen.corner_radius;
            qs.scale = 1.0;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
            glow.radius = 0.0;
        }
        self.tile_hover_progress[i] = 0.0;
        if let Some(mut overlay_qs) = self
            .ui_world
            .world
            .get_mut::<QuadState>(self.tile_overlays[i])
        {
            overlay_qs.color.w = 0.0;
        }
        if let Some(mut label) = self.ui_world.world.get_mut::<Text>(self.tile_labels[i]) {
            label.color.w = 0.0;
        }
    }

    /// Resets `tiles[i]`'s own `QuadState` back to its grid position/size/
    /// color. Group transitions never write this back onto the real entity
    /// themselves (only the virtual slices get the destination shape — see
    /// `layout_nav_buttons`'s doc), and the tile↔screen `TransitionRequest`
    /// morph leaves it wherever it lerped to — so without an unconditional
    /// reset on every arrival, a tile that ever became the video screen would
    /// carry that geometry into its next appearance as a grid tile (this was
    /// the "full-screen Jellyfish" bug: `start_screen_to_nav` collapsed a
    /// screen-shaped tile straight into the nav button without ever passing
    /// back through this reset).
    fn settle_tile_geometry(&mut self, i: usize) {
        let tile = self.tiles[i];
        let mut state = tile_quad(i, self.theme_progress);
        if self.ui_world.world.get::<BakedImage>(tile).is_some() {
            state.color = white();
        }
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
            *qs = state;
        }
    }

    /// 1→N Slice: the button splits into three vertical slices that each
    /// morph into their own tile. The baked-texture crossfade (each slice
    /// showing an actual crop of the button's rendered appearance — shape,
    /// border, and text — dissolving into a real bake of its target tile,
    /// not a flat-color approximation) is entirely `one_to_n_setup_system`'s
    /// job now: it reaches `GpuContext`/`QuadPipeline` as ECS resources and
    /// does the baking itself. This just declares the request.
    /// Computes each nav button's final centered-as-a-group layout from its
    /// label's baked width (falls back to `NAV_BUTTON_FALLBACK_SIZE` if a
    /// label somehow hasn't baked yet), writes it onto the button's own
    /// `QuadState` (so it matches once the Slice transition reveals it —
    /// group transitions never write the target's own `QuadState`, only the
    /// virtual slices, see `topology::group_transition_complete_system`),
    /// and returns the resulting states for use as `GroupTarget`s.
    fn layout_nav_buttons(&mut self) -> [QuadState; 3] {
        let sizes: [Vec2; 3] = std::array::from_fn(|i| {
            self.ui_world
                .world
                .get::<BakedText>(self.nav_labels[i])
                .map(|b| {
                    Vec2::new(b.pixel_size[0], b.pixel_size[1])
                        + Vec2::splat(2.0 * NAV_BUTTON_PADDING_PX)
                })
                .unwrap_or(NAV_BUTTON_FALLBACK_SIZE)
        });
        let total_width: f32 = sizes.iter().map(|s| s.x).sum::<f32>() + 2.0 * NAV_BUTTON_GAP_PX;
        let mut centers = [0.0f32; 3];
        let mut x = -total_width / 2.0;
        for i in 0..3 {
            centers[i] = x + sizes[i].x / 2.0;
            x += sizes[i].x + NAV_BUTTON_GAP_PX;
        }
        let states: [QuadState; 3] = std::array::from_fn(|i| QuadState {
            position: Vec3::new(centers[i], 0.0, 0.5),
            size: sizes[i],
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: NAV_BUTTON_CORNER_RADIUS,
        });
        for (i, state) in states.iter().enumerate() {
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.nav_buttons[i])
            {
                *qs = state.clone();
            }
        }
        states
    }

    /// 1→N Slice: the splash composite splits into the three nav buttons.
    fn start_splash_to_nav(&mut self) {
        let states = self.layout_nav_buttons();
        let targets = (0..3)
            .map(|i| GroupTarget {
                entity: self.nav_buttons[i],
                state: states[i].clone(),
            })
            .collect();

        self.ui_world
            .world
            .entity_mut(self.button)
            .insert(OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            });
    }

    /// 1→1 Slice ×3, reverse of `start_tiles_to_nav`: each nav button morphs
    /// directly onto the tile closest to it. "Closest" is always same-index
    /// here — see `start_tiles_to_nav`'s doc for why — so button 0 goes to
    /// tile 0, button 1 to tile 1, and so on. Each button gets its own
    /// single-target `OneToNRequest` (three independent requests, not one
    /// 3-target group), the same degenerate single-crossfade shape
    /// `start_tiles_to_nav` uses in reverse.
    fn start_nav_to_tiles(&mut self) {
        for i in 0..3 {
            let mut state = tile_quad(i, self.theme_progress);
            // Same placeholder-tint override as the old splash→tiles morph —
            // see the comment that used to live here.
            if self
                .ui_world
                .world
                .get::<BakedImage>(self.tiles[i])
                .is_some()
            {
                state.color = white();
            }
            self.ui_world
                .world
                .entity_mut(self.nav_buttons[i])
                .insert(OneToNRequest {
                    targets: vec![GroupTarget {
                        entity: self.tiles[i],
                        state,
                    }],
                    default_config: TransitionConfig {
                        duration: BUTTON_TILES_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                    strategy: SplitStrategy::Slice,
                });
        }
    }

    /// 1→1 Slice ×3, reverse of `start_tiles_to_screen`'s sibling: each tile
    /// morphs directly onto the nav button closest to it. "Closest" is always
    /// same-index here — `layout_nav_buttons` and `tile_quad` both lay their
    /// three entities out left-to-right in index order, so tile 0 is nearest
    /// button 0, tile 1 nearest button 1, and so on; no distance computation
    /// needed. Each tile gets its own single-target `OneToNRequest` (three
    /// independent requests, not one 3-source group) — with exactly one
    /// target this degenerates to a single whole-shape baked crossfade rather
    /// than an actual slice, the same way a single-source group transition
    /// does elsewhere (see `start_screen_to_nav`'s old single-target form,
    /// now generalized to all three). Each button's own `QuadState` still
    /// holds the layout
    /// `layout_nav_buttons` wrote the first time it ran — nothing has
    /// touched it since (transitions only ever move virtual entities, not
    /// the real target — see `layout_nav_buttons`'s doc) — so reading it
    /// back here is already the correct destination shape.
    fn start_tiles_to_nav(&mut self) {
        for i in 0..3 {
            let state = self
                .ui_world
                .world
                .get::<QuadState>(self.nav_buttons[i])
                .cloned()
                .unwrap_or_default();
            self.ui_world
                .world
                .entity_mut(self.tiles[i])
                .insert(OneToNRequest {
                    targets: vec![GroupTarget {
                        entity: self.nav_buttons[i],
                        state,
                    }],
                    default_config: TransitionConfig {
                        duration: BUTTON_TILES_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                    strategy: SplitStrategy::Slice,
                });
        }
    }

    /// 1→N Slice, mirror of `start_nav_to_tiles`: `tiles[screen_idx]`
    /// (currently screen-shaped) splits into all three nav buttons at once,
    /// the same "one shape fans out into three" motion as the original
    /// button→tiles split, just sourced from the screen tile instead of the
    /// button and run in reverse. The flat tile grid is never shown.
    fn start_screen_to_nav(&mut self, screen_idx: usize) {
        let targets = (0..3)
            .map(|i| {
                let state = self
                    .ui_world
                    .world
                    .get::<QuadState>(self.nav_buttons[i])
                    .cloned()
                    .unwrap_or_default();
                GroupTarget {
                    entity: self.nav_buttons[i],
                    state,
                }
            })
            .collect();
        self.ui_world
            .world
            .entity_mut(self.tiles[screen_idx])
            .insert(OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            });
    }

    /// Direct 1→1 morph: `tiles[clicked_idx]` becomes the video screen. A
    /// plain `TransitionRequest` on the tile itself — no group/slice
    /// machinery, since only one entity is changing shape.
    fn start_tiles_to_screen(&mut self, clicked_idx: usize) {
        // `surface_config.width`/`height` are physical pixels; world-space
        // geometry is logical pixels (see the comment in `RenderState::new`).
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let mut to = video_screen_quad(window_width, window_height);
        // `video_screen_quad` always bakes in the *light* screen radius —
        // override to the current theme's, so this transition's own eased
        // tick (which `advance_theme` now steps aside for while it's in
        // flight, see its corner radius doc) heads toward the theme-correct
        // value instead of always light.
        to.corner_radius = SCREEN_CORNER_RADIUS
            + (SCREEN_CORNER_RADIUS_DARK - SCREEN_CORNER_RADIUS) * self.theme_progress;
        self.ui_world
            .world
            .entity_mut(self.tiles[clicked_idx])
            .insert(TransitionRequest {
                to,
                config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                from_state: None,
            });
    }

    /// 1→N Slice: `tiles[screen_idx]` (currently screen-shaped) splits into
    /// all three tiles at once — the same "one shape fans out into three"
    /// motion as `start_screen_to_nav`, just landing back on the tile grid
    /// instead of the nav buttons. `tiles[screen_idx]` is both the source
    /// *and* one of the three targets here (it's returning to its own grid
    /// slot) — that's fine: a group transition never writes a target's own
    /// `QuadState` regardless of whether it's also the source, so
    /// `settle(VideoTiles)` (which unconditionally resets every tile's
    /// geometry on arrival, see `settle_tile_geometry`'s doc) is exactly what
    /// makes this correct rather than anything special-cased here.
    fn start_screen_to_tiles(&mut self, screen_idx: usize) {
        let targets = (0..3)
            .map(|i| {
                let mut state = tile_quad(i, self.theme_progress);
                if self
                    .ui_world
                    .world
                    .get::<BakedImage>(self.tiles[i])
                    .is_some()
                {
                    state.color = white();
                }
                GroupTarget {
                    entity: self.tiles[i],
                    state,
                }
            })
            .collect();
        self.ui_world
            .world
            .entity_mut(self.tiles[screen_idx])
            .insert(OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            });
    }

    // ---------------------------------------------------------------------
    // Photo gallery — Home ↔ Loading ↔ Gallery (M12)
    // ---------------------------------------------------------------------

    /// Hides all 12 gallery tiles + the fetch button + `loading_logo`,
    /// resetting their geometry/hover/fade state — the resting picture
    /// whenever gallery content isn't shown (`AppState::Home`, and while
    /// `AppState::Loading` is still fetching).
    fn settle_gallery_hidden(&mut self) {
        for i in 0..12 {
            self.settle_gallery_tile(i, false);
        }
        if let Some(mut vis) = self
            .ui_world
            .world
            .get_mut::<Visibility>(self.gallery_fetch_button)
        {
            vis.visible = false;
        }
        self.gallery_button_fade = 0.0;
        if let Some(mut border) = self
            .ui_world
            .world
            .get_mut::<Border>(self.gallery_fetch_button)
        {
            border.color.w = 0.0;
        }
        if let Some(mut label) = self
            .ui_world
            .world
            .get_mut::<Text>(self.gallery_fetch_button_label)
        {
            label.color.w = 0.0;
        }
        if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(self.loading_logo) {
            vis.visible = false;
        }
        self.hide_gallery_error();
    }

    /// Forces `gallery_tiles[i]` back to its resting grid shape, either
    /// hidden (`AppState::Home`/`Loading`) or visible (`AppState::Gallery`)
    /// — hover reset to idle either way, mirroring `settle_tile_idle`.
    fn settle_gallery_tile(&mut self, i: usize, visible: bool) {
        let tile = self.gallery_tiles[i];
        if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(tile) {
            vis.visible = visible;
        }
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let mut state = gallery_cell_quad(i, window_width, window_height, self.theme_progress);
        if self.ui_world.world.get::<BakedImage>(tile).is_some() {
            state.color = white();
        }
        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
            *qs = state;
        }
        if let Some(mut border) = self.ui_world.world.get_mut::<Border>(tile) {
            border.color.w = 1.0;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
            glow.radius = 0.0;
            glow.color.w = 1.0;
        }
        self.gallery_tile_hover_progress[i] = 0.0;
    }

    fn hide_gallery_error(&mut self) {
        if let Some(mut vis) = self
            .ui_world
            .world
            .get_mut::<Visibility>(self.gallery_error_text)
        {
            vis.visible = false;
        }
    }

    fn show_gallery_error(&mut self) {
        if let Some(mut vis) = self
            .ui_world
            .world
            .get_mut::<Visibility>(self.gallery_error_text)
        {
            vis.visible = true;
        }
    }

    /// Computes each gallery tile's grid layout and writes it onto the
    /// tile's own `QuadState` — group transitions never write a target's own
    /// `QuadState` (see `layout_nav_buttons`'s doc), so this must happen
    /// before `OneToNRequest` is inserted. Returns the same states for use
    /// as `GroupTarget`s.
    fn layout_gallery_tiles(&mut self) -> [QuadState; 12] {
        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let states: [QuadState; 12] = std::array::from_fn(|i| {
            let mut qs = gallery_cell_quad(i, window_width, window_height, self.theme_progress);
            if self
                .ui_world
                .world
                .get::<BakedImage>(self.gallery_tiles[i])
                .is_some()
            {
                qs.color = white();
            }
            qs
        });
        for (i, state) in states.iter().enumerate() {
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_tiles[i])
            {
                *qs = state.clone();
            }
        }
        states
    }

    /// N→1 Slice: the 3 nav buttons merge into `loading_logo`. Also clears
    /// any stale bake from a previous gallery visit (so a second visit
    /// doesn't read last time's images as "already loaded") and kicks off a
    /// fresh fetch — every Home→Loading entry re-fetches, since the "Fetch
    /// New Images" button is a no-op placeholder this pass.
    fn start_home_to_loading(&mut self) {
        for &tile in &self.gallery_tiles {
            self.ui_world
                .world
                .entity_mut(tile)
                .remove::<(BakedImage, TextureRef, Image)>();
        }
        self.gallery_fetch_elapsed = 0.0;
        self.gallery_error_shown = false;
        self.hide_gallery_error();
        self.loading_logo_frame_elapsed = 0.0;
        self.loading_logo_frame_index = 0;

        let sources: Vec<GroupSource> = (0..3)
            .map(|i| GroupSource {
                entity: self.nav_buttons[i],
                state: self
                    .ui_world
                    .world
                    .get::<QuadState>(self.nav_buttons[i])
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        self.ui_world
            .world
            .entity_mut(self.loading_logo)
            .insert(NToOneRequest {
                sources,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
            });

        let scale_factor = self.window.scale_factor() as f32;
        let window_width = self.surface_config.width as f32 / scale_factor;
        let window_height = self.surface_config.height as f32 / scale_factor;
        let side_px = (gallery_cell_size(window_width, window_height) * scale_factor)
            .min(GALLERY_IMAGE_MAX_SIDE as f32) as u32;
        self.gallery_fetch_rx = Some(gallery_fetch::spawn(12, side_px));
    }

    /// 1→N Slice: `loading_logo` splits into all 12 gallery tiles.
    fn start_loading_to_gallery(&mut self) {
        let states = self.layout_gallery_tiles();
        let targets = (0..12)
            .map(|i| GroupTarget {
                entity: self.gallery_tiles[i],
                state: states[i].clone(),
            })
            .collect();
        self.ui_world
            .world
            .entity_mut(self.loading_logo)
            .insert(OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: GALLERY_GRID_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            });
    }

    /// 1→N Slice, the error-escape-hatch path: `loading_logo` splits back
    /// into the 3 nav buttons directly (mirrors `start_screen_to_nav`'s
    /// "read back the already-correct state" idiom — nothing has touched
    /// the buttons' `QuadState` since `layout_nav_buttons` last ran). Drops
    /// the in-flight fetch (if any) so the background thread gives up
    /// rather than continuing to fetch images nobody will see.
    fn start_loading_to_home(&mut self) {
        self.gallery_fetch_rx = None;
        self.hide_gallery_error();
        let targets = (0..3)
            .map(|i| {
                let state = self
                    .ui_world
                    .world
                    .get::<QuadState>(self.nav_buttons[i])
                    .cloned()
                    .unwrap_or_default();
                GroupTarget {
                    entity: self.nav_buttons[i],
                    state,
                }
            })
            .collect();
        self.ui_world
            .world
            .entity_mut(self.loading_logo)
            .insert(OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            });
    }

    /// Three independent N→1 Slice merges, fired in the same frame: each
    /// row's 4 gallery tiles converge into their corresponding nav button
    /// (row 0 → button 0, and so on) — the way to compose an "M sources → N
    /// destinations" effect from the framework's 1↔N primitives (confirmed
    /// during planning: multiple `NToOneRequest`s with different
    /// destinations coexist correctly in the same frame, no framework
    /// changes needed).
    fn start_gallery_to_home(&mut self) {
        for row in 0..GALLERY_ROWS {
            let sources: Vec<GroupSource> = (0..GALLERY_COLS)
                .map(|col| {
                    let entity = self.gallery_tiles[row * GALLERY_COLS + col];
                    let state = self
                        .ui_world
                        .world
                        .get::<QuadState>(entity)
                        .cloned()
                        .unwrap_or_default();
                    GroupSource { entity, state }
                })
                .collect();
            self.ui_world
                .world
                .entity_mut(self.nav_buttons[row])
                .insert(NToOneRequest {
                    sources,
                    default_config: TransitionConfig {
                        duration: GALLERY_GRID_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                });
        }
    }

    /// Drains completed fetch results and, while resting in `Loading`,
    /// accumulates the fetch timeout. The "all 12 baked → go to Gallery"
    /// decision itself lives in `advance_demo`'s `Loading` arm (keeping this
    /// file's existing invariant that every state transition originates
    /// from `advance_demo`'s match, same as `Splash`'s own timer-driven arm)
    /// — this function only owns draining + the timeout-to-error decision.
    fn advance_gallery_fetch(&mut self, dt: f32) {
        self.drain_gallery_fetch();
        if self.state != AppState::Loading || self.transition.is_some() || self.gallery_error_shown
        {
            return;
        }
        self.gallery_fetch_elapsed += dt;
        if self.gallery_fetch_elapsed >= GALLERY_FETCH_TIMEOUT {
            self.gallery_error_shown = true;
            self.show_gallery_error();
        }
    }

    /// Drains `gallery_fetch_rx`, attaching each successful fetch's bytes —
    /// `bake_pending_images` (already generic over any `Image`-bearing
    /// entity) decodes/atlas-packs it on the next tick, same as any other
    /// image. A failed fetch just leaves that tile without `BakedImage`,
    /// contributing to `advance_gallery_fetch`'s "not all loaded yet"
    /// timeout — no separate failure tracking needed.
    fn drain_gallery_fetch(&mut self) {
        let Some(rx) = &self.gallery_fetch_rx else {
            return;
        };
        while let Ok((idx, result)) = rx.try_recv() {
            match result {
                Ok(bytes) => {
                    self.ui_world
                        .world
                        .entity_mut(self.gallery_tiles[idx])
                        .insert(Image::new(bytes));
                }
                Err(e) => log::warn!("gallery tile {idx}: fetch failed: {e}"),
            }
        }
    }

    /// Mirrors `advance_logo_animation`'s timer+index sweep, but loops
    /// forever while `state == Loading` (rather than playing once during
    /// `Splash`) and drives both the light layer (`loading_logo`, reusing
    /// the already-loaded `logo_frames`) and the dark layer
    /// (`loading_logo_dark`, from `loading_logo_frames_dark`) in lockstep —
    /// `advance_theme`'s own crossfade (added to its dark-overlay array)
    /// handles which one is actually visible.
    fn advance_loading_logo_animation(&mut self, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Loading {
            return;
        }
        self.loading_logo_frame_elapsed += dt;
        while self.loading_logo_frame_elapsed >= LOGO_FRAME_DURATION {
            self.loading_logo_frame_elapsed -= LOGO_FRAME_DURATION;
            self.loading_logo_frame_index =
                (self.loading_logo_frame_index + 1) % self.logo_frames.len();
            let (texture_id, baked) = self.logo_frames[self.loading_logo_frame_index].clone();
            self.ui_world
                .world
                .entity_mut(self.loading_logo)
                .insert((baked, TextureRef(texture_id)));
            if let Some((dark_id, dark_baked)) = self
                .loading_logo_frames_dark
                .get(self.loading_logo_frame_index)
            {
                self.ui_world
                    .world
                    .entity_mut(self.loading_logo_dark)
                    .insert((dark_baked.clone(), TextureRef(*dark_id)));
            }
        }
    }

    /// Glow + scale-boost hover reaction for gallery tiles — no overlay-tint/
    /// title-label children exist on them (unlike the video tiles' full
    /// `advance_tile_hover`), so this is a smaller, separate function rather
    /// than a generalization of that one.
    fn advance_gallery_tile_hover(&mut self, dt: f32) {
        let suppressed = self.transition.is_some() || self.state != AppState::Gallery;
        for i in 0..12 {
            let entity = self.gallery_tiles[i];
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&entity) {
                    self.gallery_tile_is_hovering[i] = true;
                } else if events.hover_exited.contains(&entity) {
                    self.gallery_tile_is_hovering[i] = false;
                }
            }
            let target = if suppressed {
                0.0
            } else if self.gallery_tile_is_hovering[i] {
                1.0
            } else {
                0.0
            };
            let step = dt / GLOW_DURATION;
            if self.gallery_tile_hover_progress[i] < target {
                self.gallery_tile_hover_progress[i] =
                    (self.gallery_tile_hover_progress[i] + step).min(target);
            } else if self.gallery_tile_hover_progress[i] > target {
                self.gallery_tile_hover_progress[i] =
                    (self.gallery_tile_hover_progress[i] - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.gallery_tile_hover_progress[i] * GLOW_MAX_RADIUS;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                qs.scale = 1.0 + self.gallery_tile_hover_progress[i] * HOVER_SCALE_BOOST;
            }
        }
    }

    /// Fades `gallery_fetch_button` in only once fully settled in `Gallery`
    /// (i.e. after the 1→12 morph has already revealed the tiles), and out
    /// the instant a Gallery→Home morph begins — paced by
    /// `GALLERY_GRID_MORPH_DURATION` so it lands at 0 exactly when that
    /// morph completes. The button has no background fill (same
    /// transparent-idle-fill convention as the nav buttons), so border +
    /// label alpha are what actually reads as "fading in/out."
    fn advance_gallery_button_fade(&mut self, dt: f32) {
        let target = if self.state == AppState::Gallery && self.transition.is_none() {
            1.0
        } else {
            0.0
        };
        let step = dt / GALLERY_GRID_MORPH_DURATION;
        if self.gallery_button_fade < target {
            self.gallery_button_fade = (self.gallery_button_fade + step).min(target);
        } else if self.gallery_button_fade > target {
            self.gallery_button_fade = (self.gallery_button_fade - step).max(target);
        }
        if let Some(mut vis) = self
            .ui_world
            .world
            .get_mut::<Visibility>(self.gallery_fetch_button)
        {
            vis.visible = self.gallery_button_fade > 0.0;
        }
        if let Some(mut border) = self
            .ui_world
            .world
            .get_mut::<Border>(self.gallery_fetch_button)
        {
            border.color.w = self.gallery_button_fade;
        }
        if let Some(mut glow) = self
            .ui_world
            .world
            .get_mut::<Glow>(self.gallery_fetch_button)
        {
            glow.color.w = self.gallery_button_fade;
        }
        if let Some(mut label) = self
            .ui_world
            .world
            .get_mut::<Text>(self.gallery_fetch_button_label)
        {
            label.color.w = self.gallery_button_fade;
        }
    }

    /// Starts MP4 playback for `tiles[clicked_idx]` (M9.5): probes the file's
    /// dimensions, sizes the pipeline's video texture to match, attaches
    /// `VideoPlayer` so `collect_instances` samples from it, and spawns the
    /// decode thread. Missing/unreadable files degrade gracefully — logs a
    /// warning and leaves the tile showing the video screen's zero-initialized
    /// (transparent) texture, with no playback.
    fn start_video_playback(&mut self, clicked_idx: usize) {
        let path = std::path::Path::new(TILE_VIDEO_PATHS[clicked_idx]);
        let dims = match mp4_player::probe(path) {
            Ok(dims) => dims,
            Err(e) => {
                log::warn!("start_video_playback: {e}");
                return;
            }
        };

        let (texture_id, sender) = self
            .ui_world
            .world
            .resource_mut::<QuadPipeline>()
            .init_video(&self.device, dims.width, dims.height);

        self.ui_world
            .world
            .entity_mut(self.tiles[clicked_idx])
            .insert((VideoPlayer, VideoCrossfade { video_t: 0.0 }));

        let handle = mp4_player::spawn(path.to_path_buf(), sender, dims.width, dims.height);

        self.playing_video = Some(PlayingVideo {
            tile_idx: clicked_idx,
            texture_id,
            handle,
        });
    }

    /// Stops whatever video is currently playing (M9.5): signals the decode
    /// thread and blocks briefly for it to exit, removes `VideoPlayer` from
    /// its tile, and releases the video texture's GPU memory. A no-op if
    /// nothing is playing (e.g. `start_video_playback` failed to find the file).
    fn stop_video_playback(&mut self) {
        let Some(playing) = self.playing_video.take() else {
            return;
        };
        playing.handle.stop();
        self.ui_world
            .world
            .entity_mut(self.tiles[playing.tile_idx])
            .remove::<(VideoPlayer, VideoCrossfade)>();
        self.ui_world
            .world
            .resource_mut::<QuadPipeline>()
            .suspend_video(&self.device, playing.texture_id);
        self.present_timing.reset();
    }

    fn advance_tiles_to_screen_fade(&mut self, clicked_idx: usize, elapsed: f32) {
        let t = (elapsed / BUTTON_TILES_MORPH_DURATION).clamp(0.0, 1.0);
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(self.tiles[clicked_idx]) {
            glow.radius = 0.0;
        }
        // M10: hover overlay + title label are deactivated for every tile for
        // the duration of the morph — hover shouldn't compete visually with
        // an in-flight geometry morph, and without this the clicked tile's
        // label would ride along (children move with their parent "for
        // free," per M10) onto the video screen at its old tile-sized hover
        // alpha. `advance_tile_hover` already ramps `tile_hover_progress`
        // toward 0 while transitioning (see its doc comment), but that ramp
        // takes up to `GLOW_DURATION` and lags a frame behind the phase
        // change; this hard-zeroes it immediately, the same way `glow.radius`
        // is forced above.
        for &overlay in &self.tile_overlays {
            if let Some(mut overlay_qs) = self.ui_world.world.get_mut::<QuadState>(overlay) {
                overlay_qs.color.w = 0.0;
            }
        }
        for &label in &self.tile_labels {
            if let Some(mut label) = self.ui_world.world.get_mut::<Text>(label) {
                label.color.w = 0.0;
            }
        }
        // Live crossfade (M9.8): box art → video, same easing as the morph's
        // own geometry so both read as one motion.
        if let Some(mut crossfade) = self
            .ui_world
            .world
            .get_mut::<VideoCrossfade>(self.tiles[clicked_idx])
        {
            crossfade.video_t = ease_in_out_quad(t);
        }

        let fade_duration = BUTTON_TILES_MORPH_DURATION * 0.5;
        let fade_t = (elapsed / fade_duration).clamp(0.0, 1.0);
        let fade_alpha = 1.0 - ease_out_quad(fade_t);
        for (i, &tile) in self.tiles.iter().enumerate() {
            if i == clicked_idx {
                continue;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
                qs.color.w = fade_alpha;
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(tile) {
                border.color.w = fade_alpha;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
                glow.radius = 0.0;
                glow.color.w = fade_alpha;
            }
        }
    }

    // -------------------------------------------------------------------------
    // Render
    // -------------------------------------------------------------------------

    fn render(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;

        {
            let mut pi = self.ui_world.world.resource_mut::<PointerInput>();
            pi.position = self.staged_pointer.position;
            pi.just_pressed = self.staged_pointer.just_pressed;
            pi.just_released = self.staged_pointer.just_released;
            pi.is_pressed = self.staged_pointer.is_pressed;
        }
        self.staged_pointer.just_pressed = false;
        self.staged_pointer.just_released = false;

        self.ui_world.update(dt);
        self.bake_pending_text();
        self.bake_pending_images();
        self.advance_gallery_fetch(dt);
        self.advance_background();
        self.advance_intro_and_hover(dt);
        self.advance_logo_animation(dt);
        self.advance_loading_logo_animation(dt);
        self.advance_nav_hover(dt);
        self.advance_tile_hover(dt);
        self.advance_gallery_tile_hover(dt);
        self.advance_gallery_button_fade(dt);
        self.advance_demo(dt);
        self.advance_nav_icons(dt);
        self.advance_theme(dt);

        // The functions above mutate `Visibility` directly (e.g. `settle`
        // hiding/revealing tiles and nav buttons) — refresh the cascaded
        // `EffectiveVisibility`/`EffectiveOpacity` collect_instances actually
        // reads, or those changes render one frame late (see
        // `ProteusWorld::refresh_cascades`'s doc — this was the cause of
        // hidden tiles flashing visible for a frame right after a
        // transition landed).
        self.ui_world.refresh_cascades();

        let instances = collect_instances(&mut self.ui_world.world);

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
                return;
            }
            // Window covered, minimized, or off-screen — not an error, just
            // nothing to draw into right now. Skip the frame and try again
            // once the window is visible.
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return;
            }
            e => {
                log::error!("Surface error: {e:?}");
                return;
            }
        };

        let view = frame.texture.create_view(&Default::default());

        let mut pipeline = self.ui_world.world.resource_mut::<QuadPipeline>();

        // Drain the latest decoded video frame (M9.5), if any is playing —
        // a no-op when nothing has called `init_video`.
        pipeline.consume_video_frame(&self.queue);

        if !instances.is_empty() {
            pipeline.upload_instances(&self.queue, &instances);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(BG_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if !instances.is_empty() {
                pipeline.draw(&mut pass);
            }
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
        if self.playing_video.is_some() {
            self.present_timing.record();
        }
    }
}
