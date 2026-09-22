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
//! duplicating the ramp. The label's colour is hardcoded rather than
//! theme-blended — see `violet_dark`'s own doc for why.
//!
//! `VideoScreen`'s loading UI (`backdrop`/`loading_dots`/`error_text`, driven
//! by `Demo::advance_video_loading`) papers over real decode latency — local
//! `.mp4` playback via `ffmpeg` still routinely takes "a second or more" to
//! deliver its first real frame (subprocess spawn + demux/decode warm-up),
//! not just the network-fetch case this UI was originally built for.

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
/// Dark-theme counterpart of [`TILE_CORNER_RADIUS`] — see its doc.
pub const TILE_CORNER_RADIUS_DARK: f32 = 20.0;
pub const BORDER_WIDTH: f32 = 3.0;

/// One placeholder color per tile — shown until (if ever)
/// [`crate::Demo::set_tile_image`] attaches real box-cover art.
const TILE_COLORS: [Vec4; 3] = [
    Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Tiger
    Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
    Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
];

/// Shown in the hover overlay only, never at rest.
pub const TILE_TITLES: [&str; 3] = ["Tiger", "Sintel", "Jellyfish"];
const TILE_LABEL_SIZE_PX: f32 = 18.0;
const TILE_LABEL_LETTER_SPACING_PX: f32 = TILE_LABEL_SIZE_PX * 0.02;
/// The baked glyph run is a fixed size; `scale` composes down the hierarchy
/// multiplicatively, so bumping the label child's own local scale renders
/// the same glyphs visibly bigger once the tile is resting as the (much
/// larger) video screen.
pub const TILE_LABEL_SCREEN_SCALE: f32 = 1.8;
/// The overlay's alpha animates `0.0 → TILE_OVERLAY_MAX_ALPHA` on hover, not
/// to a fully opaque `1.0` — the box art underneath should still read
/// through a dark tint, not be fully hidden.
pub const TILE_OVERLAY_MAX_ALPHA: f32 = 0.5;

/// Video screen loading dots — small and subtle by design, unlike the much
/// larger 19-frame `loading_logo` animation used for the Photo Gallery
/// fetch, which would look clunky at video-screen scale.
const VIDEO_DOT_SIZE_PX: f32 = 12.0;
const VIDEO_DOT_SPACING_PX: f32 = 28.0;
/// Full pulse cycle duration per dot.
pub const VIDEO_DOT_PULSE_PERIOD_SECS: f32 = 1.2;
/// Phase offset between adjacent dots — what makes the pulse read as a
/// left-to-right sequence rather than all 3 dots pulsing in unison.
pub const VIDEO_DOT_PULSE_STAGGER_SECS: f32 = 0.15;
pub const VIDEO_DOT_ALPHA_MIN: f32 = 0.25;
pub const VIDEO_DOT_ALPHA_MAX: f32 = 1.0;

/// How long to wait for the first real decoded frame before giving up and
/// showing `VIDEO_LOAD_ERROR_TEXT` instead of the loading dots — same
/// "elapsed timer → inline error" shape as `crate::GALLERY_FETCH_TIMEOUT_
/// SECS`.
pub const VIDEO_LOAD_TIMEOUT_SECS: f32 = 15.0;
pub const VIDEO_LOAD_ERROR_TEXT: &str = "Couldn't load video — check your connection";
/// How long to sit settled-and-waiting before the loading dots actually
/// show — playback often becomes ready within a beat of settling, and
/// showing the dots immediately in that case reads as a flash rather than a
/// loading indicator.
pub const VIDEO_DOT_SHOW_DELAY_SECS: f32 = 0.25;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// The Color-dark treatment's lighter violet — used here (unlike almost
/// every other text/border/glow color in this crate) as a **hardcoded**
/// label color, not a live `theme_progress` blend target: the label sits on
/// top of a black semi-transparent overlay in both themes, and the lighter
/// violet reads clearly against black regardless of which theme is active.
fn violet_dark() -> Vec4 {
    Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
}

/// The z every idle tile (and, at the very instant a morph starts, the
/// clicked one too) rests at — named so `backdrop_quad`'s own dynamic z
/// (see its doc) can be derived from it directly, instead of duplicating
/// the literal.
pub(crate) const TILE_Z: f32 = 0.5;

/// `idx` 0 = left, 1 = center, 2 = right — a fixed centered row, spaced
/// `TILE_WIDTH + TILE_GAP` center-to-center. Light treatment only — no
/// `theme_progress` lerp, see `screens::background`'s doc for why.
pub(crate) fn tile_quad(idx: usize) -> QuadState {
    let spacing = TILE_WIDTH + TILE_GAP;
    let x = (idx as f32 - 1.0) * spacing;
    QuadState {
        position: Vec3::new(x, 0.0, TILE_Z),
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
/// colors"). Both `Demo::start_home_to_tiles` and
/// `Demo::start_screen_to_tiles` gate the override on `BakedImage` the same
/// way.
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
/// morph.
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

/// `backdrop`'s spawn-time geometry — position/size/scale/corner_radius get
/// overwritten every tick by `Demo::advance_video_loading` to track
/// whichever tile is entering/resting as the video screen — including its
/// z, which `advance_video_loading` recomputes dynamically every tick
/// rather than leaving fixed here (see its own doc for the exact formula
/// and why); this spawn-time value is never actually seen.
///
/// **Not** source's fixed `0.49`. Source puts `video_backdrop` "just behind
/// the tile/screen quad's own z (0.5)", which works there because its own
/// renderer draws in spawn/insertion order, not a global z-sort — nothing
/// else nearby ever "wins" a z comparison it isn't part of. This crate's
/// `collect_instances` sorts *every* root by z globally (see
/// `video_screen_quad`'s own doc for the tie-break bug that already forced
/// once), so a fixed `0.49` would sit *below* the two untouched idle
/// sibling tiles (`video_tiles::TILE_Z`, `0.5`) — normally harmless (the
/// entering/settled tile, opaque, covers it completely) until source's own
/// `advance_tiles_to_screen_fade` behavior (ported to `Demo::
/// advance_video_loading`) fades that tile's own alpha toward 0, both
/// during the entering morph and while settled-and-waiting for the first
/// real frame: with the tile partially or fully transparent, the idle
/// siblings (geometrically inside the much-bigger growing/settled screen's
/// footprint) would render "in front of" backdrop wherever it should be
/// covering them — reported directly as "the other tiles are on top of the
/// one I clicked."
///
/// A single *fixed* z above `0.5` doesn't fully fix this either: the
/// tracked tile's own z is itself sweeping from `TILE_Z` (`0.5`) up to
/// `video_screen_quad`'s settled `0.51` over the same morph, and backdrop
/// must stay strictly *behind* whatever that current value is (or it would
/// wrongly cover the tile's own still-mostly-opaque content early in the
/// fade) while staying strictly *above* `TILE_Z` throughout (or the idle
/// siblings show through again). `Demo::advance_video_loading` instead
/// re-derives it every tick as the midpoint between `TILE_Z` and the
/// tracked tile's own *current* z — always strictly between the two for
/// any current z `> TILE_Z`, converging on the same `0.505` this once was
/// as a static value once the tile settles at `0.51`.
fn backdrop_quad() -> QuadState {
    QuadState {
        position: Vec3::new(0.0, 0.0, TILE_Z),
        color: Vec4::new(0.0, 0.0, 0.0, 1.0),
        ..Default::default()
    }
}

/// One of the 3 loading dots — `idx` 0/1/2 = left/center/right, spaced
/// `VIDEO_DOT_SPACING_PX` apart, centered on the video screen. z=0.52 —
/// *above* this crate's own bumped `video_screen_quad` z (0.51): tying the
/// already-bumped screen would re-open the exact root z-tie-break bug
/// `video_screen_quad`'s own doc already fixed once.
fn loading_dot_quad(idx: usize) -> QuadState {
    QuadState {
        position: Vec3::new((idx as f32 - 1.0) * VIDEO_DOT_SPACING_PX, 0.0, 0.52),
        size: Vec2::new(VIDEO_DOT_SIZE_PX, VIDEO_DOT_SIZE_PX),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: violet(),
        corner_radius: VIDEO_DOT_SIZE_PX / 2.0,
    }
}

/// Same z tier as `loading_dot_quad` (0.52, for the same reason) — never
/// visible at the same time as the dots (`Demo::advance_video_loading`
/// gates them on opposite conditions), so no stacking concern between the
/// two.
fn error_text_quad() -> QuadState {
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.52),
        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
        ..Default::default()
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
    /// A black card mirroring whichever tile is entering/resting as the
    /// video screen, sitting just behind it — closes a "briefly see-through
    /// before the first frame" gap (before `VideoPlayer`'s texture has any
    /// real content, the video-screen quad alone would show through to
    /// whatever's behind it). Independent, standalone, reused across all 3
    /// tiles.
    pub backdrop: Handle,
    /// Three small loading dots, centered on the video screen, pulsing in
    /// sequence — shown only while settled on `VideoScreen` with no frame
    /// shown yet.
    pub loading_dots: [Handle; 3],
    /// Inline "couldn't load" message, centered on the video screen — shown
    /// only once the load has timed out.
    pub error_text: Handle,
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
    // Overlay first, label second — within a z tie, draw order follows
    // spawn order (`SpawnOrder`), so the label renders on top of the
    // overlay.
    let tile_overlays = std::array::from_fn(|idx| {
        let overlay = app.component(ComponentSpec::new(tile_overlay_quad()).non_interactive());
        let _ = tiles[idx].add_child(app, overlay);
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
        let _ = tiles[idx].add_child(app, label);
        label
    });

    // Video screen loading backdrop + dots (see `backdrop_quad`'s doc for
    // the "briefly looks broken" bug this closes). Independent, spawned
    // once and reused across all 3 tiles — geometry/visibility driven
    // entirely by `Demo::advance_video_loading`, called every tick.
    let backdrop = app.component(ComponentSpec::new(backdrop_quad()).non_interactive());
    let loading_dots = std::array::from_fn(|idx| {
        app.component(ComponentSpec::new(loading_dot_quad(idx)).non_interactive())
    });
    // Shown instead of the dots once the load times out — same z as the
    // dots (never visible simultaneously, so no stacking concern).
    let error_text = app.component(
        ComponentSpec::new(error_text_quad())
            .text(Text::new(VIDEO_LOAD_ERROR_TEXT, 18.0).with_color(Vec4::ONE))
            .non_interactive(),
    );

    VideoTiles {
        tiles,
        tile_overlays,
        tile_labels,
        backdrop,
        loading_dots,
        error_text,
    }
}

const SCREEN_WIDTH_FRACTION: f32 = 0.9;
/// height / width — the source videos' native aspect ratio.
const SCREEN_ASPECT: f32 = 720.0 / 1280.0;
/// Also the theme-blend target in `Demo::advance_theme` — unlike
/// `TILE_CORNER_RADIUS`'s pair, this one's dark counterpart
/// (`SCREEN_CORNER_RADIUS_DARK`) is a genuinely different value.
pub const SCREEN_CORNER_RADIUS: f32 = 12.0;
/// Dark-theme counterpart of [`SCREEN_CORNER_RADIUS`] — genuinely different
/// here, unlike [`TILE_CORNER_RADIUS`]'s pair.
pub const SCREEN_CORNER_RADIUS_DARK: f32 = 18.0;
/// Same reasoning/value as `example_detail`'s own top-clearance constant —
/// vertical space reserved (top *and* bottom, here) so the screen never
/// overlaps `screens::nav`'s buttons.
const SCREEN_CLEARANCE_PX: f32 = 110.0;

/// The clicked tile's target geometry once it's grown into the video
/// screen — width driven by `viewport_size.x`, capped so it never overlaps
/// `screens::nav`'s buttons top or bottom. Light treatment only — no
/// `theme_progress` lerp, see `screens::background`'s doc for why.
///
/// The z is deliberately *not* the tiles' own: `tiles[0..3]` are all root
/// entities tied at z=0.5, and `collect_instances` breaks a z tie by
/// `SpawnOrder`, so the growing/settled screen would draw *under* whichever
/// sibling tiles were spawned later, visibly clipped by them wherever
/// their (untransformed, still tile-sized) footprint overlaps the much
/// bigger screen. Bumping to
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
