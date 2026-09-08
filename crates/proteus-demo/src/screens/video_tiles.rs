//! `VideoTiles` — reached from `Home`'s first nav button ("Videos"). Three
//! tiles with box-cover art (falling back to a solid placeholder color if
//! the image never loads) laid out in a fixed centered row. Clicking a tile
//! grows it into `VideoScreen` — real `.mp4` playback via `ffmpeg`, driven
//! by `Demo`'s `take_pending_video_*` injection points (see the crate-root
//! doc: decoding stays a shell concern, same as `Text`/`Image` baking).
//!
//! Hover: a black overlay + title label fade in over the box art (neither
//! visible at rest), plus the Design-System glow/scale every other
//! interactive surface gets — the glow/scale ride the shared
//! `HoverEntry`/`Demo::advance_hovers` engine (registered in `Demo::new`,
//! same as everything else); the overlay/label/screen-scale are
//! tile-specific enough (continuous geometry-tracking through the
//! tile↔screen morph, a hard scale bump once resting as the video screen)
//! to need their own `Demo::advance_tile_hover`, which reads each tile's
//! ramped hover progress back out of that same shared engine rather than
//! duplicating the ramp. Mirrors `proteus-shell-native::advance_tile_hover`
//! exactly, including the label's own hardcoded (not theme-blended)
//! `violet_dark()` — see that constant's own doc for why.
//!
//! Still deferred: the loading-dots/timeout/error UI (papering over decode
//! latency) and the morph-time box-art↔video crossfade polish —
//! `Handle::start_video` switches a tile to the video feed instantly (see
//! that method's doc), no gradual reveal. Tracked as F5c/F5d in the M12.5.5
//! plan.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Glow, Handle, Proteus, QuadState, Text};

pub const TILE_WIDTH: f32 = 200.0;
pub const TILE_HEIGHT: f32 = TILE_WIDTH * 1.5;
const TILE_GAP: f32 = 100.0;
/// Also the theme-blend target in `Demo::advance_theme` — the dark-theme
/// counterpart is numerically identical (`TILE_CORNER_RADIUS_DARK`), wired
/// anyway per this pass's design decision (see `screens::home::CORNER_
/// RADIUS`'s own doc for the same call).
pub const TILE_CORNER_RADIUS: f32 = 20.0;
/// `proteus-shell-native::TILE_CORNER_RADIUS_DARK`.
pub const TILE_CORNER_RADIUS_DARK: f32 = 20.0;
pub const BORDER_WIDTH: f32 = 3.0;

/// One placeholder color per tile — shown until (if ever)
/// [`crate::Demo::set_tile_image`] attaches real box-cover art.
const TILE_COLORS: [Vec4; 3] = [
    Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Tiger
    Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
    Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
];

/// `proteus-shell-native::TILE_TITLES` — shown in the hover overlay only,
/// never at rest.
pub const TILE_TITLES: [&str; 3] = ["Tiger", "Sintel", "Jellyfish"];
const TILE_LABEL_SIZE_PX: f32 = 18.0;
const TILE_LABEL_LETTER_SPACING_PX: f32 = TILE_LABEL_SIZE_PX * 0.02;
/// The baked glyph run is a fixed size; `scale` composes down the hierarchy
/// multiplicatively, so bumping the label child's own local scale renders
/// the same glyphs visibly bigger once the tile is resting as the (much
/// larger) video screen. Mirrors `proteus-shell-native::TILE_LABEL_SCREEN_SCALE`.
pub const TILE_LABEL_SCREEN_SCALE: f32 = 1.8;
/// The overlay's alpha animates `0.0 → TILE_OVERLAY_MAX_ALPHA` on hover, not
/// to a fully opaque `1.0` — the box art underneath should still read
/// through a dark tint, not be fully hidden. Mirrors
/// `proteus-shell-native::TILE_OVERLAY_MAX_ALPHA`.
pub const TILE_OVERLAY_MAX_ALPHA: f32 = 0.5;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// The Color-dark treatment's lighter violet — used here (unlike almost
/// every other text/border/glow color in this crate) as a **hardcoded**
/// label color, not a live `theme_progress` blend target: the label sits on
/// top of a black semi-transparent overlay in both themes, and the lighter
/// violet reads clearly against black regardless of which theme is active.
/// Mirrors `proteus-shell-native`'s own identical choice for `tile_labels`.
fn violet_dark() -> Vec4 {
    Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
}

/// `idx` 0 = left, 1 = center, 2 = right — a fixed centered row, spaced
/// `TILE_WIDTH + TILE_GAP` center-to-center. Mirrors
/// `proteus-shell-native::tile_quad` (light treatment only — no
/// `theme_progress` lerp, see `screens::background`'s doc for why).
pub(crate) fn tile_quad(idx: usize) -> QuadState {
    let spacing = TILE_WIDTH + TILE_GAP;
    let x = (idx as f32 - 1.0) * spacing;
    QuadState {
        position: Vec3::new(x, 0.0, 0.5),
        size: Vec2::new(TILE_WIDTH, TILE_HEIGHT),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: TILE_COLORS[idx],
        corner_radius: TILE_CORNER_RADIUS,
    }
}

/// `tile_quad(idx)`'s own geometry, but with its placeholder tint
/// overridden to opaque white if `tile` already has real box-cover art
/// baked — the correct explicit *target* state for any transition landing
/// back on this tile's own grid slot
/// (`Handle::split_to_with_states`, used by `Demo::start_home_to_tiles`/
/// `Demo::start_screen_to_tiles`). A bare `tile_quad(idx)` would always
/// carry the tint, even multiplying real box art underneath it — a real
/// bug, reported directly ("tiles keep color tint from the original bg
/// colors"). Mirrors `proteus-shell-native::start_nav_to_tiles`/
/// `start_screen_to_tiles`'s own identical `BakedImage`-gated override
/// exactly.
pub(crate) fn tile_target_state(app: &Proteus, tile: Handle, idx: usize) -> QuadState {
    let mut state = tile_quad(idx);
    if tile.baked_image_size(app).is_some() {
        state.color = Vec4::ONE;
    }
    state
}

/// A tile's hover overlay — a black tint faded in on hover, dark enough to
/// read the title label clearly without fully hiding the box art
/// underneath. `size`/`corner_radius` here are only the *spawn-time*
/// values (tile-shaped, inset by the border) — `Demo::advance_tile_hover`
/// recomputes both every tick from the parent tile's own *current*
/// geometry, since (unlike every other child quad in this crate) the
/// parent's shape itself changes continuously through the tile↔screen
/// morph. Mirrors `proteus-shell-native::tile_overlay_quad` exactly.
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

pub struct VideoTiles {
    pub tiles: [Handle; 3],
    /// `ChildOf` the matching `tiles[i]` — see `tile_overlay_quad`'s doc.
    pub tile_overlays: [Handle; 3],
    /// `ChildOf` the matching `tiles[i]` — title text, hidden (alpha 0) at
    /// rest, faded in on hover, scaled up via `TILE_LABEL_SCREEN_SCALE`
    /// once resting as the video screen.
    pub tile_labels: [Handle; 3],
}

pub fn spawn(app: &mut Proteus) -> VideoTiles {
    let tiles = std::array::from_fn(|idx| {
        app.component(
            ComponentSpec::new(tile_quad(idx))
                .border(Border::new(BORDER_WIDTH, violet()))
                .glow(Glow {
                    radius: 0.0,
                    color: violet(),
                    intensity: 1.0,
                }),
        )
    });
    // Overlay first, label second — draw order follows insertion order, so
    // the label renders on top of the overlay, matching
    // `proteus-shell-native`'s own spawn order.
    let tile_overlays = std::array::from_fn(|idx| {
        let overlay = app.component(ComponentSpec::new(tile_overlay_quad()).non_interactive());
        tiles[idx].add_child(app, overlay);
        overlay
    });
    let tile_labels = std::array::from_fn(|idx| {
        let label = app.component(
            ComponentSpec::new(QuadState {
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(
                Text::new(TILE_TITLES[idx], TILE_LABEL_SIZE_PX)
                    .with_color(Vec4::new(
                        violet_dark().x,
                        violet_dark().y,
                        violet_dark().z,
                        0.0,
                    ))
                    .with_letter_spacing(TILE_LABEL_LETTER_SPACING_PX),
            )
            .non_interactive(),
        );
        tiles[idx].add_child(app, label);
        label
    });
    VideoTiles {
        tiles,
        tile_overlays,
        tile_labels,
    }
}

const SCREEN_WIDTH_FRACTION: f32 = 0.9;
/// height / width — the source videos' native aspect ratio.
const SCREEN_ASPECT: f32 = 720.0 / 1280.0;
/// Also the theme-blend target in `Demo::advance_theme` — unlike
/// `TILE_CORNER_RADIUS`'s pair, this one's dark counterpart
/// (`SCREEN_CORNER_RADIUS_DARK`) is a genuinely different value.
pub const SCREEN_CORNER_RADIUS: f32 = 12.0;
/// `proteus-shell-native::SCREEN_CORNER_RADIUS_DARK`.
pub const SCREEN_CORNER_RADIUS_DARK: f32 = 18.0;
/// Same reasoning/value as `example_detail`'s own top-clearance constant —
/// vertical space reserved (top *and* bottom, here) so the screen never
/// overlaps `screens::nav`'s buttons.
const SCREEN_CLEARANCE_PX: f32 = 110.0;

/// The clicked tile's target geometry once it's grown into the video
/// screen — width driven by `viewport_size.x`, capped so it never overlaps
/// `screens::nav`'s buttons top or bottom. Mirrors
/// `proteus-shell-native::video_screen_quad`'s geometry (light treatment
/// only — no `theme_progress` lerp, see `screens::background`'s doc for
/// why), but *not* its z: `tiles[0..3]` are all root entities tied at the
/// same z=0.5, so `collect_instances`' z-sort falls back to iteration
/// order for them — the growing/settled screen would draw *under* whichever
/// sibling tiles happen to iterate later, visibly clipped by them wherever
/// their (untransformed, still tile-sized) footprint overlaps the much
/// bigger screen. `proteus-shell-native` doesn't share this concern (its own
/// renderer sorts draw calls differently), so this is a real difference
/// this port's own architecture needs, not a fidelity gap — bumping to
/// 0.51 (this crate's established "just above resting content" tier, same
/// one `example_detail::CONTENT_Z` uses over its panel's own 0.5) guarantees
/// the screen always wins the tie, growing or settled. `animate_to` lerps
/// `position` (and so `z`) same as every other field, so this ramps in
/// smoothly alongside the rest of the morph, not a jump-cut.
pub fn video_screen_quad(viewport_size: Vec2) -> QuadState {
    let uncapped_height = viewport_size.x * SCREEN_WIDTH_FRACTION * SCREEN_ASPECT;
    let max_height = (viewport_size.y - 2.0 * SCREEN_CLEARANCE_PX).max(0.0);
    let height = uncapped_height.min(max_height);
    let width = height / SCREEN_ASPECT;
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.51),
        size: Vec2::new(width, height),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        // Untinted — multiplies the sampled video texture, see
        // `Handle::start_video`'s doc.
        color: Vec4::ONE,
        corner_radius: SCREEN_CORNER_RADIUS,
    }
}
