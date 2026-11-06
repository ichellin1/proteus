//! `proteus-demo`: the Proteus reference demo, written once against
//! [`proteus_sdk::Proteus`] and run natively and on the web.
//!
//! The demo has a persistent background image and nav chrome, and these
//! screens:
//!
//! - `Splash`: the animated logo, wordmark and intro fade (see
//!   `screens::splash`).
//! - `Home`: "See what Proteus can do", over a grid of eight tiles that lead
//!   to the other screens (see `screens::home`).
//! - `ExampleDetail`: the Effects, Text, Transformations, Layout,
//!   Keyboard & Focus and About panels (see `screens::example_detail`).
//! - `VideoTiles` and `VideoScreen`: box-cover tiles that grow into video
//!   playback (see `screens::video_tiles`).
//! - `Loading`, `Gallery` and `GalleryImage`: twelve fetched photos, and a
//!   larger fetch of whichever one is enlarged (see `screens::gallery`).
//!
//! ## How the demo is split
//!
//! [`Demo`] holds the demo's logic. It changes the [`proteus_sdk::Proteus`]
//! it is given and nothing else, so it has no access to the GPU, the network
//! or assets, and can be tested without them. When it needs one of those, it
//! queues a request that its `take_pending_*` methods return, and receives
//! the result through its `set_*` methods.
//!
//! [`DemoApp`] wraps [`Demo`], implements `proteus_runtime::App`, and is
//! handed to a host crate's `run()`. The host owns the window or canvas, the
//! frame loop and the GPU, and calls `DemoApp`'s `update` once per frame.
//! `update` drains [`Demo`]'s requests and does the work through the
//! `Frame` it is given: loading assets, baking textures, fetching photos and
//! playing video. `app.rs`'s module doc has the details.
//!
//! ## Why so many `let _ = handle.foo(...)`
//!
//! Every mutating [`proteus_sdk::Handle`] method returns
//! `Result<_, proteus_sdk::HandleError>`, which is `#[must_use]`. This app owns
//! every handle it touches and destroys them only at well-defined points, so a
//! dead-handle error here would be a bug in *this* crate rather than a
//! condition to recover from — and there'd be nothing useful to do about it
//! mid-frame anyway. `let _ =` says that deliberately, at each call site, rather
//! than hiding it behind a wrapper.
//!
//! Ignoring the `Result` is not the same as ignoring the failure: the SDK logs
//! a `warn!` naming the method and entity before returning `Err`, so a mistake
//! still shows up in the log. An app that *can* react to one — e.g. a TS app
//! driving components from server data that may reference something already
//! destroyed — should branch on the `Result` instead of copying this pattern.

mod app;
mod gallery_fetch;
mod screens;
pub mod video;

pub use app::DemoApp;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use glam::{Vec2, Vec4};

use proteus_sdk::{
    Action, Border, Code, Easing, Glow, Handle, Image, ImageCrop, InputMode, KeyResult, KeyState,
    MergeLayout, Proteus, QuadState, SplitStrategy, Text, TextureHandle, TransitionConfig,
    VideoHandle,
};

use screens::{
    background, example_detail, gallery, home, loading, nav, splash, theme, video_tiles,
};

/// Initial background/viewport size in logical pixels, used only until the
/// first [`Demo::set_viewport_size`] call; see that method's doc.
/// Matches `proteus-shell-native`'s window size, so the placeholder is never
/// actually shown.
const DEFAULT_VIEWPORT_SIZE: Vec2 = Vec2::new(1280.0, 800.0);

/// How long `theme_progress` takes to ramp fully from one theme to the
/// other — a touch slower than a group transition's own 0.4–0.6s; the
/// whole app re-themes at once, a bigger showcase moment than any one
/// shape transitioning into another.
const THEME_MORPH_DURATION_SECS: f32 = 0.6;

/// The one primary color — border, glow, and idle text/icon color all draw
/// from this single value everywhere in the demo (no separate per-component
/// colors).
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// The dark-theme counterpart to [`violet`] — every Border/Glow/Text
/// primary color continuously lerps between the two as `theme_progress`
/// ramps (`Demo::blend_primary_color`).
fn violet_dark() -> Vec4 {
    Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
}

/// A corner radius `p` of the way from the light theme's, `light`, to the
/// dark theme's, `dark` (0 = light, 1 = dark). A transition's end state uses
/// it too, so that it ends at the radius the theme then keeps.
pub(crate) fn themed_radius(light: f32, dark: f32, p: f32) -> f32 {
    light + (dark - light) * p
}

/// Blends `handle`'s corner radius between `light` and `dark` by `p`
/// (0 = light, 1 = dark): both how it looks now and where it rests, its
/// declared geometry, so that a transition into it, such as a panel going back
/// to a button, ends at the themed radius rather than snapping to it after. A
/// no-op if `handle` has no `QuadState`, for example because it was destroyed.
fn blend_corner_radius(app: &mut Proteus, handle: Handle, light: f32, dark: f32, p: f32) {
    let radius = themed_radius(light, dark, p);
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(handle.id()) {
        qs.corner_radius = radius;
    }
    if let Some(mut declared) = handle.declared_geometry(app) {
        if declared.corner_radius != radius {
            declared.corner_radius = radius;
            let _ = handle.declare_geometry(app, declared);
        }
    }
}

/// Blends `handle`'s Border/Glow/Text RGB — never alpha, which stays
/// independently owned by whatever hover/fade code the entity already has
/// — toward [`violet_dark`] by `p`. Writes whichever of Border/Glow/Text
/// `handle` actually carries; a no-op for whichever it lacks.
fn blend_primary_color(app: &mut Proteus, handle: Handle, p: f32) {
    let primary = violet().lerp(violet_dark(), p);
    if let Some(mut border) = app.world_mut().get_mut::<Border>(handle.id()) {
        border.color.x = primary.x;
        border.color.y = primary.y;
        border.color.z = primary.z;
    }
    if let Some(mut glow) = app.world_mut().get_mut::<Glow>(handle.id()) {
        glow.color.x = primary.x;
        glow.color.y = primary.y;
        glow.color.z = primary.z;
    }
    if let Some(mut text) = app.world_mut().get_mut::<Text>(handle.id()) {
        text.color.x = primary.x;
        text.color.y = primary.y;
        text.color.z = primary.z;
    }
}

/// A video tile's hover and focus, set by its callbacks, which only get
/// `&mut Proteus`, and the ramp of its overlay and title that follows them:
/// see `Demo::advance_tile_hover`.
struct TileLight {
    hovered: Rc<Cell<bool>>,
    focused: Rc<Cell<bool>>,
    /// 0 (rest) → 1 (overlay and title fully shown), over
    /// `screens::styles::SECS`, as the tile's glow is.
    progress: f32,
}

impl TileLight {
    /// Wires `tile`'s hover and focus callbacks to a new `TileLight`.
    fn wire(app: &mut Proteus, tile: Handle) -> Self {
        let hovered = Rc::new(Cell::new(false));
        let focused = Rc::new(Cell::new(false));
        {
            let flag = hovered.clone();
            tile.on_hover_enter(app, move |_| flag.set(true));
        }
        {
            let flag = hovered.clone();
            tile.on_hover_exit(app, move |_| flag.set(false));
        }
        {
            let flag = focused.clone();
            tile.on_focus(app, move |_| flag.set(true));
        }
        {
            let flag = focused.clone();
            tile.on_blur(app, move |_| flag.set(false));
        }
        Self {
            hovered,
            focused,
            progress: 0.0,
        }
    }
}

/// The config for group transitions, except those touching `gallery`'s grid
/// (see [`gallery_group_transition_config`]).
fn group_transition_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.4,
        delay: 0.0,
        easing: Easing::EaseInOutQuad,
    }
}

/// A touch slower than `group_transition_config`'s 0.4s — a bigger, more
/// deliberate fan-out (1↔12 vs 1↔3) reads better slower. Used by every
/// transition touching `gallery`'s 12-tile grid; `Home↔Loading` (a 1-target
/// destination, the logo) stays on the standard duration.
fn gallery_group_transition_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.6,
        delay: 0.0,
        easing: Easing::EaseInOutQuad,
    }
}

/// How a single component splits into Home's 4 by 2 grid, each tile from its
/// own part of it.
const HOME_GRID_SPLIT: SplitStrategy = SplitStrategy::Grid {
    cols: home::COLS,
    rows: home::ROWS,
};
/// How Home's 4 by 2 grid merges into a single component, the reverse of
/// [`HOME_GRID_SPLIT`].
const HOME_GRID_MERGE: MergeLayout = MergeLayout::Grid {
    cols: home::COLS,
    rows: home::ROWS,
};

/// Overall fetch timeout, past which `Loading` gives up and shows
/// `loading::ERROR_TEXT` instead of proceeding to `Gallery`.
const GALLERY_FETCH_TIMEOUT_SECS: f32 = 10.0;

/// How long `gallery.hires_overlay` takes to fade from transparent to
/// fully opaque once it's ready to show — see
/// `Demo::advance_gallery_hires_overlay`'s doc.
const GALLERY_HIRES_CROSSFADE_DURATION_SECS: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Splash,
    Home,
    /// One of `example_detail`'s panels, by its index in
    /// `example_detail::CATEGORY_TITLES`.
    ExampleDetail(usize),
    VideoTiles,
    VideoScreen(usize),
    /// Fetching `gallery::TILE_COUNT` images in the background — the
    /// animated logo loops on `loading.logo` while waiting. Auto-advances
    /// to `Gallery` once every tile has a current-generation baked image
    /// (and the logo's played through at least one full loop), or shows
    /// `loading.error_text` after `GALLERY_FETCH_TIMEOUT_SECS` if they
    /// don't all arrive in time — see `Demo::advance_gallery_fetch`.
    Loading,
    /// The fetched grid is visible — click home to converge back to Home's
    /// grid (column by column), click "Fetch New Images"
    /// to refetch (back through `Loading`), or click a tile to enlarge it
    /// (`GalleryImage`).
    Gallery,
    /// `gallery.enlarged` showing photo `.0`'s full frame — click it (or
    /// `nav::Nav::back`) to converge back to the grid, click home to
    /// converge straight to Home's grid. Entering this state also
    /// queues a hires fetch (`Demo::take_pending_gallery_hires_fetch`) that
    /// swaps in once it arrives (`Demo::set_gallery_hires_image`) — see
    /// `Demo::start_gallery_to_image`'s doc.
    GalleryImage(usize),
}

/// A queued click from one of `nav_click`'s registered callbacks —
/// see that field's doc for why clicks can't drive the state machine
/// directly from inside a callback.
#[derive(Debug, Clone, Copy)]
enum NavClick {
    /// Home's Video Demo tile.
    OpenVideoTiles,
    /// Home's Photo Gallery tile.
    OpenGallery,
    /// `gallery.fetch_button` — only meaningful from `Gallery`.
    RefetchGallery,
    /// One of `gallery`'s 12 tiles — only meaningful from `Gallery`.
    OpenGalleryImage(usize),
    /// `nav::Nav::home` — always goes to `Home`, regardless of which screen
    /// it was clicked from (`advance_nav_click` picks the right topology
    /// from `self.state` at the time it's processed).
    GoHome,
    /// One of Home's example tiles, by its index in
    /// `example_detail::CATEGORY_TITLES`.
    OpenExampleDetail(usize),
    /// One of `video_tiles`' 3 tiles.
    OpenVideoScreen(usize),
    /// `nav::Nav::back`, or the Back action (Escape or Backspace) —
    /// meaningful from `ExampleDetail` (back to `Home`), `VideoScreen` (back
    /// to `VideoTiles`), or `GalleryImage` (back to `Gallery`, also fired by
    /// clicking `gallery.enlarged` itself — see `Demo::advance_nav_click`'s
    /// match arm); on the screens Home leads to directly, the Back action
    /// goes Home. `advance_nav_click` picks the right topology from
    /// `self.state`, same as `GoHome`.
    Back,
    /// One of the Keyboard & Focus panel's keycaps, chosen with a click or
    /// Enter, which flashes it.
    FlashKey(usize),
    /// `theme.sun`/`theme.moon` — sets `dark_target` directly (`false`/
    /// `true`), unconditional on `self.state` (a theme switch is orthogonal
    /// to navigation, unlike all the other `NavClick` variants).
    SetTheme(bool),
}

/// A transition target's *children* to reveal once the transition's own
/// duration has elapsed. `reveal_on_complete` (built into
/// `split_to`/`merge_from`) only knows about the literal target/source
/// entities it was given, not their descendants or associated standalone
/// content. Used for `screens::home`'s labels and each `ExampleDetail`
/// category's content.
struct PendingReveal {
    elapsed: f32,
    duration: f32,
    entities: Vec<Handle>,
}

/// Marks that `video_tiles`' 3 tiles need resetting back to their resting
/// shape/appearance (all 3, not just `tile` — see `Demo::
/// advance_pending_tile_reset`'s own doc for why), queued by
/// `start_screen_to_tiles`/`start_screen_to_home` right after they call
/// `split_to`/`split_to_with_states` on `tile`.
///
/// This can't happen synchronously in either of those functions: `split_to`
/// only *inserts* a request component — the system that actually processes
/// it (captures `tile`'s *current* geometry as the group transition's
/// "from" state, then hides it) doesn't run until the *next* tick.
/// Resetting `tile`'s geometry immediately would corrupt that capture
/// before it happens, collapsing the crossfade into an animation
/// from-and-to the same (already-reset) shape — no visible transition at all,
/// just an instant snap. `Demo::advance_pending_tile_reset` instead waits
/// until `tile` is actually observed hidden (proof the setup system has
/// already run and captured the real "from" state), only *then* resets
/// everything — safe whether or not `tile` is also one of the split's own
/// targets and gets revealed again later, since a hidden entity's geometry
/// can't be seen anyway.
struct PendingTileReset {
    tile: Handle,
}

/// Where focus goes once a transition ends: see `Demo::land_focus`.
struct PendingFocus {
    target: Handle,
    /// Seconds until the transition ends.
    wait: f32,
}

/// A key the key map doesn't bind, pressed, as the app's key handler saw
/// it: queued for `Demo::advance_keyboard`, since the handler only gets
/// `&mut Proteus`. Matched to a keycap by where it is, its `code`. Caps Lock
/// arrives as a press and a release at each tap, on every host.
struct UnboundKey {
    code: Code,
}

/// The video tiles' merges from Home's grid, by column: each video tile is
/// `(first column, columns)` of the grid, the second taking the middle two.
const VIDEO_TILE_COLUMNS: [(usize, usize); 3] = [(0, 1), (1, 2), (3, 1)];

/// A fresh gallery fetch to kick off — drained by `DemoApp` via
/// [`Demo::take_pending_gallery_fetch`], which owns starting
/// `gallery::TILE_COUNT` concurrent image downloads its own way (native:
/// blocking HTTP calls on background threads; web: `fetch()`) at roughly
/// `tile_side_px` each, then calling [`Demo::set_gallery_tile_image`] per
/// completed download. A fetch that never completes for some tile is fine —
/// `Demo` handles that itself (`GALLERY_FETCH_TIMEOUT_SECS`); `DemoApp`
/// doesn't need a cancellation call of its own for an abandoned fetch (e.g.
/// leaving `Loading` via the home icon before it finishes) — just stop
/// delivering results for it.
///
/// `tile_side_px` is in logical pixels, like all the other sizes `Demo`
/// deals in — `Demo` has no notion of the display's pixel density
/// (see [`Demo::set_viewport_size`]'s doc), so it's `DemoApp`'s job to
/// scale this by its own `scale_factor` (and apply whatever physical-pixel
/// cap it wants) before actually fetching. `DemoApp::advance_gallery` does
/// that (`tile_side_px * scale_factor`, capped at
/// `MAX_TILE_IMAGE_SIDE_PX`). `Demo` fetching a plain, un-scaled logical size
/// would under-fetch on any HiDPI display.
pub struct GalleryFetchRequest {
    pub tile_side_px: u32,
}

/// A hires upgrade to fetch for the currently-enlarged tile — drained by
/// `DemoApp` via [`Demo::take_pending_gallery_hires_fetch`], which fetches
/// `idx`'s photo (the same photo the tile shows, at a bigger size) at exactly
/// `width_px`×`height_px`, then calls [`Demo::set_gallery_hires_image`] with
/// the result. `width_px`/`height_px` already preserve the box's exact aspect
/// ratio (`Demo` computes them from `gallery_tile_aspect[idx]`, the only side
/// that knows the photo's real aspect — see `Demo::start_gallery_to_image`'s
/// doc) — `DemoApp` doesn't need to know or preserve the aspect ratio itself.
///
/// Same logical-pixel/uncapped convention as [`GalleryFetchRequest::
/// tile_side_px`] — see that field's doc. `DemoApp::advance_gallery` scales
/// it by the viewport's `scale_factor` and caps it, as it does for the grid
/// tiles.
///
/// Superseded (should be abandoned, not delivered) by
/// [`Demo::take_pending_gallery_hires_cancel`] firing — see that method's
/// doc.
pub struct GalleryHiresFetchRequest {
    pub idx: usize,
    pub width_px: u32,
    pub height_px: u32,
}

/// The shared reference demo application. One instance per running demo.
///
/// Does **not** own its [`Proteus`]: [`crate::DemoApp`] passes
/// `&mut Proteus` into every method.
pub struct Demo {
    state: AppState,
    viewport_size: Vec2,
    splash_elapsed: f32,
    pending_reveals: Vec<PendingReveal>,
    /// Seconds remaining before the intro fade/slide-in starts —
    /// `splash::INTRO_DELAY_SECS` counting down to 0. See `Demo::advance_intro`.
    intro_delay_remaining: f32,
    /// Seconds into the intro fade/slide-in itself, clamped to
    /// `splash::INTRO_DURATION_SECS`. `Demo::advance_state`'s hold countdown
    /// doesn't start until this reaches the cap.
    intro_elapsed: f32,
    /// `splash::INTRO_SLIDE_DISTANCE_PX * (1.0 - eased_intro_progress)` —
    /// recomputed each tick by `advance_intro`, consumed by
    /// `splash::recenter`. 0 once the intro has fully settled.
    intro_slide_offset: f32,
    /// Splash's animated logo mark's pre-baked frames, in order — empty
    /// until [`Demo::set_logo_frames`] is called. Baking the PNGs is
    /// `DemoApp`'s job (see [`Demo::set_logo_frames`]'s doc); cycling which one is shown
    /// is ordinary app state, tracked here.
    logo_frames: Vec<TextureHandle>,
    logo_frame_index: usize,
    logo_frame_elapsed: f32,
    /// `loading.logo_dark`'s own dark-treatment frames — parallel to
    /// `logo_frames`, same index (`loading_logo_frame_index`), empty until
    /// [`Demo::set_loading_logo_frames_dark`] is called. Splash's own
    /// `logo_frames` never needs a dark counterpart (see
    /// `screens::loading`'s module doc for why only this screen does).
    loading_logo_frames_dark: Vec<TextureHandle>,
    /// The full-window background image (light + dark) — persistent
    /// chrome, not owned by any one screen.
    background: background::Background,
    /// Set by whichever `on_click` callback fired this frame (see
    /// `NavClick`'s doc), consumed by `advance_nav_click`. Callbacks
    /// registered via `Handle::on_click` only receive `&mut Proteus`, not
    /// `&mut Demo` — this shared cell is how a click gets back to code that
    /// can actually drive `Demo`'s own state machine (`self.state`,
    /// `self.active_example_category`, ...), which no callback has access
    /// to. `Rc<Cell<_>>` rather than an ECS resource: single-threaded,
    /// no `proteus-ui` dependency needed to define one.
    nav_click: Rc<Cell<Option<NavClick>>>,
    active_example_category: Option<usize>,
    /// Keys the key map doesn't bind, queued by the app's key handler for
    /// `Demo::advance_keyboard`.
    unbound_keys: Rc<RefCell<Vec<UnboundKey>>>,
    /// Each virtual keycap's flash, from 1 as it's pressed fading to 0 over
    /// `keyboard::FLASH_SECS`.
    key_flash: Vec<f32>,
    /// Whether the virtual keyboard shows its shifted values, toggled by
    /// Caps Lock.
    keyboard_shifted: bool,
    /// Where focus goes once the transition under way ends: see
    /// `Demo::land_focus`.
    pending_focus: Option<PendingFocus>,
    /// Seconds until the change of screen under way ends, 0 when none is.
    /// Input that changes screens waits for it: see `Demo::advance_nav_click`.
    screen_change_left: f32,
    /// Fade progress (0..1) for Home's heading and tagline: up while Home is
    /// the current or (mid-transition) destination state.
    home_title_fade: f32,
    /// Set to `Some(tile_idx)` when a tile should start playing video —
    /// drained by `DemoApp` via [`Demo::take_pending_video_start`], which
    /// starts that tile's player (see the crate doc). The entity itself
    /// already shows `video` by the time this is set
    /// (`Handle::show_video`, called synchronously); this flag only tells
    /// `DemoApp` *which* player to start.
    pending_video_start: Option<usize>,
    /// The video the playing tile shows, while one plays. `DemoApp` uploads
    /// the player's frames to it; see [`Demo::video`].
    video: Option<VideoHandle>,
    /// Set when the currently-playing tile should stop — drained by
    /// `DemoApp` via [`Demo::take_pending_video_stop`], which stops the
    /// video and releases its GPU texture.
    pending_video_stop: bool,
    /// Whether `DemoApp` has confirmed a real decoded frame has actually
    /// been uploaded for the currently-playing video — set via
    /// [`Demo::set_video_first_frame_shown`], reset to `false` each time
    /// [`Demo::start_tiles_to_screen`] starts a new video. Drives
    /// [`Demo::advance_video_loading`]'s loading-dots visibility.
    video_first_frame_shown: bool,
    /// Elapsed time (seconds) driving the loading dots' pulse phase — reset
    /// to 0 each time [`Demo::start_tiles_to_screen`] starts a new video, so
    /// the sequence always begins at dot 0 rather than an arbitrary phase.
    /// Also doubles as the "how long have we been waiting" clock
    /// [`Demo::advance_video_loading`] checks against
    /// `video_tiles::VIDEO_LOAD_TIMEOUT_SECS`.
    video_dots_elapsed: f32,
    /// Latches once `video_dots_elapsed` crosses `video_tiles::
    /// VIDEO_LOAD_TIMEOUT_SECS` with no frame shown yet — swaps the loading
    /// dots for `video_tiles::VideoTiles::error_text`. Reset to `false`
    /// each time [`Demo::start_tiles_to_screen`] starts a new video.
    video_load_timed_out: bool,
    /// Set the same instant `video_load_timed_out` first latches, drained
    /// by `DemoApp` via [`Demo::take_pending_video_cancel`] — unlike
    /// `video_load_timed_out` itself (a level, read every tick to decide
    /// dots-vs-error visibility), this is an edge: exactly one `true` read
    /// per timeout, telling `DemoApp` "abort whatever fetch/decode is still
    /// in flight, `Demo` hasn't torn anything down (the screen stays on
    /// `VideoScreen`, now showing the error text), so don't stop playback,
    /// just stop wasting bandwidth." The native shell's `.mp4` decode has no
    /// in-flight fetch to abort, but the web shell's HLS segment fetch does.
    pending_video_cancel: bool,
    /// How long we've been *continuously* settled-and-waiting (tile not
    /// mid-transition, resting on `VideoScreen`, no frame yet) — unlike
    /// `video_dots_elapsed` (which runs from click time and never resets
    /// early), this resets to 0 the instant that condition stops holding,
    /// so it always measures just the current wait. Gates the dots'
    /// `video_tiles::VIDEO_DOT_SHOW_DELAY_SECS` grace period.
    video_settled_elapsed: f32,
    /// Set by `start_screen_to_tiles`/`start_screen_to_home` right after they
    /// call `split_to` on a tile — see [`PendingTileReset`]'s doc for why the
    /// reset can't happen synchronously in either of those functions.
    pending_tile_reset: Option<PendingTileReset>,
    /// Elapsed time driving `example_detail::advance_continuous_animation`
    /// — accumulates only while `ExampleDetail(2)` is active (see that
    /// function's doc for why: pause, don't reset, so it resumes rather
    /// than restarting).
    transforms_anim_elapsed: f32,
    /// `loading.logo`'s own frame-sweep position — separate from Splash's
    /// `logo_frame_index`/`logo_frame_elapsed` since it loops forever
    /// (Splash's plays once) and needs to restart at frame 0 on every fresh
    /// `Loading` visit, not resume Splash's. See
    /// `Demo::advance_loading_logo_animation`.
    loading_logo_frame_index: usize,
    loading_logo_frame_elapsed: f32,
    /// Seconds since the current `Loading` visit's logo fully settled in
    /// (not since the visit began — see `advance_gallery_fetch`'s "settled"
    /// gate). Drives both the ~1.7s minimum-dwell gate (so a fast fetch
    /// can't cut the spinner off mid-loop) and, past
    /// `GALLERY_FETCH_TIMEOUT_SECS`, the error path — one shared counter.
    gallery_fetch_elapsed: f32,
    /// Latches `true` once `GALLERY_FETCH_TIMEOUT_SECS` fires this
    /// `Loading` visit — reset to `false` only when a fresh fetch begins
    /// (`Demo::begin_gallery_fetch`), not by leaving `Loading` some other
    /// way.
    gallery_error_shown: bool,
    /// Cross-fades `loading.logo`/`loading.logo_dark` to fully transparent
    /// once `gallery_error_shown` latches — `1.0` is the resting/visible
    /// value (not `0.0`), reset there by `Demo::begin_gallery_fetch` on
    /// every fresh visit, same convention as `gallery_error_shown` itself.
    /// See `Demo::advance_gallery_error_fade`'s doc for why the spinner
    /// needs this at all (it has no other owner once the error text takes
    /// over its spot).
    gallery_logo_error_fade: f32,
    /// Bumped by `Demo::begin_gallery_fetch` every time a fetch (first or
    /// re-) kicks off. Paired with `gallery_tile_fetch_generation` so the
    /// `Loading` → `Gallery` auto-advance can't be satisfied by a tile that
    /// still shows a *previous* fetch's image — its own current-round fetch
    /// might still be pending, or might have failed.
    gallery_fetch_generation: u32,
    /// Which `gallery_fetch_generation` each tile's *current* image
    /// belongs to — stamped by `Demo::set_gallery_tile_image`. Starts at
    /// `u32::MAX` so it never matches generation `0` before anything's
    /// arrived.
    gallery_tile_fetch_generation: [u32; gallery::TILE_COUNT],
    /// Each tile's photo's real (width, height) ratio, stamped by
    /// `Demo::set_gallery_tile_image` — see that method's doc for why this
    /// can't just be read off the tile's own baked image size.
    /// `Vec2::ONE` (square) before anything's arrived — an arbitrary but
    /// harmless default, since `Gallery`'s auto-advance gate (see
    /// `Demo::advance_gallery_fetch`) guarantees every tile's real value is
    /// in place long before `Demo::start_gallery_to_image` could ever read
    /// a still-default one.
    gallery_tile_aspect: [Vec2; gallery::TILE_COUNT],
    /// `true` for a tile whose fresh `Image` bytes were just set
    /// (`Demo::set_gallery_tile_image`) but whose bake hasn't landed (and
    /// so hasn't been stashed/cropped) yet — polled and cleared by
    /// `Demo::advance_gallery_tile_crop` once it has. Baking is
    /// asynchronous (the renderer bakes `Image` components during the
    /// frame), so this can't happen synchronously inside
    /// `set_gallery_tile_image` itself; there's nothing to stash/crop yet
    /// at that point.
    pending_gallery_tile_crop: [bool; gallery::TILE_COUNT],
    /// This tick's gallery fetch request, if any — drained by `DemoApp` via
    /// [`Demo::take_pending_gallery_fetch`]. See [`GalleryFetchRequest`]'s
    /// doc.
    pending_gallery_fetch: Option<GalleryFetchRequest>,
    /// This tick's hires fetch request, if any — drained by `DemoApp` via
    /// [`Demo::take_pending_gallery_hires_fetch`]. See
    /// [`GalleryHiresFetchRequest`]'s doc.
    pending_gallery_hires_fetch: Option<GalleryHiresFetchRequest>,
    /// Set whenever `GalleryImage` is left (either destination) — drained
    /// by `DemoApp` via [`Demo::take_pending_gallery_hires_cancel`], which
    /// should stop delivering results for whatever hires fetch it has in
    /// flight (same "just stop delivering results, nothing to actually
    /// interrupt" convention as [`Demo::take_pending_gallery_fetch`]'s own
    /// doc). Fires unconditionally on exit, whether or not a fetch was
    /// actually still pending — cheap and always correct.
    pending_gallery_hires_cancel: bool,
    /// `gallery.hires_overlay`'s crossfade-in progress (0 → 1, never
    /// reverses within one visit) — see
    /// `Demo::advance_gallery_hires_overlay`'s doc. Reset to `0.0` at the
    /// start of every fresh `GalleryImage` visit (`start_gallery_to_image`),
    /// so a previous visit's fully-faded-in overlay never flashes in
    /// instantly for a new tile before its own hires image is ready.
    gallery_hires_fade: f32,
    /// Fade-in (once fully settled in `Gallery`) / fade-out (the instant a
    /// `Gallery`→elsewhere transition starts) progress for `gallery.fetch_button`/
    /// `.fetch_button_label` — see `Demo::advance_gallery_button_fade`'s
    /// doc.
    gallery_button_fade: f32,
    /// Each video tile's hover and focus, which its overlay and title follow.
    tile_lights: [TileLight; 3],
    /// `true` once the user has clicked toward dark — flips instantly on
    /// click; `theme_progress` is what actually ramps toward it.
    dark_target: bool,
    /// 0.0 = fully light, 1.0 = fully dark. Ramped toward `dark_target` by
    /// `Demo::advance_theme`, which derives every themed entity's corner
    /// radius/color/image-crossfade alpha from this one scalar.
    theme_progress: f32,
    /// Fade-in progress (0..1) for `[theme.sun, theme.moon]` — shares
    /// `screens::nav`'s icon fade-in timing, tracked separately.
    theme_icon_fade: [f32; 2],
    /// Fade-in progress (0..1) for `nav.lockup` — visible once past
    /// `Splash`, forever after (including on `Home` itself — this row is
    /// persistent brand chrome, not a Home-only convenience).
    nav_lockup_fade: f32,
    /// Fade-in/out progress (0..1) for `[nav.home, nav.back]` — `home`
    /// shares `nav_lockup_fade`'s target (same "visible forever past
    /// Splash" rule); `back` only targets 1 while resting on
    /// `ExampleDetail`/`VideoScreen`/`GalleryImage`.
    nav_icon_fade: [f32; 2],
    /// Fade progress (0..1) for `nav.home_selected`/`.home_selected_dark`'s
    /// shared envelope — targets 1 only while resting on `Home`. The
    /// *final* alpha each of the two actually gets is this value
    /// hard-gated by `dark_target` (`Demo::advance_theme`), not a plain
    /// write of this field.
    nav_home_selected_fade: f32,
    splash: splash::Splash,
    home: home::Home,
    example_detail: example_detail::ExampleDetail,
    nav: nav::Nav,
    video_tiles: video_tiles::VideoTiles,
    loading: loading::Loading,
    gallery: gallery::Gallery,
    theme: theme::Theme,
}

impl Demo {
    pub fn new(proteus: &mut Proteus) -> Self {
        let background = background::spawn(proteus, DEFAULT_VIEWPORT_SIZE);
        let splash = splash::spawn(proteus);
        let home = home::spawn(proteus);
        let example_detail = example_detail::spawn(proteus);
        let nav = nav::spawn(proteus);
        let video_tiles = video_tiles::spawn(proteus);
        let loading = loading::spawn(proteus);
        let gallery = gallery::spawn(proteus, DEFAULT_VIEWPORT_SIZE);
        let theme = theme::spawn(proteus);

        // Every transition target / standalone-content entity starts
        // hidden. Done here rather than with `ComponentSpec::visible(false)`
        // at each spawn site: which screens are up is this state machine's
        // business, not something a screen's own spawn function should have
        // to know about itself.
        let hide = |app: &mut Proteus, handles: &[Handle]| {
            for &h in handles {
                let _ = h.set_visible(app, false);
            }
        };
        hide(proteus, &home.tiles);
        hide(proteus, &home.labels);
        hide(proteus, &[home.heading, home.tagline]);
        hide(
            proteus,
            &[example_detail.panel, nav.home, nav.back, nav.lockup],
        );
        for idx in 0..6 {
            let content = example_detail.content_handles(idx);
            hide(proteus, &content);
        }
        hide(proteus, &video_tiles.tiles);
        hide(proteus, &[video_tiles.backdrop, video_tiles.error_text]);
        hide(proteus, &video_tiles.loading_dots);
        hide(proteus, &[loading.logo, loading.error_text]);
        hide(proteus, &gallery.tiles);
        // `fetch_button_label` deliberately stays out of this — it relies
        // entirely on cascading from `fetch_button`'s own `Visibility`
        // (toggled solely by `Demo::advance_gallery_button_fade`, which
        // never touches the label directly): still effectively hidden right now since its
        // parent is, but its own raw flag needs to stay `VISIBLE` forever
        // so a *later* `fetch_button` reveal doesn't have to also
        // remember to un-hide the label separately.
        hide(proteus, &[gallery.fetch_button]);
        hide(proteus, &[gallery.enlarged, gallery.hires_overlay]);
        hide(proteus, &gallery.tile_full);
        hide(proteus, &[theme.sun, theme.moon]);

        let nav_click: Rc<Cell<Option<NavClick>>> = Rc::new(Cell::new(None));
        for (i, &tile) in home.tiles.iter().enumerate() {
            let click = match i {
                0 => NavClick::OpenVideoTiles,
                1 => NavClick::OpenGallery,
                _ => NavClick::OpenExampleDetail(i - home::FIRST_EXAMPLE),
            };
            let flag = nav_click.clone();
            tile.on_click(proteus, move |_| flag.set(Some(click)));
        }
        {
            let flag = nav_click.clone();
            nav.home
                .on_click(proteus, move |_| flag.set(Some(NavClick::GoHome)));
        }
        {
            let flag = nav_click.clone();
            nav.back
                .on_click(proteus, move |_| flag.set(Some(NavClick::Back)));
        }
        // Every surface's glow and growth on hover or focus are interaction
        // styles: see `screens::styles`, and `screens::keyboard` for the
        // keycaps'.
        for (i, &key) in example_detail.keys.keyboard.keys.iter().enumerate() {
            let flag = nav_click.clone();
            key.on_click(proteus, move |_| flag.set(Some(NavClick::FlashKey(i))));
        }
        for idx in 0..3 {
            let flag = nav_click.clone();
            video_tiles.tiles[idx].on_click(proteus, move |_| {
                flag.set(Some(NavClick::OpenVideoScreen(idx)))
            });
        }
        let tile_lights = video_tiles.tiles.map(|tile| TileLight::wire(proteus, tile));
        {
            let flag = nav_click.clone();
            gallery
                .fetch_button
                .on_click(proteus, move |_| flag.set(Some(NavClick::RefetchGallery)));
        }
        for idx in 0..gallery::TILE_COUNT {
            let flag = nav_click.clone();
            gallery.tiles[idx].on_click(proteus, move |_| {
                flag.set(Some(NavClick::OpenGalleryImage(idx)))
            });
        }
        {
            // Clicking the enlarged image itself is a same-effect
            // alternative to the explicit back button — see
            // `NavClick::Back`'s doc.
            let flag = nav_click.clone();
            gallery
                .enlarged
                .on_click(proteus, move |_| flag.set(Some(NavClick::Back)));
        }
        {
            let flag = nav_click.clone();
            theme
                .sun
                .on_click(proteus, move |_| flag.set(Some(NavClick::SetTheme(false))));
        }
        {
            let flag = nav_click.clone();
            theme
                .moon
                .on_click(proteus, move |_| flag.set(Some(NavClick::SetTheme(true))));
        }

        // The app's key handler, which sees the keys no component handled:
        // Back goes back a screen, and the keys the key map doesn't bind are
        // queued for the Keyboard & Focus panel's keyboard.
        let unbound_keys: Rc<RefCell<Vec<UnboundKey>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let flag = nav_click.clone();
            let keys = unbound_keys.clone();
            proteus.on_key(move |_, event| {
                if event.action == Some(Action::Back) && event.state == KeyState::Pressed {
                    flag.set(Some(NavClick::Back));
                    return KeyResult::Handled;
                }
                if event.action.is_none() && event.state == KeyState::Pressed {
                    keys.borrow_mut().push(UnboundKey {
                        code: event.code.clone(),
                    });
                }
                KeyResult::Ignored
            });
        }

        Self {
            state: AppState::Splash,
            viewport_size: DEFAULT_VIEWPORT_SIZE,
            splash_elapsed: 0.0,
            pending_reveals: Vec::new(),
            intro_delay_remaining: splash::INTRO_DELAY_SECS,
            intro_elapsed: 0.0,
            intro_slide_offset: splash::INTRO_SLIDE_DISTANCE_PX,
            logo_frames: Vec::new(),
            logo_frame_index: 0,
            logo_frame_elapsed: 0.0,
            loading_logo_frames_dark: Vec::new(),
            background,
            nav_click,
            active_example_category: None,
            unbound_keys,
            key_flash: vec![0.0; example_detail.keys.keyboard.keys.len()],
            keyboard_shifted: false,
            pending_focus: None,
            screen_change_left: 0.0,
            home_title_fade: 0.0,
            pending_video_start: None,
            video: None,
            pending_video_stop: false,
            video_first_frame_shown: false,
            video_dots_elapsed: 0.0,
            video_load_timed_out: false,
            pending_video_cancel: false,
            video_settled_elapsed: 0.0,
            pending_tile_reset: None,
            transforms_anim_elapsed: 0.0,
            loading_logo_frame_index: 0,
            loading_logo_frame_elapsed: 0.0,
            gallery_fetch_elapsed: 0.0,
            gallery_error_shown: false,
            gallery_logo_error_fade: 1.0,
            gallery_fetch_generation: 0,
            gallery_tile_fetch_generation: [u32::MAX; gallery::TILE_COUNT],
            gallery_tile_aspect: [Vec2::ONE; gallery::TILE_COUNT],
            pending_gallery_tile_crop: [false; gallery::TILE_COUNT],
            pending_gallery_fetch: None,
            pending_gallery_hires_fetch: None,
            pending_gallery_hires_cancel: false,
            gallery_hires_fade: 0.0,
            gallery_button_fade: 0.0,
            tile_lights,
            dark_target: false,
            theme_progress: 0.0,
            theme_icon_fade: [0.0; 2],
            nav_lockup_fade: 0.0,
            nav_icon_fade: [0.0; 2],
            nav_home_selected_fade: 0.0,
            splash,
            home,
            example_detail,
            nav,
            video_tiles,
            loading,
            gallery,
            theme,
        }
    }

    /// Injects the background's light-theme image bytes — `DemoApp` reads
    /// the file (or `fetch()`s it, on web) its own way and hands over the
    /// bytes, and the renderer bakes them into `main_atlas` during the frame,
    /// as it does `Text`. Call once, before the first `tick`.
    pub fn set_background_image(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.background.light.id())
            .insert(Image::new(bytes));
    }

    /// Injects the background's dark-theme image bytes — same convention as
    /// [`Demo::set_background_image`]. Call once, before the first `tick`;
    /// only visibly matters once the theme toggle is actually clicked
    /// toward dark (`theme_progress > 0`), but there's no harm baking it
    /// up front regardless.
    pub fn set_background_image_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.background.dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.home`'s idle-art bytes — same `DemoApp`-does-the-I/O
    /// convention as [`Demo::set_background_image`]. Call once, before the
    /// first `tick`; a tile whose bytes never arrive just shows the bare
    /// (fully transparent) quad, same graceful-degradation convention as
    /// [`Demo::set_tile_image`].
    pub fn set_nav_home_icon(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.home.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.home_dark`'s overlay art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_home_icon_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.home_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.home_selected`'s overlay art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_home_icon_selected(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.home_selected.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.home_selected_dark`'s overlay art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_home_icon_selected_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.home_selected_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.back`'s idle-art bytes — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_back_icon(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.back.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.back_dark`'s overlay art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_back_icon_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.back_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.lockup`'s brand-lockup art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_logo_lockup(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.lockup.id())
            .insert(Image::new(bytes));
    }

    /// Injects `nav.lockup_dark`'s overlay art — same convention as
    /// [`Demo::set_nav_home_icon`].
    pub fn set_nav_logo_lockup_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.nav.lockup_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `theme.sun`'s own art — **not** the light-theme sun disc
    /// despite the field name; see `screens::theme`'s module doc for the
    /// inverted-role convention this pair uses (`sun-idle-dark.png`).
    pub fn set_theme_sun_icon(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.theme.sun.id())
            .insert(Image::new(bytes));
    }

    /// Injects `theme.sun_dark`'s overlay art — the *light*-theme sun disc,
    /// per the same inverted-role convention (`sun-selected.png`).
    pub fn set_theme_sun_icon_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.theme.sun_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects `theme.moon`'s own art — `moon-idle.png`, no role inversion
    /// (moon's roles match its name, unlike sun's — see `screens::theme`'s
    /// module doc).
    pub fn set_theme_moon_icon(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.theme.moon.id())
            .insert(Image::new(bytes));
    }

    /// Injects `theme.moon_dark`'s overlay art (`moon-selected-dark.png`).
    pub fn set_theme_moon_icon_dark(&mut self, proteus: &mut Proteus, bytes: Vec<u8>) {
        proteus
            .world_mut()
            .entity_mut(self.theme.moon_dark.id())
            .insert(Image::new(bytes));
    }

    /// Injects one video tile's box-cover art — same `DemoApp`-does-the-I/O
    /// convention as [`Demo::set_background_image`]. `idx` is 0/1/2
    /// (left/center/right); a tile whose image never arrives just keeps its
    /// solid placeholder `TILE_COLORS` fill — degradation is per tile, so
    /// call this only for tiles whose bytes were actually read.
    /// Untints the tile to opaque white so the real art isn't tinted by the
    /// placeholder color underneath.
    pub fn set_tile_image(&mut self, proteus: &mut Proteus, idx: usize, bytes: Vec<u8>) {
        let tile = self.video_tiles.tiles[idx];
        proteus
            .world_mut()
            .entity_mut(tile.id())
            .insert(Image::new(bytes));
        if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color = Vec4::ONE;
        }
    }

    /// Injects one gallery tile's fetched photo bytes — same
    /// `DemoApp`-does-the-I/O convention as [`Demo::set_background_image`]/
    /// [`Demo::set_tile_image`], but a tile can be re-fetched
    /// (`start_gallery_to_loading`'s refetch), unlike either of those, so
    /// this uses `Handle::set_image`, which replaces the previous fetch's
    /// image. Stamps this tile as
    /// belonging to the current fetch generation — see
    /// `gallery_tile_fetch_generation`'s doc. Call only for tiles whose
    /// bytes `DemoApp` actually managed to fetch; a tile that never gets a
    /// call here just keeps showing its plain white placeholder (or
    /// whichever photo it last had, on a refetch) and blocks the
    /// `Loading` → `Gallery` auto-advance until `GALLERY_FETCH_TIMEOUT_SECS`
    /// gives up on it.
    ///
    /// `aspect` is the photo's real (width, height) ratio, known to
    /// `DemoApp` before baking even happens (e.g. from whatever catalog/API
    /// response supplied `bytes`). Stashed in `gallery_tile_aspect` and
    /// used later by `Demo::start_gallery_to_image` to contain-fit the
    /// *enlarged* view to the photo's true shape (portrait/landscape/
    /// square) — get this wrong and every enlarged photo comes out looking
    /// square regardless of the photo's real proportions. Also queues a
    /// crop for the tile's own square grid cell, applied once baking
    /// completes — see `Demo::advance_gallery_tile_crop`'s doc.
    pub fn set_gallery_tile_image(
        &mut self,
        proteus: &mut Proteus,
        idx: usize,
        bytes: Vec<u8>,
        aspect: Vec2,
    ) {
        let tile = self.gallery.tiles[idx];
        let _ = tile.set_image(proteus, Image::new(bytes));
        self.pending_gallery_tile_crop[idx] = true;
        self.gallery_tile_fetch_generation[idx] = self.gallery_fetch_generation;
        self.gallery_tile_aspect[idx] = aspect;
    }

    /// Injects a hires upgrade for the currently-enlarged tile — same
    /// `DemoApp`-does-the-I/O convention as [`Demo::set_gallery_tile_image`].
    /// Targets `gallery.hires_overlay`, not `gallery.enlarged` itself —
    /// see that field's doc for why: `enlarged`'s own low-res image must
    /// stay in place the whole time so there's always *something* to show,
    /// while the overlay bakes in the background and
    /// `Demo::advance_gallery_hires_overlay` crossfades it in once ready.
    /// Guarded by `idx` still matching the *currently* enlarged tile:
    /// [`Demo::take_pending_gallery_hires_cancel`] tells `DemoApp` to stop
    /// delivering results the moment `GalleryImage` is left, but a result
    /// that was already in flight at that exact instant could still land
    /// right after — this guard is what makes that race harmless rather
    /// than needing `DemoApp` to get the timing exactly right.
    pub fn set_gallery_hires_image(&mut self, proteus: &mut Proteus, idx: usize, bytes: Vec<u8>) {
        if self.state != AppState::GalleryImage(idx) {
            return;
        }
        // The one entity that wants a bigger cap than the rest of the grid —
        // only one hires image is ever resident, so it can afford a larger
        // footprint than the 12 simultaneous thumbnails. The renderer's bake
        // honours `Image::max_side`.
        let _ = self
            .gallery
            .hires_overlay
            .set_image(proteus, Image::new(bytes).with_max_side(900));
    }

    /// Resizes/repositions everything that depends on the viewport size but
    /// isn't already re-derived from it every tick — the background,
    /// `gallery`'s grid and Home's. (`nav`'s icons and `theme`'s sun/moon toggle read
    /// `self.viewport_size` fresh every tick, in `advance_nav_icons`/
    /// `advance_theme`, so they don't need a resize-time call here — see
    /// those methods' own doc.) Call on every resize (and once up front with
    /// the real initial size, since [`Demo::new`] only has a placeholder to
    /// spawn with). Logical pixels, same convention as
    /// [`proteus_sdk::Proteus::pointer_moved`].
    /// `state` with the theme's corner radius, `theme_progress` of the way
    /// from `light` to `dark`. Every geometry the demo declares or transitions
    /// to goes through it, so nothing ends a transition at the light theme's
    /// radius and snaps to the dark one after, or keeps resetting it.
    fn themed(&self, mut state: QuadState, light: f32, dark: f32) -> QuadState {
        state.corner_radius = themed_radius(light, dark, self.theme_progress);
        state
    }

    pub fn set_viewport_size(&mut self, proteus: &mut Proteus, size: Vec2) {
        self.viewport_size = size;
        for handle in [self.background.light, self.background.dark] {
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(handle.id()) {
                qs.size = size;
            }
        }
        for (tile, state) in self.gallery.tiles.into_iter().zip(gallery::layout(size)) {
            let state = self.themed(state, gallery::CORNER_RADIUS, gallery::CORNER_RADIUS_DARK);
            let _ = tile.set_declared_geometry(proteus, state);
        }
        self.layout_home(proteus);
    }

    /// Injects Splash's animated logo mark's pre-baked frames. `DemoApp`
    /// loads the 19 `frame-NN.png` files, bakes them into `main_atlas` with
    /// `Frame::bake_texture`, and passes the resulting handles here (see the
    /// crate doc). Call once, before the
    /// first `tick`; shows `frames[0]` immediately if non-empty, so the
    /// sequence always starts on frame 1.
    pub fn set_logo_frames(&mut self, proteus: &mut Proteus, frames: Vec<TextureHandle>) {
        self.logo_frames = frames;
        self.logo_frame_index = 0;
        self.logo_frame_elapsed = 0.0;
        if let Some(&first) = self.logo_frames.first() {
            let _ = self.splash.button.set_texture(proteus, first);
        }
    }

    /// Injects `loading.logo_dark`'s pre-baked frames (`frame-NN-dark.png`)
    /// — same `DemoApp`-does-the-I/O convention as [`Demo::set_logo_frames`],
    /// which this doesn't replace: Splash's own logo never theme-crossfades
    /// (see `screens::loading`'s module doc for why only this screen needs
    /// a dark set at all), so the two frame sets are entirely independent
    /// injections. Call once, before the first `tick`; harmless to call
    /// before or after `set_logo_frames` — order between the two doesn't
    /// matter.
    pub fn set_loading_logo_frames_dark(&mut self, frames: Vec<TextureHandle>) {
        self.loading_logo_frames_dark = frames;
    }

    /// Advance one frame.
    /// The demo's per-frame logic — every `advance_*` step. Runs as
    /// [`crate::DemoApp`]'s `App::update`, i.e. **after** `Proteus::tick` and
    /// before `Proteus::refresh_cascades` + render (the engine owns both of
    /// those now — this method does neither).
    pub fn advance(&mut self, proteus: &mut Proteus, dt: f32) {
        self.screen_change_left = (self.screen_change_left - dt).max(0.0);
        self.advance_intro(proteus, dt);
        self.advance_state(proteus, dt);
        self.advance_nav_click(proteus);
        self.advance_pending_reveals(proteus, dt);
        self.advance_pending_focus(proteus, dt);
        self.advance_pending_tile_reset(proteus);
        self.advance_logo_animation(proteus, dt);
        self.advance_loading_logo_animation(proteus, dt);
        self.advance_gallery_tile_crop(proteus);
        self.advance_gallery_fetch(proteus, dt);
        self.advance_gallery_button_fade(proteus, dt);
        self.advance_gallery_hires_overlay(proteus, dt);
        self.advance_example_animation(proteus, dt);
        self.advance_keyboard(proteus, dt);
        self.advance_nav_icons(proteus, dt);
        self.advance_home_titles(proteus, dt);
        self.advance_tile_hover(proteus, dt);
        self.advance_video_crossfade(proteus);
        self.advance_video_loading(proteus, dt);
        self.advance_theme(proteus, dt);
        // Must run after advance_theme — see this fn's own doc for why.
        self.advance_gallery_error_fade(proteus, dt);
        splash::recenter(proteus, &self.splash, self.intro_slide_offset);
        self.apply_example_detail_layout(proteus);
        self.apply_gallery_fetch_button_layout(proteus);
    }

    /// Intro fade (waits `splash::INTRO_DELAY_SECS`, then plays once,
    /// 0 → 1, never reverses) + slide-in, in lockstep. Burns off the delay
    /// first; any leftover `dt` in the same tick carries into the fade
    /// itself rather than being dropped (same pattern as `ActiveTransition`'s
    /// delay handling).
    fn advance_intro(&mut self, proteus: &mut Proteus, dt: f32) {
        let fade_dt = if self.intro_delay_remaining > 0.0 {
            let burned = dt.min(self.intro_delay_remaining);
            self.intro_delay_remaining -= burned;
            dt - burned
        } else {
            dt
        };
        self.intro_elapsed = (self.intro_elapsed + fade_dt).min(splash::INTRO_DURATION_SECS);
        let raw_t = self.intro_elapsed / splash::INTRO_DURATION_SECS;
        let alpha = Easing::EaseOutQuad.apply(raw_t);
        self.intro_slide_offset = splash::INTRO_SLIDE_DISTANCE_PX * (1.0 - alpha);

        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.splash.button.id())
        {
            qs.color.w = alpha;
        }
        if let Some(mut text) = proteus
            .world_mut()
            .get_mut::<Text>(self.splash.wordmark.id())
        {
            text.color.w = alpha;
        }
    }

    fn advance_state(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.state != AppState::Splash {
            return;
        }
        // The countdown only starts once the intro slide/fade has fully
        // settled — "1.5 seconds to register it" is measured from when the
        // composite is actually done animating in, not from when Splash was
        // spawned.
        if self.intro_elapsed < splash::INTRO_DURATION_SECS {
            return;
        }
        self.splash_elapsed += dt;
        if self.splash_elapsed < splash::HOLD_SECS {
            return;
        }

        // The splash logo splits into Home's grid, each tile from its own
        // part of the logo. The tiles' geometry follows the viewport: see
        // `Demo::layout_home`.
        self.layout_home(proteus);
        let button = self.splash.button;
        let targets = self.home.tiles;
        let _ = button.split_to(
            proteus,
            &targets,
            group_transition_config(),
            HOME_GRID_SPLIT,
        );
        self.queue_reveal(
            self.home.labels.to_vec(),
            group_transition_config().duration,
        );
        self.land_focus(
            proteus,
            Some(self.home.tiles[0]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// Processes whichever `NavClick` fired this tick (see `nav_click`'s
    /// doc) and drives the corresponding transition. The guard on each arm
    /// re-checks `self.state` at processing time, not just at the moment the
    /// click happened. `self.state` flips the instant a transition starts,
    /// so that alone would act on input that arrives while the transition
    /// is still under way, as though it had ended; input that changes
    /// screens is dropped until it has, by `screen_change_left`.
    fn advance_nav_click(&mut self, proteus: &mut Proteus) {
        let Some(click) = self.nav_click.take() else {
            return;
        };
        // A click or key that changes screens waits for the change under way
        // to end: `self.state` is already the screen it's going to, so acting
        // on it would start another change from components still moving,
        // such as the video tiles merging to Home mid-split. The theme and
        // the keyboard's flashes don't change screens.
        let changes_screen = !matches!(click, NavClick::SetTheme(_) | NavClick::FlashKey(_));
        if changes_screen && self.screen_change_left > 0.0 {
            return;
        }
        match click {
            NavClick::OpenVideoTiles if self.state == AppState::Home => {
                self.start_home_to_tiles(proteus);
            }
            NavClick::OpenGallery if self.state == AppState::Home => {
                self.start_home_to_loading(proteus);
            }
            NavClick::RefetchGallery if self.state == AppState::Gallery => {
                self.start_gallery_to_loading(proteus);
            }
            NavClick::OpenGalleryImage(idx) if self.state == AppState::Gallery => {
                self.start_gallery_to_image(proteus, idx);
            }
            NavClick::GoHome => match self.state {
                AppState::ExampleDetail(_) => self.start_detail_to_home(proteus),
                AppState::VideoTiles => self.start_tiles_to_home(proteus),
                AppState::VideoScreen(idx) => self.start_screen_to_home(proteus, idx),
                AppState::Loading => self.start_loading_to_home(proteus),
                AppState::Gallery => self.start_gallery_to_home(proteus),
                AppState::GalleryImage(_) => self.start_image_to_home(proteus),
                _ => {}
            },
            NavClick::OpenExampleDetail(idx) if self.state == AppState::Home => {
                self.start_home_to_detail(proteus, idx);
            }
            NavClick::OpenVideoScreen(idx) if self.state == AppState::VideoTiles => {
                self.start_tiles_to_screen(proteus, idx);
            }
            NavClick::Back => match self.state {
                AppState::ExampleDetail(_) => self.start_detail_to_home(proteus),
                AppState::VideoScreen(idx) => self.start_screen_to_tiles(proteus, idx),
                AppState::GalleryImage(_) => self.start_image_to_gallery(proteus),
                AppState::VideoTiles => self.start_tiles_to_home(proteus),
                AppState::Loading => self.start_loading_to_home(proteus),
                AppState::Gallery => self.start_gallery_to_home(proteus),
                _ => {}
            },
            NavClick::FlashKey(i)
                if self.state == AppState::ExampleDetail(example_detail::KEYS) =>
            {
                self.key_flash[i] = 1.0;
            }
            NavClick::SetTheme(dark) => {
                self.dark_target = dark;
                // The icon chosen stops taking clicks and focus, as the other
                // starts: focus moves across, so the keys keep their place.
                let (from, to) = if dark {
                    (self.theme.moon, self.theme.sun)
                } else {
                    (self.theme.sun, self.theme.moon)
                };
                if proteus.focused() == Some(from) {
                    self.land_focus(proteus, Some(to), 0.0);
                }
            }
            _ => {}
        }
    }

    /// Merges Home's grid into the three video tiles, by columns: the first
    /// column into the first tile, the middle two into the second, and the
    /// last into the third (see [`VIDEO_TILE_COLUMNS`]), so each tile grows
    /// from the part of the grid nearest it.
    ///
    /// Each video tile's resting state is set first, from
    /// `video_tiles::tile_target_state`: a tile's own declared geometry
    /// isn't guaranteed to carry its "white if real box art is baked"
    /// override, and a merge ends at its destination's declared geometry.
    fn start_home_to_tiles(&mut self, proteus: &mut Proteus) {
        for (i, &(start_col, width)) in VIDEO_TILE_COLUMNS.iter().enumerate() {
            let tile = self.video_tiles.tiles[i];
            let state = video_tiles::tile_target_state(proteus, tile, i, self.theme_progress);
            let _ = tile.set_declared_geometry(proteus, state);
            let sources: Vec<Handle> = home::column_group(start_col, width)
                .into_iter()
                .map(|idx| self.home.tiles[idx])
                .collect();
            let _ = tile.merge_from(
                proteus,
                &sources,
                group_transition_config(),
                MergeLayout::Grid {
                    cols: width,
                    rows: home::ROWS,
                },
            );
        }
        self.land_focus(
            proteus,
            Some(self.video_tiles.tiles[0]),
            group_transition_config().duration,
        );
        self.state = AppState::VideoTiles;
    }

    /// The reverse of `start_home_to_tiles`: each video tile splits back
    /// into its columns of Home's grid.
    fn start_tiles_to_home(&mut self, proteus: &mut Proteus) {
        for (i, &(start_col, width)) in VIDEO_TILE_COLUMNS.iter().enumerate() {
            let targets: Vec<Handle> = home::column_group(start_col, width)
                .into_iter()
                .map(|idx| self.home.tiles[idx])
                .collect();
            let _ = self.video_tiles.tiles[i].split_to(
                proteus,
                &targets,
                group_transition_config(),
                SplitStrategy::Grid {
                    cols: width,
                    rows: home::ROWS,
                },
            );
        }
        self.land_focus(
            proteus,
            Some(self.home.tiles[0]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// Grows the clicked tile into the video screen (a 1→1 transition, not a
    /// group transition — same shape as `Handle::animate_to`'s doc
    /// describes) and marks it to start showing video, crossfading in from
    /// the box-cover art in lockstep with the geometry transition
    /// (`Demo::advance_video_crossfade` owns the ramp itself — this just
    /// starts it at `0.0` instead of `show_video`'s own instant-cut
    /// default). Queues `idx` for `DemoApp` to actually start decoding
    /// (`take_pending_video_start`) — `video_t` starts at `0.0` (fully box
    /// art) regardless of how quickly `DemoApp` manages to actually get a
    /// real frame decoded — a brief black gap behind the fading-in art
    /// before the first frame lands is expected.
    fn start_tiles_to_screen(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        // At the theme's radius, so the tile doesn't snap to it on arriving.
        let mut target = video_tiles::video_screen_quad(self.viewport_size);
        target.corner_radius = themed_radius(
            video_tiles::SCREEN_CORNER_RADIUS,
            video_tiles::SCREEN_CORNER_RADIUS_DARK,
            self.theme_progress,
        );
        let _ = tile.animate_to(proteus, target, group_transition_config());
        let video = proteus.create_video();
        let _ = tile.show_video(proteus, &video);
        let _ = tile.set_video_crossfade(proteus, 0.0);
        self.video = Some(video);
        self.pending_video_start = Some(idx);
        // Fresh loading-UI state for this visit — see each field's own doc.
        self.video_first_frame_shown = false;
        self.video_dots_elapsed = 0.0;
        self.video_load_timed_out = false;
        self.pending_video_cancel = false;
        self.land_focus(
            proteus,
            Some(self.nav.back),
            group_transition_config().duration,
        );
        self.state = AppState::VideoScreen(idx);
    }

    /// One 1→3 `Row` split — the clicked (screen-sized) tile fans back
    /// out to all 3 grid slots, including its own.
    ///
    /// Uses `split_to_with_states`, not plain `split_to` — `tile` (the
    /// clicked one) is both the source *and* one of the 3 targets here, so
    /// its own `declared_geometry` at this exact moment is still
    /// screen-shaped (it hasn't been reset yet — see `PendingTileReset`'s
    /// doc); resolving its target state that way would bake this crossfade
    /// toward the wrong (screen, not grid) shape, and `set_declared_geometry`
    /// isn't safe to call first either — see `split_to_with_states`'s own
    /// doc for exactly why. Passing `video_tiles::tile_target_state`
    /// explicitly for all 3 sidesteps both problems, and — same helper
    /// `start_home_to_tiles` uses — also carries the "white if real box art
    /// is baked" override.
    fn start_screen_to_tiles(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        // Undo `advance_video_loading`'s "hide the tile's own art while
        // waiting" override *before* `split_to_with_states` bakes its own
        // "from" snapshot below — otherwise backing out of a still-loading
        // video would bake the tile's momentarily-invisible alpha into that
        // snapshot, and the whole outgoing transition back to the grid would show
        // nothing instead of fading back in. Unconditional (not gated on
        // `ready`): harmless if the tile was already fully visible.
        if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color.w = 1.0;
        }
        self.stop_tile_video(proteus, tile);
        self.restore_idle_tiles_opacity(proteus, idx);
        let targets: Vec<(Handle, QuadState)> = (0..3)
            .map(|i| {
                let t = self.video_tiles.tiles[i];
                let state = video_tiles::tile_target_state(proteus, t, i, self.theme_progress);
                (t, state)
            })
            .collect();
        let _ = tile.split_to_with_states(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Row,
        );
        // See `PendingTileReset`'s doc for why this can't happen
        // synchronously here.
        self.pending_tile_reset = Some(PendingTileReset { tile });
        self.land_focus(
            proteus,
            Some(self.video_tiles.tiles[idx]),
            group_transition_config().duration,
        );
        self.state = AppState::VideoTiles;
    }

    /// One 1→8 `Grid` split straight to Home's grid, skipping `VideoTiles`
    /// entirely — the `VideoScreen`-to-`Home` escape hatch, same shape as
    /// `start_detail_to_home`. The other two tiles
    /// never participate in this split (only the playing one does), so —
    /// unlike `start_screen_to_tiles`, where all 3 tiles are targets and
    /// get revealed by the split itself — they're hidden explicitly here;
    /// otherwise they'd be left visible at their old grid position, which
    /// would be wrong given nothing on `Home` should show any tile at all.
    /// Restores the opacity of the tiles `advance_video_crossfade` faded out
    /// while tile `idx` played, hiding them first. Must run before a split
    /// that targets them: the split bakes each target's end image at the
    /// target's current opacity, so a tile still faded out would bake as
    /// nothing, and its pieces would fade out instead of in. Hidden, they
    /// can't flash at full opacity before the split takes them over, and
    /// whatever reveals them next shows them restored.
    fn restore_idle_tiles_opacity(&self, proteus: &mut Proteus, idx: usize) {
        for (i, &other) in self.video_tiles.tiles.iter().enumerate() {
            if i != idx {
                let _ = other.set_visible(proteus, false);
                let _ = other.set_opacity(proteus, 1.0);
            }
        }
    }

    fn start_screen_to_home(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        // See `start_screen_to_tiles`'s identical restore for why this must
        // happen before `split_to` bakes its own "from" snapshot below.
        if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color.w = 1.0;
        }
        self.stop_tile_video(proteus, tile);
        self.restore_idle_tiles_opacity(proteus, idx);
        let targets = self.home.tiles;
        let _ = tile.split_to(
            proteus,
            &targets,
            group_transition_config(),
            HOME_GRID_SPLIT,
        );
        // `tile` isn't a target of *this* split (only `home.tiles`
        // are), so nothing reveals it here to expose its still-screen-sized
        // live geometry right now — but leaving it un-reset would resurface
        // the exact same staleness the next time `start_home_to_tiles` uses
        // this same tile as a target (whose reveal, again, only flips
        // `Visibility`, never resyncs geometry). See `PendingTileReset`'s
        // doc for why this can't happen synchronously here.
        self.pending_tile_reset = Some(PendingTileReset { tile });
        self.land_focus(
            proteus,
            Some(self.home.tiles[0]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// Resets everything a fresh gallery fetch needs: bumps the generation
    /// counter (so stale per-tile stamps from the previous round can't
    /// satisfy the auto-advance check — see `gallery_tile_fetch_generation`'s
    /// doc), clears the error/timeout state, restarts the logo's loop at
    /// frame 0 (mirrors `screens::splash`'s own "start on frame 1"
    /// convention — a fresh visit shouldn't resume mid-sequence from a
    /// stale previous visit), and queues the fetch request itself. Shared
    /// by `start_home_to_loading` and `start_gallery_to_loading` — the only
    /// two edges that begin a fetch.
    fn begin_gallery_fetch(&mut self, proteus: &mut Proteus) {
        self.gallery_fetch_generation = self.gallery_fetch_generation.wrapping_add(1);
        self.gallery_fetch_elapsed = 0.0;
        self.gallery_error_shown = false;
        self.gallery_logo_error_fade = 1.0;
        let _ = self.loading.error_text.set_visible(proteus, false);
        self.loading_logo_frame_index = 0;
        self.loading_logo_frame_elapsed = 0.0;
        self.apply_loading_logo_frame(proteus);
        let tile_side_px = gallery::layout(self.viewport_size)[0]
            .size
            .x
            .round()
            .max(1.0) as u32;
        self.pending_gallery_fetch = Some(GalleryFetchRequest { tile_side_px });
    }

    /// Pushes `loading_logo_frame_index`'s bake onto `loading.logo`/
    /// `loading.logo_dark` (whichever of the two has that frame loaded —
    /// each set is injected independently, see `Demo::set_loading_logo_
    /// frames_dark`'s doc). Factored out of `advance_loading_logo_animation`'s
    /// loop body so `Demo::begin_gallery_fetch` can call it once,
    /// immediately, to force frame 0 onto both layers the instant a fresh
    /// `Loading` visit begins — otherwise they'd keep showing whichever
    /// frame the *previous* visit last left them on until the animation's
    /// own timer first ticks past a full frame duration, which reads as the
    /// loop starting mid-sequence and jumping back to frame 0 a beat later.
    fn apply_loading_logo_frame(&mut self, proteus: &mut Proteus) {
        if let Some(&frame) = self.logo_frames.get(self.loading_logo_frame_index) {
            let _ = self.loading.logo.set_texture(proteus, frame);
        }
        if let Some(&frame) = self
            .loading_logo_frames_dark
            .get(self.loading_logo_frame_index)
        {
            let _ = self.loading.logo_dark.set_texture(proteus, frame);
        }
    }

    /// One 8→1 `Grid` merge — Home's whole grid converges onto
    /// `loading.logo`, then a fresh fetch begins.
    fn start_home_to_loading(&mut self, proteus: &mut Proteus) {
        self.begin_gallery_fetch(proteus);
        let sources = self.home.tiles;
        let _ = self.loading.logo.merge_from(
            proteus,
            &sources,
            group_transition_config(),
            HOME_GRID_MERGE,
        );
        self.land_focus(proteus, None, group_transition_config().duration);
        self.state = AppState::Loading;
    }

    /// One 1→8 `Grid` split back to Home's grid — the error escape hatch
    /// (going home while `Loading`, fetching or erroring) as well as the
    /// ordinary "Loading" → "Home" back-navigation.
    fn start_loading_to_home(&mut self, proteus: &mut Proteus) {
        let targets = self.home.tiles;
        let _ = self.loading.logo.split_to(
            proteus,
            &targets,
            group_transition_config(),
            HOME_GRID_SPLIT,
        );
        let _ = self.loading.error_text.set_visible(proteus, false);
        self.land_focus(
            proteus,
            Some(self.home.tiles[1]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// One 1→`gallery::TILE_COUNT` `Grid` split — `loading.logo` fans
    /// out into the grid once every tile's current-generation image has
    /// arrived (`advance_gallery_fetch`). `Grid` rather than
    /// `Row` (unlike `Home`↔`Loading`'s single-target merge/split) so each
    /// tile radiates from its own quadrant instead of zigzagging across one
    /// shared axis — see `proteus_ui::SplitStrategy::Grid`'s doc.
    /// `gallery.fetch_button`/`.fetch_button_label` aren't split targets
    /// (only the 12 tiles are) — their own fade-in is `Demo::advance_
    /// gallery_button_fade`'s job, not queued here at all: it derives
    /// "settled" from the tiles' own real visibility, which already tracks
    /// this split's actual completion more precisely than a fixed-duration
    /// guess would.
    fn start_loading_to_gallery(&mut self, proteus: &mut Proteus) {
        let targets = self.gallery.tiles;
        let _ = self.loading.logo.split_to(
            proteus,
            &targets,
            gallery_group_transition_config(),
            SplitStrategy::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        self.land_focus(
            proteus,
            Some(self.gallery.tiles[0]),
            gallery_group_transition_config().duration,
        );
        self.state = AppState::Gallery;
    }

    /// One `gallery::TILE_COUNT`→1 `Grid` merge — the reverse of
    /// `start_loading_to_gallery`, triggered by clicking "Fetch New
    /// Images". `fetch_button`/`.fetch_button_label` fade out on their own
    /// (`Demo::advance_gallery_button_fade`, the instant `self.state` stops
    /// being `Gallery` — which happens synchronously below) rather than
    /// hiding immediately here.
    fn start_gallery_to_loading(&mut self, proteus: &mut Proteus) {
        self.begin_gallery_fetch(proteus);
        let sources = self.gallery.tiles;
        let _ = self.loading.logo.merge_from(
            proteus,
            &sources,
            gallery_group_transition_config(),
            MergeLayout::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        self.land_focus(proteus, None, gallery_group_transition_config().duration);
        self.state = AppState::Loading;
    }

    /// The gallery's 4 by 3 grid becomes Home's 4 by 2, column by column: in
    /// each, the top photo becomes the top tile, and the other two merge
    /// into the bottom one, so every photo heads for the tile nearest it.
    fn start_gallery_to_home(&mut self, proteus: &mut Proteus) {
        for col in 0..home::COLS {
            let photo = |row: usize| self.gallery.tiles[row * gallery::COLS + col];
            let _ = photo(0).split_to(
                proteus,
                &[self.home.tiles[col]],
                gallery_group_transition_config(),
                SplitStrategy::Row,
            );
            let _ = self.home.tiles[home::COLS + col].merge_from(
                proteus,
                &[photo(1), photo(2)],
                gallery_group_transition_config(),
                MergeLayout::Grid { cols: 1, rows: 2 },
            );
        }
        // `fetch_button`/`.fetch_button_label` fade out on their own
        // (`Demo::advance_gallery_button_fade`) rather than hiding
        // immediately here — see `start_gallery_to_loading`'s doc for the
        // same call.
        self.land_focus(
            proteus,
            Some(self.home.tiles[1]),
            gallery_group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// One `gallery::TILE_COUNT`→1 `Grid` merge — all 12 tiles converge
    /// onto `gallery.enlarged` (a dedicated coordinator entity, not the
    /// clicked tile itself — see [`gallery::Gallery::enlarged`]'s doc for
    /// why). `enlarged`'s declared geometry and initial content are both
    /// set *before* the merge starts (same "set the real final resting
    /// state up front" ordering as `start_examples_to_detail`): the target
    /// size comes from `gallery::large_image_quad`, contain-fit using the
    /// photo's real aspect ratio (`gallery_tile_aspect[idx]` — deliberately
    /// *not* the tile's own baked image size, which is by now a
    /// center-cropped square, not the photo's real shape — see
    /// `Demo::set_gallery_tile_image`'s doc for why); the initial content
    /// is a direct copy of `gallery.tile_full[idx]`'s *uncropped* frame
    /// (`Handle::copy_baked_image_from`) — deliberately not the tile's own,
    /// by-then-cropped `BakedImage`, which would show the wrong (square)
    /// framing stretched into the enlarged view's real-aspect box.
    /// Without a copy at all, `enlarged` would be revealed showing nothing,
    /// since a merge's own completion only flips `Visibility`, never
    /// touches the destination's `BakedImage` (see `Handle::split_to`'s
    /// doc).
    ///
    /// Also queues a hires fetch for `DemoApp`, sized from `enlarged`'s own
    /// target geometry — the *larger* axis rounded to an integer once, the
    /// other axis then derived from *that* rounded integer via the exact
    /// aspect ratio (never rounded independently — see the inline comment
    /// below for why that one extra degree of freedom is enough to visibly
    /// shift the image the instant the hires crossfade completes). Both
    /// values are in the same logical-pixel units as `self.viewport_size`
    /// and *uncapped* — `Demo` has no notion of the display's actual pixel
    /// density (see [`Demo::set_viewport_size`]'s doc), so it can't decide
    /// how many *physical* pixels "sharp enough" means; scaling by the
    /// viewport's `scale_factor` and applying whatever physical-pixel cap
    /// bounds the network fetch happens above this crate, in
    /// `DemoApp::advance_gallery` — both applied after this same rounding,
    /// to the physical width/height, never to this method's logical
    /// `target_size`. `Demo` computes both axes
    /// itself, since it's the only side that actually knows the photo's
    /// real aspect ratio (`aspect`, above) — `DemoApp` just scales/caps
    /// and fetches, no aspect-ratio bookkeeping of its own needed.
    fn start_gallery_to_image(&mut self, proteus: &mut Proteus, idx: usize) {
        let aspect = self.gallery_tile_aspect[idx];
        let target = self.themed(
            gallery::large_image_quad(aspect, self.viewport_size),
            gallery::CORNER_RADIUS,
            gallery::CORNER_RADIUS_DARK,
        );
        let target_size = target.size;
        let _ = self.gallery.enlarged.set_declared_geometry(proteus, target);
        let _ = self
            .gallery
            .enlarged
            .copy_baked_image_from(proteus, self.gallery.tile_full[idx]);

        // Clear any bake/bytes left over from a *previous* visit's hires
        // fetch — without this, `advance_gallery_hires_overlay`'s
        // `has_bake` check would see the stale `BakedImage` and start
        // crossfading the wrong photo in immediately, before this visit's
        // own fetch has even started.
        let _ = self.gallery.hires_overlay.free_resources(proteus);
        let _ = self.gallery.hires_overlay.set_visible(proteus, false);
        self.gallery_hires_fade = 0.0;

        // `fetch_button`/`.fetch_button_label` fade out on their own
        // (`Demo::advance_gallery_button_fade`) rather than hiding
        // immediately here — see `start_gallery_to_loading`'s doc for the
        // same call.
        let sources = self.gallery.tiles;
        let _ = self.gallery.enlarged.merge_from(
            proteus,
            &sources,
            gallery_group_transition_config(),
            MergeLayout::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );

        // Round only the larger axis, then derive the other from *that*
        // already-rounded integer via the exact aspect ratio. Rounding both axes
        // independently lets the fetched image's actual aspect ratio drift slightly
        // from the box's exact one, which shows up as a small content shift the
        // instant the crossfade swaps the (stretched) low-res stand-in for the
        // (correctly-shaped) hires image. Same technique
        // `gallery_fetch::fetch_dimensions` uses for the low-res fetch.
        // Deliberately uncapped here — see this method's own doc for why
        // capping belongs to `DemoApp`.
        let (width_px, height_px) = if target_size.x >= target_size.y {
            let width_px = target_size.x.round().max(1.0);
            let height_px = (width_px * target_size.y / target_size.x).round().max(1.0);
            (width_px as u32, height_px as u32)
        } else {
            let height_px = target_size.y.round().max(1.0);
            let width_px = (height_px * target_size.x / target_size.y).round().max(1.0);
            (width_px as u32, height_px as u32)
        };
        self.pending_gallery_hires_fetch = Some(GalleryHiresFetchRequest {
            idx,
            width_px,
            height_px,
        });
        self.land_focus(
            proteus,
            Some(self.gallery.enlarged),
            gallery_group_transition_config().duration,
        );
        self.state = AppState::GalleryImage(idx);
    }

    /// One 1→`gallery::TILE_COUNT` `Grid` split — the reverse of
    /// `start_gallery_to_image`, triggered by clicking `gallery.enlarged`
    /// itself or `nav::Nav::back`. Cancels the hires fetch unconditionally
    /// (see `pending_gallery_hires_cancel`'s doc), whether or not one had
    /// actually landed yet.
    fn start_image_to_gallery(&mut self, proteus: &mut Proteus) {
        let enlarged_from = match self.state {
            AppState::GalleryImage(idx) => Some(idx),
            _ => None,
        };
        self.pending_gallery_hires_cancel = true;
        self.pending_gallery_hires_fetch = None;
        let _ = self.gallery.hires_overlay.set_visible(proteus, false);
        let targets = self.gallery.tiles;
        let _ = self.gallery.enlarged.split_to(
            proteus,
            &targets,
            gallery_group_transition_config(),
            SplitStrategy::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        // `fetch_button`/`.fetch_button_label` fade in on their own
        // (`Demo::advance_gallery_button_fade`) — see
        // `start_loading_to_gallery`'s doc for why this isn't queued here.
        self.land_focus(
            proteus,
            enlarged_from.map(|i| self.gallery.tiles[i]),
            gallery_group_transition_config().duration,
        );
        self.state = AppState::Gallery;
    }

    /// One 1→8 `Grid` split straight to Home's grid — the
    /// `GalleryImage`-to-`Home` escape hatch, skipping `Gallery`'s grid
    /// entirely, same shape as `start_detail_to_home`/`start_screen_to_home`.
    /// The 12 real tiles were never revealed in the first place (they're
    /// still hidden from `start_gallery_to_image`'s own merge — see
    /// `gallery::Gallery::enlarged`'s doc), so unlike those two there's
    /// nothing extra to hide here.
    fn start_image_to_home(&mut self, proteus: &mut Proteus) {
        self.pending_gallery_hires_cancel = true;
        self.pending_gallery_hires_fetch = None;
        let _ = self.gallery.hires_overlay.set_visible(proteus, false);
        let targets = self.home.tiles;
        let _ = self.gallery.enlarged.split_to(
            proteus,
            &targets,
            group_transition_config(),
            HOME_GRID_SPLIT,
        );
        self.land_focus(
            proteus,
            Some(self.home.tiles[1]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// One 8→1 `Grid` merge — Home's whole grid converges onto the shared
    /// `example_detail.panel`. The panel's target geometry is fixed
    /// *before* the merge starts — the real final resting state goes in up
    /// front, so the transition animates straight to the correct spot
    /// instead of snapping after.
    fn start_home_to_detail(&mut self, proteus: &mut Proteus, idx: usize) {
        let target = self.themed(
            example_detail::panel_target(idx, self.viewport_size),
            example_detail::CORNER_RADIUS,
            example_detail::CORNER_RADIUS_DARK,
        );
        let _ = self
            .example_detail
            .panel
            .set_declared_geometry(proteus, target);
        let sources = self.home.tiles;
        let _ = self.example_detail.panel.merge_from(
            proteus,
            &sources,
            group_transition_config(),
            HOME_GRID_MERGE,
        );

        self.queue_reveal(
            self.example_detail.content_handles(idx),
            group_transition_config().duration,
        );
        self.active_example_category = Some(idx);
        self.land_focus(
            proteus,
            Some(if idx == example_detail::KEYS {
                self.example_detail.keys.keyboard.space()
            } else {
                self.nav.back
            }),
            group_transition_config().duration,
        );
        self.state = AppState::ExampleDetail(idx);
    }

    /// One 1→8 `Grid` split — the reverse of `start_home_to_detail`, by Back
    /// or Home alike.
    fn start_detail_to_home(&mut self, proteus: &mut Proteus) {
        let came_from = self.active_example_category;
        let targets = self.home.tiles;
        let _ = self.example_detail.panel.split_to(
            proteus,
            &targets,
            group_transition_config(),
            HOME_GRID_SPLIT,
        );
        self.hide_active_example_content(proteus);
        self.land_focus(
            proteus,
            came_from.map(|idx| self.home.tiles[home::FIRST_EXAMPLE + idx]),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// Hides whichever category is currently showing and clears
    /// `active_example_category` — called on the way out of
    /// `ExampleDetail`, since (unlike `screens::home`'s labels) this
    /// content is standalone, not a
    /// child of anything the transition system itself hides/reveals — see
    /// `screens::example_detail`'s module doc.
    fn hide_active_example_content(&mut self, proteus: &mut Proteus) {
        self.set_keyboard_shifted(proteus, false);
        self.key_flash.fill(0.0);
        if let Some(idx) = self.active_example_category.take() {
            for handle in self.example_detail.content_handles(idx) {
                let _ = handle.set_visible(proteus, false);
            }
        }
    }

    /// Queues `entities` to become `Visibility::VISIBLE` once `duration`
    /// seconds have elapsed — see `PendingReveal`'s doc.
    /// Clears focus as a transition starts, and gives it to `target`, if
    /// any, once the transition ends, `after` seconds from now: see
    /// `Demo::advance_pending_focus`. Each screen's target is where the keys
    /// pick up from there — the tile Home was left from, the photo or video
    /// tile that was open, the back icon on a leaf screen, or nothing while
    /// loading — so the keys never start from a component that's gone.
    ///
    /// Every change of screen calls it as it starts, so it also notes when the
    /// change ends, which input that changes screens waits for.
    fn land_focus(&mut self, proteus: &mut Proteus, target: Option<Handle>, after: f32) {
        self.screen_change_left = after;
        proteus.clear_focus();
        self.pending_focus = target.map(|target| PendingFocus {
            target,
            wait: after,
        });
    }

    /// Gives focus to `pending_focus`'s target once its wait is over and it
    /// shows: a hidden component can't take focus, and a transition's
    /// targets show as it ends, so it tries each tick until it holds.
    fn advance_pending_focus(&mut self, proteus: &mut Proteus, dt: f32) {
        let Some(pending) = &mut self.pending_focus else {
            return;
        };
        pending.wait -= dt;
        if pending.wait > 0.0 {
            return;
        }
        let target = pending.target;
        if proteus.get(target).is_some_and(|d| d.visible) {
            let _ = target.focus(proteus);
        }
        if proteus.focused() == Some(target) {
            self.pending_focus = None;
        }
    }

    fn queue_reveal(&mut self, entities: Vec<Handle>, duration: f32) {
        self.pending_reveals.push(PendingReveal {
            elapsed: 0.0,
            duration,
            entities,
        });
    }

    fn advance_pending_reveals(&mut self, proteus: &mut Proteus, dt: f32) {
        let mut i = 0;
        while i < self.pending_reveals.len() {
            self.pending_reveals[i].elapsed += dt;
            if self.pending_reveals[i].elapsed >= self.pending_reveals[i].duration {
                let reveal = self.pending_reveals.remove(i);
                for e in reveal.entities {
                    let _ = e.set_visible(proteus, true);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Resets *every* tile back to its resting shape/appearance once
    /// `start_screen_to_tiles`/`start_screen_to_home`'s queued tile is
    /// actually observed hidden — proof `one_to_n_setup_system` has already
    /// run and captured its (still screen-sized) live geometry as the group
    /// transition's "from" state. See [`PendingTileReset`]'s doc for the
    /// full reasoning; this just implements the wait.
    ///
    /// Covers all 3 tiles, not just the one that was playing — a split's
    /// own reveal only flips `Visibility`, it never rewrites a target's
    /// live `QuadState` or `Glow` back to anything (same "reveal doesn't
    /// touch content" rule [`Handle::copy_baked_image_from`]'s doc already
    /// covers for `BakedImage`). Their opacity, faded out by
    /// `Demo::advance_video_crossfade`, is restored earlier, before the
    /// split: see `Demo::restore_idle_tiles_opacity`. Called for all 3 tiles from
    /// `settle(AppState::VideoTiles)`.
    ///
    /// `video_tiles::tile_target_state`'s own `color` is `tile_quad`'s
    /// placeholder tint unless real box-cover art is baked, in which case
    /// it's untinted opaque white — a bare `tile_quad(idx)` would discard
    /// that and tint the box art with the placeholder color.
    fn advance_pending_tile_reset(&mut self, proteus: &mut Proteus) {
        let Some(reset) = &self.pending_tile_reset else {
            return;
        };
        let hidden = !proteus.get(reset.tile).map(|d| d.visible).unwrap_or(true);
        if !hidden {
            return;
        }
        self.pending_tile_reset = None;
        for (i, tile) in self.video_tiles.tiles.into_iter().enumerate() {
            // The tile also rests here again: `start_tiles_to_screen` moved it
            // to the screen with `animate_to`, which made that where it rests.
            let state = video_tiles::tile_target_state(proteus, tile, i, self.theme_progress);
            let _ = tile.set_declared_geometry(proteus, state);
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(tile.id()) {
                glow.radius = 0.0;
            }
        }
    }

    /// Lays Home's grid out for the current viewport: where each tile rests
    /// (see `home::layout`). Called as the splash logo splits into the grid,
    /// and on every resize. A tile on Home moves to its new place at once;
    /// one away from Home, or still coming back, is hidden, and its next
    /// transition ends there.
    fn layout_home(&mut self, proteus: &mut Proteus) {
        for (tile, state) in self
            .home
            .tiles
            .into_iter()
            .zip(home::layout(self.viewport_size))
        {
            let state = self.themed(state, home::CORNER_RADIUS, home::CORNER_RADIUS_DARK);
            let _ = tile.set_declared_geometry(proteus, state);
        }
    }

    /// Fades Home's heading and tagline in while Home is the current or
    /// (mid-transition) destination state, and out otherwise, so they come
    /// and go with the grid. Runs every tick, like `advance_nav_icons`.
    fn advance_home_titles(&mut self, proteus: &mut Proteus, dt: f32) {
        let target = if self.state == AppState::Home {
            1.0
        } else {
            0.0
        };
        let step = dt / home::TITLE_FADE_SECS;
        if self.home_title_fade < target {
            self.home_title_fade = (self.home_title_fade + step).min(target);
        } else if self.home_title_fade > target {
            self.home_title_fade = (self.home_title_fade - step).max(target);
        }
        let (heading_y, tagline_y) = home::title_positions();
        for (handle, y) in [
            (self.home.heading, heading_y),
            (self.home.tagline, tagline_y),
        ] {
            let _ = handle.set_visible(proteus, self.home_title_fade > 0.0);
            let _ = handle.set_opacity(proteus, self.home_title_fade);
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(handle.id()) {
                qs.position.y = y;
            }
        }
    }

    /// Drives the Keyboard & Focus panel's virtual keyboard: a key the key
    /// map doesn't bind flashes its keycap, matched by where the key is, and
    /// Caps Lock toggles the keycaps' shifted values. Each flash fades over
    /// `keyboard::FLASH_SECS`, eased in and out, and the Caps Lock keycap
    /// stays tinted while the keyboard is shifted. Keys pressed on any other
    /// screen are dropped. Focus starts on the space bar, as
    /// `Demo::start_home_to_detail` lands it.
    fn advance_keyboard(&mut self, proteus: &mut Proteus, dt: f32) {
        let keys = std::mem::take(&mut *self.unbound_keys.borrow_mut());
        if self.state == AppState::ExampleDetail(example_detail::KEYS) {
            for key in keys {
                let keyboard = &self.example_detail.keys.keyboard;
                let Some(i) = keyboard.index_of(&key.code) else {
                    continue;
                };
                self.key_flash[i] = 1.0;
                if keyboard.is_caps_lock(i) {
                    self.set_keyboard_shifted(proteus, !self.keyboard_shifted);
                }
            }
        }
        let keyboard = &self.example_detail.keys.keyboard;
        for (i, &key) in keyboard.keys.iter().enumerate() {
            let flash = &mut self.key_flash[i];
            *flash = (*flash - dt / screens::keyboard::FLASH_SECS).max(0.0);
            let tinted = self.keyboard_shifted && keyboard.is_caps_lock(i);
            let fill = screens::keyboard::key_fill(Easing::EaseInOutQuad.apply(*flash), tinted);
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(key.id()) {
                qs.color = fill;
            }
        }
    }

    /// Shows the virtual keycaps' shifted values, or their own, by changing
    /// the labels of the keycaps whose labels differ.
    fn set_keyboard_shifted(&mut self, proteus: &mut Proteus, shifted: bool) {
        if self.keyboard_shifted == shifted {
            return;
        }
        self.keyboard_shifted = shifted;
        let keyboard = &self.example_detail.keys.keyboard;
        for (i, &key) in keyboard.keys.iter().enumerate() {
            if keyboard.shifts(i) {
                let text = screens::keyboard::label_text(keyboard.label(i, shifted));
                let _ = key.set_text(proteus, text);
            }
        }
    }

    /// Positions the active category's heading/rows every tick — cheap and
    /// idempotent (see `example_detail::layout_content`'s doc), safe
    /// unconditionally: the panel's own `QuadState` never changes once
    /// `start_examples_to_detail` sets it, so there's no live transition
    /// here to fight.
    fn apply_example_detail_layout(&mut self, proteus: &mut Proteus) {
        let Some(idx) = self.active_example_category else {
            return;
        };
        let Some(panel_qs) = proteus
            .world()
            .get::<QuadState>(self.example_detail.panel.id())
            .cloned()
        else {
            return;
        };
        example_detail::layout_content(proteus, &self.example_detail, idx, &panel_qs);
    }

    /// Advances the logo's frame-sweep animation while the button is idle
    /// (waiting for a click) — swaps which pre-baked frame's texture sits on
    /// `splash.button`, wrapping through `logo_frames` every
    /// `splash::LOGO_FRAME_DURATION` seconds. Stops once Splash has handed
    /// off to Home: the button is either mid-transition (its current frame gets
    /// baked into the split's snapshot, same as any other texture
    /// content) or already hidden, so there's nothing left to animate.
    fn advance_logo_animation(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Splash {
            return;
        }
        self.logo_frame_elapsed += dt;
        while self.logo_frame_elapsed >= splash::LOGO_FRAME_DURATION {
            self.logo_frame_elapsed -= splash::LOGO_FRAME_DURATION;
            self.logo_frame_index = (self.logo_frame_index + 1) % self.logo_frames.len();
            let frame = self.logo_frames[self.logo_frame_index];
            let _ = self.splash.button.set_texture(proteus, frame);
        }
    }

    /// Same frame-sweep mechanics as `advance_logo_animation`, but for
    /// `loading.logo` — a separate index/elapsed pair (see that field's
    /// doc) since this one loops forever while `Loading` is active, rather
    /// than playing once.
    fn advance_loading_logo_animation(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Loading {
            return;
        }
        self.loading_logo_frame_elapsed += dt;
        while self.loading_logo_frame_elapsed >= splash::LOGO_FRAME_DURATION {
            self.loading_logo_frame_elapsed -= splash::LOGO_FRAME_DURATION;
            self.loading_logo_frame_index =
                (self.loading_logo_frame_index + 1) % self.logo_frames.len();
            self.apply_loading_logo_frame(proteus);
        }
    }

    /// For every tile `Demo::set_gallery_tile_image` queued
    /// (`pending_gallery_tile_crop`), checks whether its bake has landed
    /// yet (`Handle::baked_image_size`) — baking is the renderer's own
    /// per-frame job, so this can lag an arbitrary number of ticks behind
    /// the `set_gallery_tile_image` call. Once it has: stashes the
    /// still-uncropped frame onto `gallery.tile_full[idx]`
    /// (`Handle::copy_baked_image_from`), *then* center-crops the tile's
    /// own copy to a square in place (`Handle::crop_image`) —
    /// in that order, since the crop mutates the tile's `BakedImage`
    /// in place and would otherwise poison what gets stashed. Called every
    /// tick, before `advance_gallery_fetch` — so by the moment that
    /// function's "every tile baked" check can ever pass and advance to
    /// `Gallery`, every tile that just finished baking this same tick has
    /// already been stashed/cropped too, not left to catch up next tick.
    /// Split out from the bake pass itself, since baking stays a `Frame`
    /// concern (`DemoApp`) rather than this crate's.
    fn advance_gallery_tile_crop(&mut self, proteus: &mut Proteus) {
        for idx in 0..gallery::TILE_COUNT {
            if !self.pending_gallery_tile_crop[idx] {
                continue;
            }
            let tile = self.gallery.tiles[idx];
            if tile.baked_image_size(proteus).is_none() {
                continue;
            }
            let _ = self.gallery.tile_full[idx].copy_baked_image_from(proteus, tile);
            let _ = tile.crop_image(proteus, ImageCrop::CenteredSquare);
            self.pending_gallery_tile_crop[idx] = false;
        }
    }

    /// Drives `Loading`'s resting-state logic: the minimum-dwell/timeout
    /// clock, the error path, and the auto-advance to `Gallery`. Only ticks
    /// once `loading.logo` is actually visible — i.e. the incoming
    /// `Home`/`Gallery` → `Loading` merge has fully settled, not just
    /// started (same "observe the flag the transition system itself sets"
    /// technique as `advance_pending_tile_reset`) — so the ~1.7s
    /// minimum-dwell and 10s timeout both measure from when the spinner
    /// actually appears, not from when the click happened. The timeout/error
    /// half and the `Loading`-arm auto-advance check are one function here,
    /// since this crate has no separate settle-tick/drive-tick
    /// split.
    fn advance_gallery_fetch(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.state != AppState::Loading || self.gallery_error_shown {
            return;
        }
        let settled = proteus
            .get(self.loading.logo)
            .map(|d| d.visible)
            .unwrap_or(false);
        if !settled {
            return;
        }
        self.gallery_fetch_elapsed += dt;
        if self.gallery_fetch_elapsed >= GALLERY_FETCH_TIMEOUT_SECS {
            self.gallery_error_shown = true;
            let _ = self.loading.error_text.set_visible(proteus, true);
            return;
        }
        let min_dwell = self.logo_frames.len() as f32 * splash::LOGO_FRAME_DURATION;
        if self.gallery_fetch_elapsed < min_dwell {
            return;
        }
        let all_current = (0..gallery::TILE_COUNT).all(|i| {
            self.gallery_tile_fetch_generation[i] == self.gallery_fetch_generation
                && self.gallery.tiles[i].baked_image_size(proteus).is_some()
        });
        if all_current {
            self.start_loading_to_gallery(proteus);
        }
    }

    /// Fades `gallery.fetch_button`/`.fetch_button_label` in once fully
    /// settled in `Gallery` (i.e. after whichever incoming grid transition
    /// revealed the tiles has actually completed — not just started), and
    /// out the instant a `Gallery`→elsewhere transition begins. Paced by
    /// `gallery_group_transition_config()`'s own duration so it lands at
    /// `0`/`1` right as that transition completes/starts. Without this the
    /// button would pop in and out. "Settled" is read from the tiles' own real
    /// `Visibility` rather than a fixed-duration timer: group transitions
    /// never write a target's own `QuadState`/`Visibility` until the whole
    /// group actually completes (see `Handle::split_to`'s doc), so this
    /// tracks the *real* completion, not a guess that happens to usually
    /// match it. This state machine has no crate-wide "transitioning" flag,
    /// so this uses a per-entity check instead, as `Demo::advance_tile_hover`
    /// does.
    fn advance_gallery_button_fade(&mut self, proteus: &mut Proteus, dt: f32) {
        let settled = self.state == AppState::Gallery
            && self
                .gallery
                .tiles
                .iter()
                .all(|&tile| proteus.get(tile).map(|d| d.visible).unwrap_or(false));
        let target = if settled { 1.0 } else { 0.0 };
        let step = dt / gallery_group_transition_config().duration;
        if self.gallery_button_fade < target {
            self.gallery_button_fade = (self.gallery_button_fade + step).min(target);
        } else if self.gallery_button_fade > target {
            self.gallery_button_fade = (self.gallery_button_fade - step).max(target);
        }
        let fade = self.gallery_button_fade;
        let _ = self.gallery.fetch_button.set_visible(proteus, fade > 0.0);
        // Fades the border, the glow and the label, which is a child.
        let _ = self.gallery.fetch_button.set_opacity(proteus, fade);
    }

    /// Fades `loading.logo`/`loading.logo_dark` out once
    /// `gallery_error_shown` latches — the spinner otherwise keeps looping
    /// forever behind the error text, which now sits dead center where the
    /// logo would otherwise show through. Must run *after* `advance_theme`
    /// (see `Demo::tick`'s own call order) so it has the final say on
    /// `loading.logo_dark`'s alpha this frame — combining with (rather than
    /// being immediately overwritten by) `advance_theme`'s own
    /// `theme_progress` crossfade on that same layer; `loading.logo`'s own
    /// alpha has no other owner, so this is a plain overwrite there. Reset
    /// to fully visible (`1.0`) on every fresh visit by `Demo::begin_
    /// gallery_fetch`, same convention as `gallery_error_shown` itself.
    fn advance_gallery_error_fade(&mut self, proteus: &mut Proteus, dt: f32) {
        let target = if self.gallery_error_shown { 0.0 } else { 1.0 };
        // Deliberately the same constant `screens::theme`'s own sun/moon
        // fade uses — one nav-icon fade duration, reused here rather than
        // given a second name.
        let step = dt / theme::FADE_DURATION_SECS;
        if self.gallery_logo_error_fade < target {
            self.gallery_logo_error_fade = (self.gallery_logo_error_fade + step).min(target);
        } else if self.gallery_logo_error_fade > target {
            self.gallery_logo_error_fade = (self.gallery_logo_error_fade - step).max(target);
        }
        let fade = self.gallery_logo_error_fade;
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.loading.logo.id())
        {
            qs.color.w = fade;
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.loading.logo_dark.id())
        {
            qs.color.w = self.theme_progress * fade;
        }
    }

    /// Keeps `gallery.hires_overlay` glued to `gallery.enlarged`'s current
    /// geometry (position/size/scale/corner_radius — including mid-transition
    /// values, since `enlarged` is still animating during the
    /// `Gallery↔GalleryImage` transition) and crossfades its alpha in once
    /// two things are both true: it has a real baked image (the hires fetch
    /// landed and the renderer's bake pass picked it up —
    /// `Handle::baked_image_size`), and `enlarged` has fully settled
    /// (`visible`, not mid-transition). Gating on *both* — not just "has a
    /// bake" — matters because the hires fetch routinely finishes well
    /// before the ~0.6s grid transition does; crossfading in mid-transition would
    /// read as the sharp image popping in ahead of the tile finishing its
    /// own growth, so the low-res stand-in holds until the transition settles,
    /// only *then* crossfades. `gallery_hires_fade` only ramps up, never
    /// down, within one visit — `start_gallery_to_image` resets it to
    /// `0.0` for the next one. A no-op whenever `self.state` isn't
    /// `GalleryImage` at all.
    ///
    /// Deliberately does *not* re-derive `enlarged`'s box from the hires
    /// bake's decoded pixel size. The low-res and hires fetches round the
    /// photo's real aspect ratio at very different resolutions, and picsum
    /// center-crops each fetch to the size it was asked for, so the two can
    /// land on very slightly different crops. Resizing the box when the hires
    /// bake lands would then be a visible one- or two-pixel jump, just as the
    /// user is looking at the image. Instead the box is set once, in
    /// `start_gallery_to_image`, from `gallery_tile_aspect[idx]` (the photo's
    /// real aspect, known before either fetch happens), and both images
    /// stretch into it: a crop disagreement becomes an imperceptible
    /// sub-pixel stretch of the content within a still box.
    fn advance_gallery_hires_overlay(&mut self, proteus: &mut Proteus, dt: f32) {
        if !matches!(self.state, AppState::GalleryImage(_)) {
            return;
        }
        let Some(base) = proteus.get(self.gallery.enlarged) else {
            return;
        };
        let has_bake = self
            .gallery
            .hires_overlay
            .baked_image_size(proteus)
            .is_some();
        let target = if has_bake && base.visible { 1.0 } else { 0.0 };
        let step = dt / GALLERY_HIRES_CROSSFADE_DURATION_SECS;
        if self.gallery_hires_fade < target {
            self.gallery_hires_fade = (self.gallery_hires_fade + step).min(target);
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.gallery.hires_overlay.id())
        {
            qs.position.x = base.geometry.position.x;
            qs.position.y = base.geometry.position.y;
            qs.size = base.geometry.size;
            qs.scale = base.geometry.scale;
            qs.corner_radius = base.geometry.corner_radius;
            qs.color.w = self.gallery_hires_fade;
        }
        let _ = self
            .gallery
            .hires_overlay
            .set_visible(proteus, has_bake && base.visible);
    }

    /// Finalizes `gallery.fetch_button`'s geometry once its label has
    /// baked — see `screens::gallery::fetch_button_quad`'s doc. Checked every
    /// tick, but set only when it changes: `set_declared_geometry` moves the
    /// button at once and ends a hover or focus style's change under way, so
    /// setting it every tick would make the button's style snap rather than
    /// grow, unlike the photos'. The button is never itself a
    /// group-transition target (only the 12 tiles are — see
    /// `start_loading_to_gallery`'s doc), so there's no live transition on
    /// this entity to fight.
    fn apply_gallery_fetch_button_layout(&mut self, proteus: &mut Proteus) {
        if let Some(qs) = gallery::fetch_button_quad(proteus, &self.gallery, self.viewport_size) {
            // At the theme's radius: the theme blends it every tick, so at the
            // light radius this would undo the blend.
            let qs = self.themed(
                qs,
                gallery::FETCH_BUTTON_CORNER_RADIUS,
                gallery::FETCH_BUTTON_CORNER_RADIUS_DARK,
            );
            let button = self.gallery.fetch_button;
            if button.declared_geometry(proteus).as_ref() != Some(&qs) {
                let _ = button.set_declared_geometry(proteus, qs);
            }
        }
    }

    /// Drives `example_detail`'s "Continuous Animation" box while
    /// `ExampleDetail(2)` is active — paused (not reset) otherwise, so it
    /// resumes from wherever it left off rather than restarting each time.
    fn advance_example_animation(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.state != AppState::ExampleDetail(example_detail::TRANSFORMS) {
            return;
        }
        self.transforms_anim_elapsed += dt;
        example_detail::advance_continuous_animation(
            proteus,
            &self.example_detail,
            self.transforms_anim_elapsed,
        );
    }

    /// Drains this tick's pending gallery fetch request, if any — see
    /// [`GalleryFetchRequest`]'s doc.
    pub fn take_pending_gallery_fetch(&mut self) -> Option<GalleryFetchRequest> {
        self.pending_gallery_fetch.take()
    }

    /// Drains this tick's pending hires fetch request, if any — see
    /// [`GalleryHiresFetchRequest`]'s doc.
    pub fn take_pending_gallery_hires_fetch(&mut self) -> Option<GalleryHiresFetchRequest> {
        self.pending_gallery_hires_fetch.take()
    }

    /// Drains this tick's pending hires-fetch cancellation, if any — `true`
    /// means `DemoApp` should stop delivering results for whatever hires
    /// fetch it currently has in flight. See `pending_gallery_hires_cancel`'s
    /// doc.
    pub fn take_pending_gallery_hires_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_gallery_hires_cancel)
    }

    /// Drains this tick's pending video-start request, if any — `Some(idx)`
    /// means `DemoApp` should probe/decode whichever file index `idx`
    /// (0/1/2, matching `screens::video_tiles`' left/center/right tiles)
    /// maps to and start pushing frames at the video texture (see the
    /// crate doc: `Demo` leaves decoding to `DemoApp`). The entity
    /// already shows the video texture by the time this fires — see
    /// `pending_video_start`'s doc.
    pub fn take_pending_video_start(&mut self) -> Option<usize> {
        self.pending_video_start.take()
    }

    /// The video the playing tile shows, while one plays, for `DemoApp` to
    /// upload the player's frames to.
    pub fn video(&self) -> Option<VideoHandle> {
        self.video
    }

    /// Stops showing video on `tile`, releases the video, and asks `DemoApp`
    /// to stop the player.
    fn stop_tile_video(&mut self, proteus: &mut Proteus, tile: Handle) {
        let _ = tile.hide_video(proteus);
        if let Some(video) = self.video.take() {
            video.release(proteus);
        }
        self.pending_video_stop = true;
    }

    /// Drains this tick's pending video-stop request — `true` means
    /// `DemoApp` should stop the player. The video itself is already
    /// released.
    pub fn take_pending_video_stop(&mut self) -> bool {
        std::mem::take(&mut self.pending_video_stop)
    }

    /// Tells `Demo` a real decoded video frame has actually landed for the
    /// currently-playing tile — `DemoApp`'s own job is just detecting that
    /// (`VideoHandle::upload_frame` returning `true`) and calling this once;
    /// `Demo` has no way to see the GPU texture itself. Drives
    /// `Demo::advance_video_loading`'s loading-dots/error visibility.
    /// A no-op call (e.g. after the tile has already moved on) is harmless —
    /// this just sets a flag `start_tiles_to_screen` resets on the next
    /// visit anyway.
    pub fn set_video_first_frame_shown(&mut self) {
        self.video_first_frame_shown = true;
    }

    /// Drains this tick's pending video-cancel signal — `true` means a load
    /// just timed out and `DemoApp` should abort whatever fetch/decode is
    /// still in flight for it, *without* stopping/tearing down playback the
    /// way [`Demo::take_pending_video_stop`] means: `Demo` hasn't given up
    /// on this tile, it's just showing the error text now instead of
    /// pulsing dots, and playback may yet succeed if a response is close.
    /// See `pending_video_cancel`'s own doc for why this exists (a native
    /// decode has nothing to abort; the web shell's HLS fetch does).
    pub fn take_pending_video_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_video_cancel)
    }

    /// Drives `video_tiles.tile_overlays`/`tile_labels` — the black tint
    /// and title label shown while a tile is hovered with the pointer or
    /// focused with the keys, whichever the input in use shows, as its
    /// interaction style does — plus the screen-scale bump once resting as
    /// the video screen. The tile's own glow and growth are its interaction
    /// style. Each tile's overlay ramps over `screens::styles::SECS`, and
    /// stays down while the tile transitions: `tiles[idx]` genuinely has a
    /// transition during `start_tiles_to_screen`'s 1:1 `animate_to`. Also
    /// makes the settled, playing screen tile non-interactive — clicking it
    /// is a no-op, so there's nothing to invite feedback for (same `Handle::
    /// set_interactive` mutual-exclusion pattern `nav.home` already uses).
    fn advance_tile_hover(&mut self, proteus: &mut Proteus, dt: f32) {
        let pointing = proteus.input_mode() == InputMode::Pointing;
        let screen_focus_idx = match self.state {
            AppState::VideoScreen(idx) => Some(idx),
            _ => None,
        };
        for i in 0..3 {
            let tile = self.video_tiles.tiles[i];
            let _ = tile.set_interactive(proteus, screen_focus_idx != Some(i));

            let transitioning = proteus.get(tile).is_none_or(|d| d.transition.is_some());
            let light = &mut self.tile_lights[i];
            let lit = if pointing {
                light.hovered.get()
            } else {
                light.focused.get()
            };
            let target = if lit && !transitioning { 1.0 } else { 0.0 };
            let step = dt / screens::styles::SECS;
            light.progress = if light.progress < target {
                (light.progress + step).min(target)
            } else {
                (light.progress - step).max(target)
            };
            let progress = light.progress;

            // Recomputed every tick from the tile's own *current* geometry —
            // tile-shaped in grid view, the screen's very different
            // proportions once settled, anything in between mid-transition.
            let tile_geometry = proteus
                .get(tile)
                .map(|d| (d.geometry.size, d.geometry.corner_radius));
            let overlay = self.video_tiles.tile_overlays[i];
            if let Some(mut overlay_qs) = proteus.world_mut().get_mut::<QuadState>(overlay.id()) {
                if let Some((size, corner_radius)) = tile_geometry {
                    overlay_qs.size =
                        (size - Vec2::splat(2.0 * video_tiles::BORDER_WIDTH)).max(Vec2::ZERO);
                    overlay_qs.corner_radius = (corner_radius - video_tiles::BORDER_WIDTH).max(0.0);
                }
                overlay_qs.color.w = progress * video_tiles::TILE_OVERLAY_MAX_ALPHA;
            }

            let label = self.video_tiles.tile_labels[i];
            if let Some(mut text) = proteus.world_mut().get_mut::<Text>(label.id()) {
                text.color.w = progress;
            }
            // On the tile becoming the screen, the label grows with the tile
            // rather than jumping to its screen scale on the click.
            let label_scale = match (screen_focus_idx == Some(i), tile_geometry) {
                (true, Some((size, _))) => (size.x / video_tiles::TILE_WIDTH)
                    .clamp(1.0, video_tiles::TILE_LABEL_SCREEN_SCALE),
                _ => 1.0,
            };
            if let Some(mut label_qs) = proteus.world_mut().get_mut::<QuadState>(label.id()) {
                label_qs.scale = label_scale;
            }
        }
    }

    /// Ramps the currently-playing tile's `VideoCrossfade.video_t` from
    /// `0.0` (box-cover poster art) to `1.0` (live video) in lockstep with
    /// `start_tiles_to_screen`'s own geometry transition — eased the same way
    /// (`Easing::EaseInOutQuad`, matching `group_transition_config()`'s own
    /// choice, since `TransitionData` doesn't expose which easing fn is
    /// actually driving it), so both read as one motion instead of two
    /// separate effects. Once the transition settles (`proteus.get(tile)
    /// .transition` goes `None`), forces `video_t` to `1.0` outright;
    /// `Demo::advance_video_loading` takes over from there for a video whose
    /// first frame hasn't arrived yet.
    ///
    /// While actually mid-transition (not yet settled), this also does two
    /// more things, so the other tiles don't show over the transitioning one:
    /// - Fades the clicked tile's *own* alpha toward `0.0` in lockstep with
    ///   `video_t` (`1.0 - eased_t`, gated on `!ready` — a fast decode that's
    ///   already showing a real frame before the transition even finishes must
    ///   *not* have this fade it back out, only to have `advance_video_
    ///   loading` snap it back to `1.0` the instant the transition
    ///   settles, a one-frame flicker). This is what reveals
    ///   `advance_video_loading`'s `backdrop` *during* the transition, not just
    ///   once settled; without it, the tile would stay fully opaque (showing
    ///   whatever `VideoCrossfade` blended, poster art or a real frame) for the
    ///   entire transition and snap transparent the instant it settled,
    ///   reading as an abrupt pop rather than a dissolve.
    /// - Fades the *other two* tiles out with `set_opacity` (and forces their
    ///   `Glow` radius to `0`), over *half* the transition's own duration (`fade_t` reaches `1.0` at
    ///   `raw_t == 0.5`) — without this, the two untouched tiles just sit
    ///   there fully opaque for the whole transition. Once the growing/settled
    ///   screen's own opacity is *also* fading toward `0.0` (the point
    ///   above), z-order between it and the idle siblings stops being
    ///   enough to hide them on its own — `advance_video_loading`'s
    ///   `backdrop` is deliberately z-ordered to still occlude these two
    ///   once faded (see that entity's own doc), but only for the *tracked*
    ///   tile's footprint; fading the siblings themselves is still needed
    ///   so they don't just sit there fully visible next to/behind it.
    ///   `advance_tile_hover` already zeroes these tiles' hover-or-focus
    ///   decorations (overlay/label) whenever they're neither hovered nor
    ///   focused, which is always true here (the mouse or focus is on the
    ///   *clicked* tile) — this only needs to additionally fade each tile's
    ///   own base appearance.
    ///
    /// Only the *forward* direction gets any of this: `start_screen_to_
    /// tiles`'s reverse transition is a baked-slice crossfade (a frozen snapshot
    /// up front, and a fresh full-opacity target state for all 3 tiles —
    /// see `video_tiles::tile_target_state`'s doc), so there's no *live*
    /// content to fade in the first place. Called every tick, unconditionally —
    /// a no-op outside `VideoScreen` (nothing has `VideoCrossfade` then) and a
    /// no-op on `Handle::set_video_crossfade`'s own end once `hide_video` has
    /// removed it.
    fn advance_video_crossfade(&mut self, proteus: &mut Proteus) {
        let AppState::VideoScreen(idx) = self.state else {
            return;
        };
        let tile = self.video_tiles.tiles[idx];
        let raw_t = proteus
            .get(tile)
            .and_then(|d| d.transition)
            .map(|t| t.progress);
        let _ = tile.set_video_crossfade(
            proteus,
            raw_t.map(|t| Easing::EaseInOutQuad.apply(t)).unwrap_or(1.0),
        );

        let Some(raw_t) = raw_t else {
            return;
        };
        let eased_t = Easing::EaseInOutQuad.apply(raw_t);

        if !self.video_first_frame_shown {
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
                qs.color.w = 1.0 - eased_t;
            }
        }

        let fade_t = (raw_t * 2.0).min(1.0);
        let fade_alpha = 1.0 - Easing::EaseOutQuad.apply(fade_t);
        for (i, &other) in self.video_tiles.tiles.iter().enumerate() {
            if i == idx {
                continue;
            }
            let _ = other.set_opacity(proteus, fade_alpha);
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(other.id()) {
                glow.radius = 0.0;
            }
        }
    }

    /// Video screen loading UI: a black backdrop tracking the tile's live
    /// geometry (so the screen never looks broken mid-transition, before the
    /// first real frame even lands), 3 phase-staggered pulsing dots once
    /// settled and still waiting, replaced by inline error text after
    /// `video_tiles::VIDEO_LOAD_TIMEOUT_SECS`. Runs unconditionally, every
    /// tick, same as source — every branch below is gated on locally
    /// computed booleans rather than an early return, so a click away from
    /// `VideoScreen` mid-wait still correctly hides everything on the very
    /// next tick. "Mid-transition" vs. "settled" is read off the tile's own
    /// `ActiveTransition` component (`d.transition`, the same technique
    /// `advance_video_crossfade` above uses) rather than a crate-wide
    /// transition flag — `self.state` flips to `VideoScreen(idx)`
    /// immediately rather than waiting for the transition to settle (see
    /// `start_tiles_to_screen`'s doc), so the per-entity check is what
    /// actually distinguishes the two.
    fn advance_video_loading(&mut self, proteus: &mut Proteus, dt: f32) {
        let in_video_screen = matches!(self.state, AppState::VideoScreen(_));
        let video_idx = match self.state {
            AppState::VideoScreen(idx) => Some(idx),
            _ => None,
        };

        let mid_morph = video_idx.is_some_and(|idx| {
            proteus
                .get(self.video_tiles.tiles[idx])
                .and_then(|d| d.transition)
                .is_some()
        });

        // Only once the tile has risen above the idle ones. On the click
        // frame it is still at `TILE_Z`, where the backdrop's midpoint z
        // would tie with it and draw over it: a one-frame black flash.
        let backdrop_visible = video_idx.is_some_and(|idx| {
            proteus
                .get(self.video_tiles.tiles[idx])
                .is_some_and(|d| d.geometry.position.z > video_tiles::TILE_Z)
        });
        let _ = self
            .video_tiles
            .backdrop
            .set_visible(proteus, backdrop_visible);
        if let Some(idx) = video_idx {
            if let Some(tile_state) = proteus.get(self.video_tiles.tiles[idx]) {
                if let Some(mut qs) = proteus
                    .world_mut()
                    .get_mut::<QuadState>(self.video_tiles.backdrop.id())
                {
                    qs.position.x = tile_state.geometry.position.x;
                    qs.position.y = tile_state.geometry.position.y;
                    // Dynamic, not fixed — see `backdrop_quad`'s own doc
                    // for why: always the midpoint between the idle tiles'
                    // z and this tile's own *current* z, so it stays
                    // strictly behind the tracked tile (whatever point in
                    // the transition it's currently at) and strictly above the
                    // untouched idle siblings, throughout the entire transition
                    // and once settled alike.
                    qs.position.z = (video_tiles::TILE_Z + tile_state.geometry.position.z) / 2.0;
                    qs.size = tile_state.geometry.size;
                    qs.scale = tile_state.geometry.scale;
                    qs.corner_radius = tile_state.geometry.corner_radius;
                }
            }
        }

        // Set by `DemoApp` once `Frame::poll_video` reports a real frame.
        let ready = self.video_first_frame_shown;
        let settled_waiting = !mid_morph && in_video_screen && !ready;

        // Hide the tile's own art (its poster `BakedImage`, still showing
        // through `VideoCrossfade` at whatever `t` `advance_video_crossfade`
        // left it at) once settled, for as long as we're waiting —
        // otherwise it renders in front of `backdrop` (z 0.505 <
        // `video_tiles.tiles`' own settled 0.51), defeating the whole
        // black-fallback/dots design the instant loading is slow enough for
        // the gap to actually show. Restored the instant `ready` flips
        // true — same z, same geometry, just the real video showing
        // through again. `hide_video`'s own callers already restore this
        // unconditionally too, for the "user backs out before ready" case
        // this alone doesn't cover.
        if let Some(idx) = video_idx {
            if !mid_morph {
                if let Some(mut qs) = proteus
                    .world_mut()
                    .get_mut::<QuadState>(self.video_tiles.tiles[idx].id())
                {
                    qs.color.w = if ready { 1.0 } else { 0.0 };
                }
            }
        }

        self.video_dots_elapsed += dt;
        if settled_waiting {
            self.video_settled_elapsed += dt;
        } else {
            self.video_settled_elapsed = 0.0;
        }
        if settled_waiting
            && !self.video_load_timed_out
            && self.video_dots_elapsed >= video_tiles::VIDEO_LOAD_TIMEOUT_SECS
        {
            self.video_load_timed_out = true;
            self.pending_video_cancel = true;
        }
        // Delayed by `VIDEO_DOT_SHOW_DELAY_SECS` from when we *first*
        // became settled-and-waiting (not from click time, unlike
        // `video_dots_elapsed`/the timeout above) — playback often becomes
        // ready within a beat of settling, and showing the dots
        // immediately in that case reads as a flash right as the video
        // appears rather than an actual loading indicator.
        let dots_visible = settled_waiting
            && !self.video_load_timed_out
            && self.video_settled_elapsed >= video_tiles::VIDEO_DOT_SHOW_DELAY_SECS;
        let error_visible = settled_waiting && self.video_load_timed_out;
        for (i, &dot) in self.video_tiles.loading_dots.iter().enumerate() {
            let _ = dot.set_visible(proteus, dots_visible);
            if dots_visible {
                let phase = (self.video_dots_elapsed
                    - i as f32 * video_tiles::VIDEO_DOT_PULSE_STAGGER_SECS)
                    / video_tiles::VIDEO_DOT_PULSE_PERIOD_SECS
                    * std::f32::consts::TAU;
                let alpha = video_tiles::VIDEO_DOT_ALPHA_MIN
                    + (video_tiles::VIDEO_DOT_ALPHA_MAX - video_tiles::VIDEO_DOT_ALPHA_MIN)
                        * (0.5 + 0.5 * phase.sin());
                if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(dot.id()) {
                    qs.color.w = alpha;
                }
            }
        }
        let _ = self
            .video_tiles
            .error_text
            .set_visible(proteus, error_visible);
    }

    /// Drives the whole light/dark theme system off `theme_progress`. Runs
    /// every tick, unconditionally, regardless of `AppState`, so a component
    /// already reflects the current theme by the time it becomes visible, with
    /// no visibility branching needed here. It owns the ramp itself, the
    /// sun/moon toggle, `background`'s crossfade (persistent chrome, not owned
    /// by any one screen), and each screen's corner-radius and color blends
    /// (`blend_corner_radius`/`blend_primary_color`) and dark-overlay
    /// crossfades.
    fn advance_theme(&mut self, proteus: &mut Proteus, dt: f32) {
        // Ramp theme_progress toward dark_target — linear, not eased, over
        // THEME_MORPH_DURATION_SECS. dark_target itself already flipped
        // instantly on click (advance_nav_click's `NavClick::SetTheme`
        // arm); this is purely the cosmetic catch-up.
        let target = if self.dark_target { 1.0 } else { 0.0 };
        let step = dt / THEME_MORPH_DURATION_SECS;
        if self.theme_progress < target {
            self.theme_progress = (self.theme_progress + step).min(target);
        } else if self.theme_progress > target {
            self.theme_progress = (self.theme_progress - step).max(target);
        }
        let p = self.theme_progress;

        // Active icon = whichever does NOT match the current theme — a
        // mutual-exclusion toggle pair, not two independent buttons.
        // Idempotent every tick; `Handle::set_interactive`'s own doc covers
        // why this is safe to reassert unconditionally.
        let _ = self.theme.sun.set_interactive(proteus, self.dark_target);
        let _ = self.theme.moon.set_interactive(proteus, !self.dark_target);

        // Unconditional dark-overlay crossfades — safe regardless of
        // `AppState`/visibility (a hidden overlay's alpha doesn't matter
        // until it's shown again, and by then it's already theme-correct),
        // so they all live here rather than in a later per-screen step —
        // `loading.logo_dark` included, even though (unlike the others)
        // it's screen-specific, not persistent chrome.
        for handle in [
            self.background.dark,
            self.nav.home_dark,
            self.nav.back_dark,
            self.nav.lockup_dark,
            self.loading.logo_dark,
        ] {
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(handle.id()) {
                qs.color.w = p;
            }
        }
        // `sun_dark` holds the *light*-theme art (see `screens::theme`'s
        // doc for why) — it must be fully opaque while light and fade
        // *away* going dark, so its alpha runs inverted from all the other
        // overlays.
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.theme.sun_dark.id())
        {
            qs.color.w = 1.0 - p;
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.theme.moon_dark.id())
        {
            qs.color.w = p;
        }

        // Fade sun/moon in once past Splash, and keep them. `self.state` flips
        // at the start of a transition, so a plain `self.state` check fades
        // them in the instant the transition begins.
        let chrome_visible = if self.state == AppState::Splash {
            0.0
        } else {
            1.0
        };
        if chrome_visible > 0.0 {
            let _ = self.theme.sun.set_visible(proteus, true);
            let _ = self.theme.moon.set_visible(proteus, true);
        }
        let fade_step = dt / theme::FADE_DURATION_SECS;
        for fade in &mut self.theme_icon_fade {
            if *fade < chrome_visible {
                *fade = (*fade + fade_step).min(chrome_visible);
            } else if *fade > chrome_visible {
                *fade = (*fade - fade_step).max(chrome_visible);
            }
        }

        // Position — mirror image of `nav`'s home/back row, anchored to
        // the right edge. Moon = outer (closest to the edge), sun = inner.
        let right_edge = self.viewport_size.x / 2.0 - theme::MARGIN_PX;
        let moon_x = right_edge - theme::ICON_SIZE_PX / 2.0;
        let sun_x = moon_x - theme::ICON_SIZE_PX - theme::GAP_PX;
        let y = self.viewport_size.y / 2.0 - theme::MARGIN_PX - theme::ICON_SIZE_PX / 2.0;
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.theme.sun.id())
        {
            qs.position.x = sun_x;
            qs.position.y = y;
            qs.color.w = self.theme_icon_fade[0];
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.theme.moon.id())
        {
            qs.position.x = moon_x;
            qs.position.y = y;
            qs.color.w = self.theme_icon_fade[1];
        }

        // `home`'s tiles — corner radius, twice as round in the dark theme,
        // and Border/Glow/label RGB — and its heading and tagline.
        for &tile in &self.home.tiles {
            blend_corner_radius(
                proteus,
                tile,
                home::CORNER_RADIUS,
                home::CORNER_RADIUS_DARK,
                p,
            );
            blend_primary_color(proteus, tile, p);
        }
        for &label in &self.home.labels {
            blend_primary_color(proteus, label, p);
        }
        blend_primary_color(proteus, self.home.heading, p);
        blend_primary_color(proteus, self.home.tagline, p);
        // `nav.home`/`nav.back` — Glow only (border+glyph are baked into
        // the PNG art itself, and neither has a `Text` label), and both
        // icons' dark overlays above (unlike this pair) are a plain
        // continuous crossfade, not hard-gated — see `screens::nav`'s doc.
        blend_primary_color(proteus, self.nav.home, p);
        blend_primary_color(proteus, self.nav.back, p);

        // `home_selected`/`home_selected_dark` share `nav_home_selected_fade`
        // (`Demo::advance_nav_icons`' envelope) but split it by a hard
        // `dark_target` gate rather than a true bilinear (page-selected ×
        // theme) blend. The two icons are alternative artwork for the same
        // slot, not two layers to cross-fade, so a continuous lerp would
        // show both at half strength mid-theme-change.
        let (light_w, dark_w) = if self.dark_target {
            (0.0, 1.0)
        } else {
            (1.0, 0.0)
        };
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.nav.home_selected.id())
        {
            qs.color.w = self.nav_home_selected_fade * light_w;
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.nav.home_selected_dark.id())
        {
            qs.color.w = self.nav_home_selected_fade * dark_w;
        }

        // `example_detail.panel` — corner radius is a *real* light/dark
        // difference here (12→18, unlike every button family above), Border
        // only (no `Glow`/`Text` on the panel itself — see its own spawn
        // doc).
        blend_corner_radius(
            proteus,
            self.example_detail.panel,
            example_detail::CORNER_RADIUS,
            example_detail::CORNER_RADIUS_DARK,
            p,
        );
        blend_primary_color(proteus, self.example_detail.panel, p);

        // The 6 category titles — Text only. Deliberately narrower scope
        // than `content_handles` (each category's own row labels/content
        // boxes stay a fixed light-treatment violet, never blended) —
        // deliberate, not an oversight.
        for &heading in &self.example_detail.headings {
            blend_primary_color(proteus, heading, p);
        }

        // `video_tiles.tiles` — corner radius blends between the tile pair
        // and the screen pair depending on whether *this* tile is currently
        // the video screen (`screen_focus_idx`); Border/Glow color blends
        // unconditionally regardless, since (unlike corner radius) neither
        // rides the tile↔screen transition's own eased curve — only
        // `QuadState` fields do.
        let screen_focus_idx = match self.state {
            AppState::VideoScreen(idx) => Some(idx),
            _ => None,
        };
        for (i, &tile) in self.video_tiles.tiles.iter().enumerate() {
            // While `tile` has its own active 1:1 `animate_to` transition
            // (the tile↔screen transition itself), leave corner_radius alone —
            // reasserting here would fight that transition's own eased
            // curve instead of letting it ease smoothly from the tile's
            // shape to the screen's. Reasserted immediately once settled,
            // using the screen pair now that it genuinely IS screen-shaped.
            let transitioning = proteus
                .get(tile)
                .map(|d| d.transition.is_some())
                .unwrap_or(false);
            if !(transitioning && screen_focus_idx == Some(i)) {
                let (light_r, dark_r) = if screen_focus_idx == Some(i) {
                    (
                        video_tiles::SCREEN_CORNER_RADIUS,
                        video_tiles::SCREEN_CORNER_RADIUS_DARK,
                    )
                } else {
                    (
                        video_tiles::TILE_CORNER_RADIUS,
                        video_tiles::TILE_CORNER_RADIUS_DARK,
                    )
                };
                blend_corner_radius(proteus, tile, light_r, dark_r, p);
            }
            blend_primary_color(proteus, tile, p);
        }

        // `gallery`'s 12 tiles + `enlarged` — real tiles stay hidden/static
        // during a `Loading`↔`Gallery` `Grid` transition (only the
        // virtuals animate), so it's always safe to reassert corner radius
        // unconditionally here, no "actively transitioning" guard needed (unlike
        // `video_tiles.tiles` above).
        for &tile in &self.gallery.tiles {
            blend_corner_radius(
                proteus,
                tile,
                gallery::CORNER_RADIUS,
                gallery::CORNER_RADIUS_DARK,
                p,
            );
            blend_primary_color(proteus, tile, p);
        }
        blend_corner_radius(
            proteus,
            self.gallery.enlarged,
            gallery::CORNER_RADIUS,
            gallery::CORNER_RADIUS_DARK,
            p,
        );
        blend_primary_color(proteus, self.gallery.enlarged, p);

        // `gallery.fetch_button` — corner radius blends against the
        // NAV_BUTTON pair (see `gallery::FETCH_BUTTON_CORNER_RADIUS`'s own doc).
        blend_corner_radius(
            proteus,
            self.gallery.fetch_button,
            gallery::FETCH_BUTTON_CORNER_RADIUS,
            gallery::FETCH_BUTTON_CORNER_RADIUS_DARK,
            p,
        );
        blend_primary_color(proteus, self.gallery.fetch_button, p);
        blend_primary_color(proteus, self.gallery.fetch_button_label, p);
    }

    /// Fades/positions `nav`'s persistent chrome (`lockup`, `home`, `back`)
    /// and cross-fades `home_selected`'s *envelope* (the hard dark_target
    /// gate on top of it is `advance_theme`'s final say — see this fn's
    /// note below), and drives their hover glow/scale. Runs every tick,
    /// unconditionally, same "no visibility branching needed at the call
    /// site" shape as `advance_theme`.
    ///
    /// `home` is up on every state past `Splash` — including `Home` itself,
    /// unlike `back` (idle-screens-only) — since it's also persistent brand
    /// chrome, not just a "how do I get home" affordance; the `home_selected`
    /// overlay is what actually communicates "you're here" on top of it, not
    /// visibility of the icon itself. Called before `advance_theme`, so that
    /// function always has the final say on `home_selected`'s actual alpha
    /// split for the tick.
    fn advance_nav_icons(&mut self, proteus: &mut Proteus, dt: f32) {
        // `home` fades in/out with `Splash` alone. This crate flips
        // `self.state` synchronously at the *start* of the splash→home
        // transition rather than once it lands (see `Demo::advance_state`),
        // so a plain state check gives "the instant the transition begins,
        // not once it ends" timing.
        let home_target = if self.state == AppState::Splash {
            0.0
        } else {
            1.0
        };
        // `back` only ever fades in once idle on a "leaf" screen — clicking
        // home from any of these skips straight to `Home` without an
        // intermediate `back_target > 0` state to pass through.
        let back_target = match self.state {
            AppState::VideoScreen(_) | AppState::ExampleDetail(_) | AppState::GalleryImage(_) => {
                1.0
            }
            _ => 0.0,
        };
        let targets = [home_target, back_target];
        // No hover reaction while already resting on Home — clicking there
        // would be a no-op, so there's nothing to invite hover feedback for
        // (mirrors `advance_tile_hover`'s suppression on the playing tile,
        // for the same reason). Toggling `Interactable` off is what actually
        // suppresses it: a non-interactive component has no hover or focused
        // style, and can't take focus — same mechanism `advance_theme`
        // already uses for the sun/moon mutual-exclusion toggle. Idempotent
        // every tick, same as that call.
        let _ = self
            .nav
            .home
            .set_interactive(proteus, self.state != AppState::Home);

        let logo_left_edge = -self.viewport_size.x / 2.0 + nav::MARGIN_PX;
        let base_x = logo_left_edge
            + nav::LOGO_TEXT_RIGHT_PX
            + nav::LOGO_ICONS_GAP_PX
            + nav::ICON_SIZE_PX / 2.0;
        let y = self.viewport_size.y / 2.0 - nav::MARGIN_PX - nav::ICON_SIZE_PX / 2.0;
        let xs = [base_x, base_x + nav::ICON_SIZE_PX + nav::GAP_PX];

        // Logo fades in alongside `home` (same target/duration), then stays
        // up (that target never returns to 0 past Splash).
        if home_target > 0.0 {
            let _ = self.nav.lockup.set_visible(proteus, true);
        }
        let logo_step = dt / nav::FADE_DURATION_SECS;
        if self.nav_lockup_fade < home_target {
            self.nav_lockup_fade = (self.nav_lockup_fade + logo_step).min(home_target);
        } else if self.nav_lockup_fade > home_target {
            self.nav_lockup_fade = (self.nav_lockup_fade - logo_step).max(home_target);
        }
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.nav.lockup.id())
        {
            qs.position.x = logo_left_edge + nav::LOGO_WIDTH_PX / 2.0;
            qs.position.y = y;
        }
        // Fades `lockup_dark` too, which is a child.
        let _ = self.nav.lockup.set_opacity(proteus, self.nav_lockup_fade);

        // `home_selected`'s fade *envelope* — up while Home is the current
        // or (mid-transition) destination state. The final light/dark alpha
        // split against this envelope is `advance_theme`'s job (hard-gated
        // by `dark_target`, not a continuous lerp — see that fn's doc), so
        // this only writes the envelope value itself, not either overlay's
        // actual `color.w`.
        let home_selected_target = if self.state == AppState::Home {
            1.0
        } else {
            0.0
        };
        let home_selected_step = dt / nav::FADE_DURATION_SECS;
        if self.nav_home_selected_fade < home_selected_target {
            self.nav_home_selected_fade =
                (self.nav_home_selected_fade + home_selected_step).min(home_selected_target);
        } else if self.nav_home_selected_fade > home_selected_target {
            self.nav_home_selected_fade =
                (self.nav_home_selected_fade - home_selected_step).max(home_selected_target);
        }

        for (i, &icon) in [self.nav.home, self.nav.back].iter().enumerate() {
            let target = targets[i];
            if target > 0.0 {
                let _ = icon.set_visible(proteus, true);
            }
            let step = dt / nav::FADE_DURATION_SECS;
            if self.nav_icon_fade[i] < target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] + step).min(target);
            } else if self.nav_icon_fade[i] > target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] - step).max(target);
            }
            if target <= 0.0 && self.nav_icon_fade[i] <= 0.0 {
                let _ = icon.set_visible(proteus, false);
            }

            // Position + fade only — the glow and growth on hover or focus
            // are the icons' interaction style. The opacity fades the glow
            // and the icon's overlays with it.
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(icon.id()) {
                qs.position.x = xs[i];
                qs.position.y = y;
            }
            let _ = icon.set_opacity(proteus, self.nav_icon_fade[i]);
        }
    }

    /// Pointer position in **world-space** (viewport-center origin, Y-up) —
    /// see [`proteus_sdk::Proteus::pointer_moved`]'s doc for the exact
    /// contract and the conversion a caller needs from window/CSS pixels.
    ///
    /// Test-only. A host drives input through `Engine::pointer_*`, which
    /// reaches `Proteus` without passing through `Demo` at all; these three
    /// forwarders exist so the test harness can drive input without standing
    /// up an `Engine`.
    #[cfg(test)]
    pub(crate) fn pointer_moved(&mut self, proteus: &mut Proteus, pos: Option<Vec2>) {
        proteus.pointer_moved(pos);
    }

    #[cfg(test)]
    pub(crate) fn pointer_pressed(&mut self, proteus: &mut Proteus) {
        proteus.pointer_pressed();
    }

    #[cfg(test)]
    pub(crate) fn pointer_released(&mut self, proteus: &mut Proteus) {
        proteus.pointer_released();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_sdk::Key;

    // Test harness: owns the `Proteus` that `Demo` is given, and runs the
    // full frame sequence each `tick` (`Proteus::tick` → `Demo::advance` →
    // `Proteus::refresh_cascades`, the same order `DemoApp`'s engine uses).
    // Derefs to `Demo` for reads; `.app` is the world these tests inspect
    // directly via `Proteus::get` / `world()`.
    struct Harness {
        app: Proteus,
        demo: Demo,
    }

    impl Harness {
        fn new() -> Self {
            let mut app = Proteus::new();
            let demo = Demo::new(&mut app);
            Self { app, demo }
        }

        fn tick(&mut self, dt: f32) {
            self.app.tick(dt);
            self.demo.advance(&mut self.app, dt);
            self.app.refresh_cascades();
        }
    }

    impl std::ops::Deref for Harness {
        type Target = Demo;
        fn deref(&self) -> &Self::Target {
            &self.demo
        }
    }
    impl std::ops::DerefMut for Harness {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.demo
        }
    }

    // Forward a `Demo` method that now takes `&mut Proteus` as its first
    // arg, so tests can keep calling `harness.start_foo()`.
    macro_rules! harness_forward {
        ($( fn $name:ident ( $($arg:ident : $ty:ty),* ) );* $(;)?) => {
            impl Harness {
                $( #[allow(dead_code)] fn $name(&mut self $(, $arg: $ty)*) {
                    self.demo.$name(&mut self.app $(, $arg)*);
                } )*
            }
        };
    }

    harness_forward! {
        fn start_home_to_tiles();
        fn start_tiles_to_home();
        fn start_tiles_to_screen(idx: usize);
        fn start_screen_to_tiles(idx: usize);
        fn start_screen_to_home(idx: usize);
        fn start_home_to_loading();
        fn start_loading_to_home();
        fn start_loading_to_gallery();
        fn start_gallery_to_loading();
        fn start_gallery_to_home();
        fn start_gallery_to_image(idx: usize);
        fn start_image_to_gallery();
        fn start_image_to_home();
        fn start_home_to_detail(idx: usize);
        fn start_detail_to_home();
        fn set_gallery_hires_image(idx: usize, bytes: Vec<u8>);
        fn set_gallery_tile_image(idx: usize, bytes: Vec<u8>, aspect: Vec2);
        fn pointer_moved(pos: Option<Vec2>);
        fn pointer_pressed();
        fn pointer_released();
    }

    // How long a test needs to tick to be certain `Demo` has moved past
    // `Splash` — derived from the real constants (delay + intro + hold,
    // plus a small margin) rather than a hardcoded literal, so a change to
    // any of them (including a deliberate temporary one, e.g. while
    // visually verifying the Splash sequence — see `splash::HOLD_SECS`'s
    // own doc) can't silently desync every test that needs to get past
    // Splash first.
    fn past_splash_secs() -> f32 {
        splash::INTRO_DELAY_SECS + splash::INTRO_DURATION_SECS + splash::HOLD_SECS + 0.5
    }

    // `collect_instances` draws root entities in ascending `position.z`
    // order, breaking ties by `SpawnOrder` (earlier-spawned = further
    // back). Before `SpawnOrder` existed, a tie fell back to ECS iteration
    // order, which tracks archetype creation (roughly "when this exact set
    // of components was first seen") rather than spawn time or draw intent.
    // `background` gains its `Image` component *after* all the other screens
    // have already spawned (via `Demo::set_background_image`) — which landed
    // it in a newer archetype than content spawned earlier with a
    // *matching* z, and silently drew it on top,
    // hiding that content entirely. Every root entity meant to be visible
    // over the background must have a strictly greater z — this guards
    // that invariant for every screen reachable from a fresh `Demo`,
    // rather than relying on visual inspection to catch the next one.
    #[test]
    fn every_reachable_screens_content_draws_above_the_background() {
        let mut demo = Harness::new();
        let background_z = demo
            .app
            .get(demo.background.light)
            .unwrap()
            .geometry
            .position
            .z;

        let assert_above = |demo: &Harness, handle: Handle, what: &str| {
            let z = demo.app.get(handle).unwrap().geometry.position.z;
            assert!(
                z > background_z,
                "{what} (z={z}) must draw above background (z={background_z})"
            );
        };

        // Splash.
        assert_above(&demo, demo.splash.button, "splash.button");

        // Home — advance well past the Splash intro/hold/transition.
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);
        for &tile in &demo.home.tiles {
            assert_above(&demo, tile, "home.tiles");
        }
        assert_above(&demo, demo.home.heading, "home.heading");
        assert_above(&demo, demo.home.tagline, "home.tagline");

        // VideoTiles, then back to Home before continuing on to ExampleDetail.
        demo.start_home_to_tiles();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoTiles);
        for &tile in &demo.video_tiles.tiles {
            assert_above(&demo, tile, "video_tiles.tiles");
        }
        assert_above(&demo, demo.nav.home, "nav.home");
        demo.start_tiles_to_home();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Home);

        // ExampleDetail(0).
        demo.start_home_to_detail(0);
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::ExampleDetail(0));
        assert_above(&demo, demo.example_detail.panel, "example_detail.panel");
        assert_above(&demo, demo.nav.back, "nav.back");
    }

    // Checks the Splash intro fade sequence
    // (`Demo::advance_intro`/`Demo::advance_state`'s Splash arm): invisible
    // for `INTRO_DELAY_SECS`, fades 0 → 1 over `INTRO_DURATION_SECS`, then
    // holds at full opacity for `HOLD_SECS` before handing off to `Home`.
    // The other Splash tests fast-forward straight past all of
    // this with `past_splash_secs()`; this one samples mid-flight,
    // so a mistake in the ramp itself (as opposed to "does it eventually
    // reach Home") gets caught.
    #[test]
    fn splash_button_fades_in_after_the_intro_delay_then_holds_before_advancing() {
        let mut demo = Harness::new();
        let alpha = |demo: &Harness| demo.app.get(demo.splash.button).unwrap().geometry.color.w;

        // Still within the initial delay: fully invisible.
        let mut t = 0.0;
        while t < splash::INTRO_DELAY_SECS - 0.1 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(
            alpha(&demo),
            0.0,
            "must stay invisible during the intro delay"
        );
        assert_eq!(demo.state, AppState::Splash);

        // Partway through the fade itself: neither fully transparent nor
        // fully opaque.
        while t < splash::INTRO_DELAY_SECS + splash::INTRO_DURATION_SECS / 2.0 {
            demo.tick(0.05);
            t += 0.05;
        }
        let mid_alpha = alpha(&demo);
        assert!(
            mid_alpha > 0.0 && mid_alpha < 1.0,
            "must be mid-fade partway through the intro, got {mid_alpha}"
        );
        assert_eq!(demo.state, AppState::Splash);

        // Fade has settled, but the hold hasn't elapsed yet: fully opaque,
        // still resting on Splash.
        while t < splash::INTRO_DELAY_SECS + splash::INTRO_DURATION_SECS + 0.1 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(
            alpha(&demo),
            1.0,
            "must be fully opaque once the fade settles"
        );
        assert_eq!(
            demo.state,
            AppState::Splash,
            "must keep holding on Splash until HOLD_SECS elapses"
        );

        // Hold elapses: hands off to Home.
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);
    }

    // Clicks each example tile with the pointer, rather than calling
    // `start_home_to_detail` directly, so a tile left out of `Demo::new`'s
    // click wiring is caught. A fresh `Demo` each: see
    // `going_home_and_back_again_reopens_a_panel` for a round trip.
    #[test]
    fn each_example_tile_opens_its_own_panel() {
        for idx in 0..example_detail::CATEGORY_TITLES.len() {
            let mut demo = advance_to_home();
            let tile = demo.home.tiles[home::FIRST_EXAMPLE + idx];
            click(&mut demo, tile);
            assert_eq!(
                demo.state,
                AppState::ExampleDetail(idx),
                "clicking example tile {idx} should open ExampleDetail({idx})"
            );
            for handle in demo.example_detail.content_handles(idx) {
                assert!(
                    demo.app.get(handle).unwrap().visible,
                    "panel {idx}'s content should show once it settles"
                );
            }
        }
    }

    // Home's whole grid merges into the panel, and the panel splits back
    // into all of it: every tile is hidden while the panel shows, and shown
    // again once back.
    #[test]
    fn a_panel_comes_from_and_returns_to_the_whole_grid() {
        let mut demo = advance_to_home();
        demo.start_home_to_detail(5);
        demo.tick(1.0);
        assert!(demo.app.get(demo.example_detail.panel).unwrap().visible);
        for &tile in &demo.home.tiles {
            assert!(!demo.app.get(tile).unwrap().visible);
        }

        demo.start_detail_to_home();
        assert_eq!(demo.state, AppState::Home);
        demo.tick(1.0);
        assert!(!demo.app.get(demo.example_detail.panel).unwrap().visible);
        for &tile in &demo.home.tiles {
            assert!(demo.app.get(tile).unwrap().visible);
        }
        for handle in demo.example_detail.content_handles(5) {
            assert!(!demo.app.get(handle).unwrap().visible);
        }
    }

    // Home's heading and tagline fade out as the grid leaves, and back in
    // as it returns.
    #[test]
    fn the_heading_and_tagline_fade_with_the_grid() {
        let mut demo = advance_to_home();
        demo.tick(1.0);
        let opacity = |demo: &Harness, handle: Handle| {
            demo.app
                .world()
                .get::<proteus_ui::Opacity>(handle.id())
                .map(|o| o.0)
                .unwrap_or(1.0)
        };
        assert_eq!(opacity(&demo, demo.home.heading), 1.0);
        assert!(demo.app.get(demo.home.tagline).unwrap().visible);

        demo.start_home_to_detail(0);
        demo.tick(home::TITLE_FADE_SECS / 2.0);
        let mid = opacity(&demo, demo.home.heading);
        assert!(mid > 0.0 && mid < 1.0, "should be fading out, got {mid}");
        demo.tick(1.0);
        assert!(!demo.app.get(demo.home.heading).unwrap().visible);
        assert!(!demo.app.get(demo.home.tagline).unwrap().visible);

        demo.start_detail_to_home();
        demo.tick(1.0);
        assert_eq!(opacity(&demo, demo.home.tagline), 1.0);
        assert!(demo.app.get(demo.home.heading).unwrap().visible);
    }

    // Escape is bound to Back: from a panel it goes Home, as the back icon
    // does, and from the video tiles, which Home leads to directly, it goes
    // Home too.
    #[test]
    fn escape_goes_back_a_screen() {
        let mut demo = advance_to_home();
        demo.start_home_to_detail(1);
        demo.tick(1.0);
        press_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::Escape),
            proteus_sdk::Code::Escape,
        );
        assert_eq!(demo.state, AppState::Home);

        demo.tick(1.0);
        demo.start_home_to_tiles();
        demo.tick(1.0);
        press_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::Escape),
            proteus_sdk::Code::Escape,
        );
        assert_eq!(demo.state, AppState::Home);
    }

    fn key_event(key: Key, code: proteus_sdk::Code, state: KeyState) -> proteus_sdk::KeyEvent {
        proteus_sdk::KeyEvent::new(key, code, state, proteus_sdk::Modifiers::default(), None)
    }

    // Presses and releases `key`, a tick apart.
    fn press_key(demo: &mut Harness, key: Key, code: proteus_sdk::Code) {
        demo.app
            .key_event(key_event(key.clone(), code.clone(), KeyState::Pressed));
        demo.tick(0.016);
        demo.app.key_event(key_event(key, code, KeyState::Released));
        demo.tick(0.016);
    }

    // Clicks `handle` at its center with the pointer.
    fn click(demo: &mut Harness, handle: Handle) {
        let probe = demo.app.get(handle).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(0.016);
        demo.pointer_released();
        demo.tick(1.0);
    }

    fn advance_to_keys_panel() -> Harness {
        let mut demo = advance_to_home();
        demo.start_home_to_detail(example_detail::KEYS);
        demo.tick(1.0);
        // The space bar takes focus a frame after the keyboard shows: see
        // `Demo::advance_keyboard`.
        demo.tick(0.016);
        demo
    }

    /// The virtual keycap of the key at `code`.
    fn keycap(demo: &Harness, code: proteus_sdk::Code) -> Handle {
        let keyboard = &demo.example_detail.keys.keyboard;
        keyboard.keys[keyboard.index_of(&code).expect("on the keyboard")]
    }

    fn fill(demo: &Harness, key: Handle) -> Vec4 {
        demo.app.get(key).unwrap().geometry.color
    }

    fn label(demo: &Harness, key: Handle) -> String {
        demo.app
            .world()
            .get::<Text>(key.id())
            .unwrap()
            .content
            .clone()
    }

    /// Presses the key at `code`, holds it `hold` seconds, and releases it.
    fn hold_key(demo: &mut Harness, key: Key, code: proteus_sdk::Code, hold: f32) {
        demo.app
            .key_event(key_event(key.clone(), code.clone(), KeyState::Pressed));
        let mut held = 0.0;
        while held < hold {
            demo.tick(0.05);
            held += 0.05;
        }
        demo.app.key_event(key_event(key, code, KeyState::Released));
        demo.tick(0.016);
    }

    // A key pressed on the real keyboard flashes its keycap, matched by
    // where it is, and the flash fades back to white.
    #[test]
    fn a_key_pressed_flashes_its_keycap_then_fades() {
        let mut demo = advance_to_keys_panel();
        let a = keycap(&demo, proteus_sdk::Code::KeyA);
        assert_eq!(fill(&demo, a), Vec4::ONE);

        // Shift+A types "A", but it's the same key, so the same keycap.
        demo.app.key_event(key_event(
            Key::Character("A".to_string()),
            proteus_sdk::Code::KeyA,
            KeyState::Pressed,
        ));
        demo.tick(0.016);
        assert_ne!(fill(&demo, a), Vec4::ONE, "the keycap flashes");
        assert_eq!(
            fill(&demo, keycap(&demo, proteus_sdk::Code::KeyS)),
            Vec4::ONE,
            "and no other"
        );
        demo.tick(screens::keyboard::FLASH_SECS);
        assert_eq!(fill(&demo, a), Vec4::ONE, "then fades back");
    }

    // Caps Lock, a press and a release at each tap on every host, switches
    // the keycaps to their shifted values and tints its own keycap, and
    // switches them back at the next tap. A long press of Shift does nothing
    // more than flash it.
    #[test]
    fn caps_lock_toggles_the_shifted_values() {
        let mut demo = advance_to_keys_panel();
        let caps = || Key::Named(proteus_sdk::NamedKey::CapsLock);
        let q = keycap(&demo, proteus_sdk::Code::KeyQ);
        let one = keycap(&demo, proteus_sdk::Code::Digit1);
        let caps_key = keycap(&demo, proteus_sdk::Code::CapsLock);

        hold_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::Shift),
            proteus_sdk::Code::ShiftLeft,
            0.7,
        );
        assert!(!demo.keyboard_shifted, "Shift doesn't toggle");

        hold_key(&mut demo, caps(), proteus_sdk::Code::CapsLock, 0.1);
        assert!(demo.keyboard_shifted);
        assert_eq!(label(&demo, q), "Q");
        assert_eq!(label(&demo, one), "!");
        demo.tick(1.0);
        assert_ne!(fill(&demo, caps_key), Vec4::ONE, "Caps Lock stays tinted");
        assert_eq!(fill(&demo, q), Vec4::ONE);

        hold_key(&mut demo, caps(), proteus_sdk::Code::CapsLock, 0.1);
        assert!(!demo.keyboard_shifted);
        assert_eq!(label(&demo, one), "1");
        demo.tick(1.0);
        assert_eq!(fill(&demo, caps_key), Vec4::ONE);
    }

    // Leaving the panel unshifts the keyboard, so it comes back with its own
    // values.
    #[test]
    fn leaving_the_keys_panel_unshifts_the_keyboard() {
        let mut demo = advance_to_keys_panel();
        hold_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::CapsLock),
            proteus_sdk::Code::CapsLock,
            0.1,
        );
        assert!(demo.keyboard_shifted);
        demo.start_detail_to_home();
        assert!(!demo.keyboard_shifted);
        let q = keycap(&demo, proteus_sdk::Code::KeyQ);
        assert_eq!(label(&demo, q), "q");
    }

    // Focus starts on the space bar each time the panel opens, once the
    // keyboard shows.
    #[test]
    fn focus_starts_on_the_space_bar() {
        let mut demo = advance_to_keys_panel();
        let space = keycap(&demo, proteus_sdk::Code::Space);
        assert_eq!(demo.app.focused(), Some(space));

        let _ = keycap(&demo, proteus_sdk::Code::KeyQ).focus(&mut demo.app);
        demo.start_detail_to_home();
        demo.tick(1.0);
        demo.start_home_to_detail(example_detail::KEYS);
        demo.tick(1.0);
        demo.tick(0.016);
        assert_eq!(
            demo.app.focused(),
            Some(space),
            "and again on a second visit"
        );
    }

    // A key that isn't on the virtual keyboard, such as F1, flashes nothing,
    // and nor does a key pressed on another screen.
    #[test]
    fn keys_not_on_the_keyboard_or_pressed_elsewhere_flash_nothing() {
        let mut demo = advance_to_home();
        press_key(
            &mut demo,
            Key::Character("z".to_string()),
            proteus_sdk::Code::KeyZ,
        );
        demo.start_home_to_detail(example_detail::KEYS);
        demo.tick(0.016);
        press_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::F1),
            proteus_sdk::Code::F1,
        );
        for &key in &demo.example_detail.keys.keyboard.keys {
            assert_eq!(fill(&demo, key), Vec4::ONE);
        }
    }

    // The arrows and Tab move between keycaps, and Enter or a click flashes
    // the keycap chosen.
    #[test]
    fn the_arrows_and_tab_move_between_keycaps_and_enter_flashes_one() {
        let mut demo = advance_to_keys_panel();
        let q = keycap(&demo, proteus_sdk::Code::KeyQ);
        let w = keycap(&demo, proteus_sdk::Code::KeyW);
        let e = keycap(&demo, proteus_sdk::Code::KeyE);
        let _ = q.focus(&mut demo.app);
        demo.tick(0.016);
        let right = || Key::Named(proteus_sdk::NamedKey::ArrowRight);
        // The first arrow after the pointer only shows where focus is.
        press_key(&mut demo, right(), proteus_sdk::Code::ArrowRight);
        assert_eq!(demo.app.focused(), Some(q));
        press_key(&mut demo, right(), proteus_sdk::Code::ArrowRight);
        assert_eq!(
            demo.app.focused(),
            Some(w),
            "Right moves to the next keycap"
        );
        press_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::Tab),
            proteus_sdk::Code::Tab,
        );
        assert_eq!(demo.app.focused(), Some(e), "and so does Tab");

        press_key(
            &mut demo,
            Key::Named(proteus_sdk::NamedKey::Enter),
            proteus_sdk::Code::Enter,
        );
        assert_ne!(
            fill(&demo, e),
            Vec4::ONE,
            "Enter flashes the focused keycap"
        );

        // The keycaps are placed in the keyboard, so the click goes where
        // the keyboard puts `w`.
        let board = demo
            .app
            .get(demo.example_detail.keys.keyboard.board)
            .unwrap()
            .geometry
            .position;
        let local = demo.app.get(w).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(board.x + local.x, board.y + local.y)));
        demo.pointer_pressed();
        demo.tick(0.016);
        demo.pointer_released();
        demo.tick(0.016);
        assert_ne!(fill(&demo, w), Vec4::ONE, "a click flashes a keycap too");
    }

    /// Ticks past a transition and the frame after, when focus lands.
    fn settle(demo: &mut Harness) {
        demo.tick(1.0);
        demo.tick(0.016);
        demo.tick(0.016);
    }

    fn named(key: proteus_sdk::NamedKey) -> Key {
        Key::Named(key)
    }

    // As each transition ends, focus lands where the keys pick up from: the
    // tile Home was left from, the tile or photo that was open, the back
    // icon on a leaf screen, and nothing while loading.
    #[test]
    fn focus_lands_where_each_screen_picks_up_from() {
        let mut demo = advance_to_home();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.home.tiles[0]), "Home, first");

        demo.start_home_to_tiles();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.video_tiles.tiles[0]));
        demo.start_tiles_to_screen(1);
        demo.take_pending_video_start();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.nav.back), "a video plays");
        demo.start_screen_to_tiles(1);
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.video_tiles.tiles[1]));
        demo.start_tiles_to_home();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.home.tiles[0]));

        demo.start_home_to_loading();
        demo.take_pending_gallery_fetch();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), None, "nothing while loading");
        demo.start_loading_to_gallery();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.gallery.tiles[0]));
        demo.start_gallery_to_image(3);
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.gallery.enlarged));
        demo.start_image_to_gallery();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.gallery.tiles[3]));
        demo.start_gallery_to_home();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.home.tiles[1]));

        demo.start_home_to_detail(2);
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.nav.back), "a panel");
        demo.start_detail_to_home();
        settle(&mut demo);
        assert_eq!(
            demo.app.focused(),
            Some(demo.home.tiles[home::FIRST_EXAMPLE + 2])
        );
    }

    // The keys alone go from Home into a screen and back: an arrow moves
    // between tiles, Enter opens one, and Escape comes back.
    #[test]
    fn the_keys_open_a_tile_from_home_and_escape_comes_back() {
        let mut demo = advance_to_home();
        settle(&mut demo);
        let right = || named(proteus_sdk::NamedKey::ArrowRight);
        // The first arrow after the pointer only shows where focus is.
        press_key(&mut demo, right(), proteus_sdk::Code::ArrowRight);
        press_key(&mut demo, right(), proteus_sdk::Code::ArrowRight);
        assert_eq!(demo.app.focused(), Some(demo.home.tiles[1]));
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::Enter),
            proteus_sdk::Code::Enter,
        );
        assert_eq!(demo.state, AppState::Loading);
        demo.take_pending_gallery_fetch();
        // Escape waits for the change of screen under way to end.
        settle(&mut demo);
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::Escape),
            proteus_sdk::Code::Escape,
        );
        assert_eq!(
            demo.state,
            AppState::Home,
            "Escape leaves the loading screen"
        );
    }

    // Escape goes back one screen from each screen it can.
    #[test]
    fn escape_goes_back_from_every_screen() {
        let escape = |demo: &mut Harness| {
            press_key(
                demo,
                named(proteus_sdk::NamedKey::Escape),
                proteus_sdk::Code::Escape,
            );
        };
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0);
        escape(&mut demo);
        assert_eq!(
            demo.state,
            AppState::VideoTiles,
            "a video goes back to its tiles"
        );

        let mut demo = advance_to_gallery_image(2);
        escape(&mut demo);
        assert_eq!(
            demo.state,
            AppState::Gallery,
            "a photo goes back to the gallery"
        );
        demo.tick(1.0);
        escape(&mut demo);
        assert_eq!(demo.state, AppState::Home, "the gallery goes Home");
    }

    // A video tile's overlay and title show while it's focused, with the
    // keys in use, as they do while it's hovered with the pointer.
    #[test]
    fn a_focused_video_tile_shows_its_overlay_while_navigating() {
        let mut demo = advance_to_video_tiles();
        settle(&mut demo);
        assert_eq!(demo.app.focused(), Some(demo.video_tiles.tiles[0]));
        let overlay_alpha = |demo: &Harness| {
            demo.app
                .get(demo.video_tiles.tile_overlays[0])
                .unwrap()
                .geometry
                .color
                .w
        };
        assert_eq!(overlay_alpha(&demo), 0.0, "not while pointing");

        // An arrow, the first after the pointer, shows where focus is.
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::ArrowRight),
            proteus_sdk::Code::ArrowRight,
        );
        demo.tick(screens::styles::SECS + 0.1);
        assert!(
            (overlay_alpha(&demo) - video_tiles::TILE_OVERLAY_MAX_ALPHA).abs() < 0.01,
            "shown once navigating, got {}",
            overlay_alpha(&demo)
        );
    }

    // A video tile clicked with the pointer, which then rests over the
    // playing video, and Backspace to back out: the tile becomes clickable
    // again as the screen, under the pointer, so it's hovered as it splits
    // back into the tiles. It must still end at its place in the grid, not
    // back at the screen's size (B-38: `set_declared_geometry` was undone by
    // the hover style's change under way).
    #[test]
    fn backing_out_of_a_video_with_backspace_ends_in_the_grid() {
        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[1];
        click(&mut demo, tile);
        assert_eq!(demo.state, AppState::VideoScreen(1));
        demo.take_pending_video_start();
        settle(&mut demo);
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::Backspace),
            proteus_sdk::Code::Backspace,
        );
        assert_eq!(demo.state, AppState::VideoTiles);
        settle(&mut demo);
        settle(&mut demo);
        let resting = video_tiles::tile_target_state(&demo.app, tile, 1, demo.theme_progress);
        let now = demo.app.get(tile).unwrap().geometry;
        assert_eq!(now.size, resting.size, "the tile ends at its grid size");
    }

    // Backspace twice, the second before the first's transition ends: the
    // second is ignored, rather than starting the tiles' merge to Home while
    // they're still splitting from the video (B-39). Once the transition
    // ends, Backspace goes back again.
    #[test]
    fn input_that_changes_screens_waits_for_the_transition_under_way() {
        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[1];
        click(&mut demo, tile);
        demo.take_pending_video_start();
        settle(&mut demo);
        let backspace = |demo: &mut Harness| {
            press_key(
                demo,
                named(proteus_sdk::NamedKey::Backspace),
                proteus_sdk::Code::Backspace,
            );
        };
        backspace(&mut demo);
        assert_eq!(demo.state, AppState::VideoTiles);
        backspace(&mut demo);
        assert_eq!(demo.state, AppState::VideoTiles, "the second is ignored");

        settle(&mut demo);
        for &tile in &demo.video_tiles.tiles {
            assert!(demo.app.get(tile).unwrap().visible, "the tiles show");
        }
        for &tile in &demo.home.tiles {
            assert!(!demo.app.get(tile).unwrap().visible);
        }
        backspace(&mut demo);
        assert_eq!(
            demo.state,
            AppState::Home,
            "once it ends, Backspace goes back"
        );
    }

    // A Home tile hovered, then Tab twice, which moves focus on and fades
    // its glow, and the pointer nudged while it fades, as a resting hand on
    // a trackpad does, which hovers it again mid-fade: its glow must still
    // go all the way out once it's neither hovered nor focused, even after
    // another screen and back (B-40).
    #[test]
    fn a_tile_hovered_again_mid_fade_doesnt_keep_its_glow() {
        let mut demo = advance_to_home();
        settle(&mut demo);
        let tile = demo.home.tiles[0];
        let glow = |demo: &Harness| demo.app.world().get::<Glow>(tile.id()).unwrap().radius;
        let p = demo.app.get(tile).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(p.x, p.y)));
        demo.tick(0.5);
        assert_eq!(glow(&demo), screens::styles::GLOW_PX, "hovered");

        let tab = || named(proteus_sdk::NamedKey::Tab);
        press_key(&mut demo, tab(), proteus_sdk::Code::Tab);
        press_key(&mut demo, tab(), proteus_sdk::Code::Tab);
        demo.tick(0.05);
        demo.pointer_moved(Some(Vec2::new(p.x + 1.0, p.y)));
        demo.tick(0.5);
        // The first key after the pointer shows focus, on the tile under
        // it; the next moves focus on.
        press_key(&mut demo, tab(), proteus_sdk::Code::Tab);
        assert_eq!(demo.app.focused(), Some(tile));
        press_key(&mut demo, tab(), proteus_sdk::Code::Tab);
        demo.tick(0.5);
        assert_eq!(glow(&demo), 0.0, "neither hovered nor focused, no glow");

        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::Enter),
            proteus_sdk::Code::Enter,
        );
        demo.take_pending_gallery_fetch();
        settle(&mut demo);
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::Escape),
            proteus_sdk::Code::Escape,
        );
        settle(&mut demo);
        assert_eq!(glow(&demo), 0.0, "and none after another screen and back");
    }

    // The fetch button's focus grows it over the style's time, as the
    // photos' does, rather than at once: its per-frame layout mustn't end
    // the style's change under way (`set_declared_geometry` does).
    #[test]
    fn the_fetch_buttons_focus_grows_over_the_styles_time() {
        let mut demo = advance_to_gallery();
        // The button is laid out every frame once its label has baked, as
        // it has in the app; headless, nothing bakes, so it's given a size.
        let label = demo.gallery.fetch_button_label.id();
        demo.app
            .world_mut()
            .entity_mut(label)
            .insert(proteus_ui::BakedText {
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
                page: 0,
                pixel_size: [180.0, 24.0],
            });
        settle(&mut demo);
        let button = demo.gallery.fetch_button;
        // The first key after the pointer shows focus, on the first photo;
        // focus then goes to the button.
        press_key(
            &mut demo,
            named(proteus_sdk::NamedKey::ArrowUp),
            proteus_sdk::Code::ArrowUp,
        );
        let _ = button.focus(&mut demo.app);
        demo.tick(screens::styles::SECS / 2.0);
        let scale = demo.app.get(button).unwrap().geometry.scale;
        assert!(
            scale > 1.0 && scale < screens::styles::SCALE,
            "growing, not grown at once: {scale}"
        );
        demo.tick(screens::styles::SECS);
        assert_eq!(
            demo.app.get(button).unwrap().geometry.scale,
            screens::styles::SCALE
        );
    }

    // Each video tile merges from its own columns of Home's grid, and
    // together they take every tile exactly once.
    #[test]
    fn the_video_tiles_columns_take_every_home_tile_once() {
        let mut covered: Vec<usize> = VIDEO_TILE_COLUMNS
            .into_iter()
            .flat_map(|(start_col, width)| home::column_group(start_col, width))
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, (0..home::TILE_COUNT).collect::<Vec<_>>());
    }

    #[test]
    fn clicking_the_video_tile_opens_video_tiles_and_home_returns_home() {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);

        let button = demo.home.tiles[0];
        let probe = demo.app.get(button).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        demo.pointer_released();
        assert_eq!(demo.state, AppState::VideoTiles);

        // `nav::Nav`'s icons are center-anchored, like everything else in
        // this crate — `demo.tick(1.0)` above already ran `advance_nav_icons`
        // enough to have positioned `home` for the current viewport, so its
        // live `position` is a safe click target directly.
        let home_button = demo.nav.home;
        let probe = demo.app.get(home_button).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Home);
    }

    #[test]
    fn in_the_dark_theme_a_tile_heads_for_the_dark_screens_radius() {
        let mut demo = advance_to_video_tiles();
        demo.dark_target = true;
        let mut t = 0.0;
        while t < THEME_MORPH_DURATION_SECS + 0.5 {
            demo.tick(0.1);
            t += 0.1;
        }
        assert_eq!(demo.theme_progress, 1.0);
        let tile = demo.video_tiles.tiles[0];
        demo.start_tiles_to_screen(0);
        // Where the transition ends: the dark screen's radius, so it doesn't
        // snap there once it arrives.
        assert_eq!(
            tile.declared_geometry(&demo.app).unwrap().corner_radius,
            video_tiles::SCREEN_CORNER_RADIUS_DARK
        );
    }

    fn advance_to_video_tiles() -> Harness {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        demo.start_home_to_tiles();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoTiles);
        demo
    }

    // On the click frame the clicked tile hasn't moved yet, so no z puts the
    // backdrop strictly behind it: drawn, it would cover the tile for that
    // frame, a black flash. It appears once the tile has risen.
    #[test]
    fn the_video_backdrop_waits_for_the_clicked_tile_to_rise() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        // The rest of the click frame: `Demo::advance`, as `DemoApp::update`
        // runs it, before the next tick moves the tile.
        demo.demo.advance(&mut demo.app, 0.016);
        demo.app.refresh_cascades();
        let backdrop = demo.video_tiles.backdrop;
        assert!(
            !demo.app.get(backdrop).unwrap().visible,
            "hidden on the click frame"
        );

        demo.tick(0.05);
        assert!(
            demo.app.get(backdrop).unwrap().visible,
            "shown once the tile rises"
        );
    }

    #[test]
    fn hovering_a_tile_fades_in_its_overlay_and_title_label() {
        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        let probe = demo.app.get(tile).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        let mut t = 0.0;
        while t < screens::styles::SECS + 0.5 {
            demo.tick(0.05);
            t += 0.05;
        }

        let overlay = demo.video_tiles.tile_overlays[0];
        let overlay_alpha = demo.app.get(overlay).unwrap().geometry.color.w;
        assert!(
            (overlay_alpha - video_tiles::TILE_OVERLAY_MAX_ALPHA).abs() < 0.01,
            "fully-hovered overlay should reach TILE_OVERLAY_MAX_ALPHA, got {overlay_alpha}"
        );

        let label = demo.video_tiles.tile_labels[0];
        let label_alpha = demo.app.world().get::<Text>(label.id()).unwrap().color.w;
        assert!(
            (label_alpha - 1.0).abs() < 0.01,
            "fully-hovered label should fade to fully opaque, got {label_alpha}"
        );

        // Untouched tiles must stay hidden — this isn't a global fade.
        let other_overlay = demo.video_tiles.tile_overlays[1];
        assert_eq!(demo.app.get(other_overlay).unwrap().geometry.color.w, 0.0);
    }

    #[test]
    fn resting_video_screen_tile_scales_up_its_label_and_suppresses_hover() {
        use proteus_ui::Interactable;

        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(1);
        demo.take_pending_video_start();
        demo.tick(1.0); // settle the tile<->screen transition
        assert_eq!(demo.state, AppState::VideoScreen(1));

        let label = demo.video_tiles.tile_labels[1];
        let scale = demo.app.get(label).unwrap().geometry.scale;
        assert_eq!(
            scale,
            video_tiles::TILE_LABEL_SCREEN_SCALE,
            "the screen tile's own label should scale up once settled"
        );

        let tile = demo.video_tiles.tiles[1];
        assert!(
            demo.app.world().get::<Interactable>(tile.id()).is_none(),
            "the settled, playing screen tile shouldn't react to hover — clicking it is a no-op"
        );

        // The other two (idle, grid-shaped) tiles keep their normal 1.0 scale
        // and stay interactable.
        let idle_label = demo.video_tiles.tile_labels[0];
        assert_eq!(demo.app.get(idle_label).unwrap().geometry.scale, 1.0);
        let idle_tile = demo.video_tiles.tiles[0];
        assert!(demo
            .app
            .world()
            .get::<Interactable>(idle_tile.id())
            .is_some());
    }

    // Box-cover tiles must stay untinted after a trip to the video screen
    // and back. `advance_pending_tile_reset` must not reset a tile's whole
    // `QuadState`, including `color`, to `tile_quad`'s placeholder tint: that
    // discards the untinted white `set_tile_image` sets once real box art
    // lands, so the tint would multiply the art on every return after the
    // first.
    #[test]
    fn returning_from_video_screen_keeps_real_box_art_untinted() {
        use proteus_ui::BakedImage;

        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        // Simulate the renderer's bake pass having landed real box-cover art —
        // real baking needs GPU, unavailable in this headless test world.
        demo.app
            .world_mut()
            .entity_mut(tile.id())
            .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [400.0, 600.0]));
        if let Some(mut qs) = demo.app.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color = Vec4::ONE;
        }

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoScreen(0));

        demo.start_screen_to_tiles(0);
        // Two ticks: one lets `one_to_n_setup_system` hide the (still
        // screen-shaped) source, the next lets `advance_pending_tile_reset`
        // observe that and actually apply the reset.
        demo.tick(0.05);
        demo.tick(0.05);
        assert_eq!(
            demo.app.get(tile).unwrap().geometry.color,
            Vec4::ONE,
            "a tile with real box art must stay untinted after returning from the video screen"
        );
    }

    #[test]
    fn returning_from_video_screen_restores_the_non_playing_tiles_own_box_art_too() {
        // When returning from playback, the *other two* tiles must get their
        // backgrounds back. A split's reveal only flips `Visibility` — it never
        // rewrites a target's live `QuadState` or opacity — so those tiles,
        // faded out by `Demo::advance_video_crossfade` while the clicked one
        // was playing, stay faded unless `advance_pending_tile_reset` resets
        // all three tiles, not just the clicked one.
        use proteus_ui::BakedImage;

        let mut demo = advance_to_video_tiles();
        let tiles = demo.video_tiles.tiles;
        for &tile in &tiles {
            demo.app
                .world_mut()
                .entity_mut(tile.id())
                .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [400.0, 600.0]));
            if let Some(mut qs) = demo.app.world_mut().get_mut::<QuadState>(tile.id()) {
                qs.color = Vec4::ONE;
            }
        }

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        // Fine-grained ticks — matching this file's own convention — so the
        // fade actually observes the transition mid-flight; a single big
        // jump completes it before `advance_video_crossfade` ever runs and
        // never touches the siblings' alpha at all.
        let mut t = 0.0;
        while t < 1.0 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::VideoScreen(0));
        for &idx in &[1usize, 2] {
            assert_eq!(
                demo.app.get(demo.video_tiles.tiles[idx]).unwrap().opacity,
                0.0,
                "sanity: the non-playing tiles must actually be faded out at this point"
            );
        }

        demo.start_screen_to_tiles(0);
        demo.tick(0.05);
        demo.tick(0.05);
        assert_eq!(demo.state, AppState::VideoTiles);
        for &idx in &[1usize, 2] {
            let tile = demo.video_tiles.tiles[idx];
            assert_eq!(
                demo.app.get(tile).unwrap().geometry.color,
                Vec4::ONE,
                "tile {idx}'s own box art must be visible and untinted again after returning, \
                 not stuck at whatever alpha it faded to while tile 0 was playing"
            );
        }
    }

    // Complements `returning_from_video_screen_keeps_real_box_art_untinted`:
    // that test proves the *settled* steady state is correct (which
    // `advance_pending_tile_reset` alone guarantees); this one proves the
    // transition's own mid-flight bake *target* is correct too, which is what
    // is visible during the transition. `tile[idx]` is both the source and
    // one of the 3 targets of `start_screen_to_tiles`'s split (returning to
    // its own grid slot). Resolving that target's state from
    // `declared_geometry(tile)`, still screen-sized at this moment, would make
    // the virtual converging back into this slot animate toward the stale
    // screen shape and color instead of the real grid slot; hence
    // `split_to_with_states`.
    #[test]
    fn screen_to_tiles_virtual_targets_the_real_grid_shape_not_the_stale_screen_shape() {
        use bevy_ecs::query::With;
        use proteus_ui::{ActiveTransition, BakedImage, Virtual};

        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        demo.app
            .world_mut()
            .entity_mut(tile.id())
            .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [400.0, 600.0]));
        if let Some(mut qs) = demo.app.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color = Vec4::ONE;
        }

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoScreen(0));

        demo.start_screen_to_tiles(0);
        // One small tick: enough for `one_to_n_setup_system` to spawn the
        // virtuals, nowhere near enough to finish the 0.4s transition.
        demo.tick(0.02);

        // All 3 targets are tile-sized (only tile[0] has real box art in
        // this test, so its own virtual is the only one targeting white —
        // that's what uniquely picks it out among the 3).
        let expected_tile_size = Vec2::new(video_tiles::TILE_WIDTH, video_tiles::TILE_HEIGHT);
        let world = demo.app.world_mut();
        let mut query = world.query_filtered::<&ActiveTransition, With<Virtual>>();
        let virtual_targets: Vec<QuadState> = query.iter(world).map(|a| a.to.clone()).collect();
        assert_eq!(
            virtual_targets.len(),
            3,
            "start_screen_to_tiles should spawn exactly 3 virtuals"
        );
        let own_slot_target = virtual_targets
            .iter()
            .find(|to| to.color == Vec4::ONE)
            .expect("should find the one virtual targeting white (tile[0]'s own slot)");

        assert_eq!(
            own_slot_target.size, expected_tile_size,
            "the virtual returning to tile[0]'s own grid slot must target the real tile size, \
             not tile[0]'s own stale (still screen-sized) declared geometry"
        );
        assert_eq!(
            own_slot_target.color,
            Vec4::ONE,
            "and it must target white (real box art), not the placeholder tint"
        );
    }

    // `video_tiles.tiles[0..3]` are root entities at the same z, and
    // `collect_instances` breaks that tie by spawn order, not visual intent,
    // so the growing or settled video screen could draw *under* a sibling
    // tile spawned later, clipped by its footprint wherever they overlap.
    // Asserts the settled screen tile's own z has risen above its
    // still-tile-shaped siblings', not just that its geometry and color are
    // right.
    #[test]
    fn settled_video_screen_tile_draws_above_its_idle_siblings() {
        let mut demo = advance_to_video_tiles();
        let tile_z = |demo: &Harness, idx: usize| {
            demo.app
                .get(demo.video_tiles.tiles[idx])
                .unwrap()
                .geometry
                .position
                .z
        };
        let (z0, z1, z2) = (tile_z(&demo, 0), tile_z(&demo, 1), tile_z(&demo, 2));
        assert_eq!(z0, z1);
        assert_eq!(z1, z2);

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoScreen(0));

        assert!(
            tile_z(&demo, 0) > tile_z(&demo, 1),
            "the settled screen tile (z={}) must draw above its idle sibling (z={})",
            tile_z(&demo, 0),
            tile_z(&demo, 1)
        );
        assert_eq!(
            tile_z(&demo, 0),
            video_tiles::video_screen_quad(demo.viewport_size)
                .position
                .z,
            "should land exactly on video_screen_quad's own z, not something ad hoc"
        );
    }

    #[test]
    fn clicking_a_tile_queues_a_video_start_and_shows_the_video_texture() {
        let mut demo = advance_to_video_tiles();
        assert_eq!(demo.take_pending_video_start(), None);

        demo.start_tiles_to_screen(1);
        assert_eq!(demo.state, AppState::VideoScreen(1));
        assert_eq!(
            demo.take_pending_video_start(),
            Some(1),
            "should queue exactly the clicked tile's index for the shell to decode"
        );
        // Draining is destructive — a second read this same tick sees nothing.
        assert_eq!(demo.take_pending_video_start(), None);
    }

    // Starting playback must crossfade from the tile's box art to the video.
    // `Handle::show_video` alone cuts straight to the video (`video_t: 1.0`,
    // its documented default), so `Demo::start_tiles_to_screen` must set it
    // back to 0 and `Demo::advance_video_crossfade` must ramp it up in step
    // with the geometry transition.
    #[test]
    fn video_crossfades_in_from_box_art_alongside_the_geometry_morph() {
        use proteus_ui::VideoCrossfade;

        let mut demo = advance_to_video_tiles();
        let video_t = |demo: &Harness, idx: usize| {
            demo.app
                .world()
                .get::<VideoCrossfade>(demo.video_tiles.tiles[idx].id())
                .map(|c| c.video_t)
        };

        demo.start_tiles_to_screen(0);
        assert_eq!(
            video_t(&demo, 0),
            Some(0.0),
            "must start fully as box art, not cut straight to video"
        );

        // Partway through the ~0.4s transition — neither endpoint yet.
        demo.tick(0.2);
        let mid = video_t(&demo, 0).expect("still playing");
        assert!(
            mid > 0.0 && mid < 1.0,
            "should be ramping partway through the morph, got {mid}"
        );

        // Past the transition's duration — settled.
        demo.tick(1.0);
        assert_eq!(
            video_t(&demo, 0),
            Some(1.0),
            "should be fully video once the morph has settled"
        );
    }

    #[test]
    fn video_backdrop_tracks_and_stays_between_idle_and_tracked_tile_z_throughout() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();

        // Mid-transition: backdrop is visible and tracking from the very start —
        // `Demo::advance_video_crossfade` fades the clicked tile's own art
        // out *during* the transition too (not just once settled), so backdrop
        // has to be there to receive that fade from the first tick. Its z
        // must stay strictly between the idle siblings' fixed `TILE_Z` and
        // the tracked tile's own *current* (still-ramping) z throughout —
        // see `backdrop_quad`'s own doc for why a fixed z can't do this.
        let mut t = 0.0;
        while t < 0.6 {
            demo.tick(0.05);
            let idle_z = demo
                .app
                .get(demo.video_tiles.tiles[1])
                .unwrap()
                .geometry
                .position
                .z;
            let tile = demo.app.get(demo.video_tiles.tiles[0]).unwrap();
            let backdrop = demo.app.get(demo.video_tiles.backdrop).unwrap();
            assert!(
                backdrop.visible,
                "backdrop must be visible throughout, not just once settled"
            );
            assert!(
                backdrop.geometry.position.z > idle_z,
                "backdrop z ({}) must stay above the idle siblings' ({idle_z}) throughout",
                backdrop.geometry.position.z
            );
            assert!(
                backdrop.geometry.position.z < tile.geometry.position.z,
                "backdrop z ({}) must stay below the tracked tile's own current z ({})",
                backdrop.geometry.position.z,
                tile.geometry.position.z
            );
            t += 0.05;
        }
        assert_eq!(
            demo.app
                .get(demo.video_tiles.tiles[0])
                .unwrap()
                .geometry
                .color
                .w,
            0.0,
            "tile's own art must hide once settled with no frame yet, so it can't render in \
             front of the black backdrop"
        );
        let screen_geo = demo.app.get(demo.video_tiles.tiles[0]).unwrap().geometry;
        let backdrop_geo = demo.app.get(demo.video_tiles.backdrop).unwrap().geometry;
        assert_eq!(backdrop_geo.size, screen_geo.size);
        assert_eq!(backdrop_geo.position.x, screen_geo.position.x);
        assert_eq!(backdrop_geo.position.y, screen_geo.position.y);
    }

    #[test]
    fn other_two_tiles_fade_out_during_the_morph_and_are_fully_restored_on_return() {
        // The *other two* (non-clicked) tiles must fade out during the
        // transition, or they show over the transitioning tile. Also checks
        // that returning to VideoTiles restores them
        // (`advance_pending_tile_reset`): a split's reveal doesn't reset a
        // target's opacity, so they'd otherwise stay faded after the first
        // video.
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();

        // Partway through the transition: the other two tiles should already be
        // partly faded (fade reaches 0 at *half* the transition's own progress).
        demo.tick(0.15);
        for &idx in &[1usize, 2] {
            let tile = demo.app.get(demo.video_tiles.tiles[idx]).unwrap();
            assert!(
                tile.opacity < 1.0,
                "tile {idx} must already be fading out partway through the morph"
            );
        }

        // Fully settled: the other two must be fully transparent.
        let mut t = 0.0;
        while t < 0.4 {
            demo.tick(0.05);
            t += 0.05;
        }
        for &idx in &[1usize, 2] {
            let tile = demo.app.get(demo.video_tiles.tiles[idx]).unwrap();
            assert_eq!(
                tile.opacity, 0.0,
                "tile {idx} must be fully faded out once settled"
            );
        }

        // Back to the grid: every tile must be fully restored, not stuck at
        // whatever opacity the fade-out left it at. Restored at once, before
        // the split bakes each tile's end image at its current opacity:
        // baked faded out, the tiles would fade to nothing and then pop in.
        demo.start_screen_to_tiles(0);
        demo.app.refresh_cascades();
        for &idx in &[1usize, 2] {
            assert_eq!(
                demo.app.get(demo.video_tiles.tiles[idx]).unwrap().opacity,
                1.0,
                "tile {idx} is restored before the split bakes it"
            );
        }
        let mut t = 0.0;
        while t < 1.0 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::VideoTiles);
        for &tile in &demo.video_tiles.tiles {
            assert_eq!(
                demo.app.get(tile).unwrap().opacity,
                1.0,
                "tile's opacity must be fully restored"
            );
            let glow = demo.app.world().get::<Glow>(tile.id()).unwrap();
            assert_eq!(glow.radius, 0.0, "glow radius must be back at rest");
        }
    }

    #[test]
    fn settled_video_backdrop_draws_above_the_idle_sibling_tiles_not_just_below_the_screen() {
        // Once settled and waiting for the first real frame, the tile is
        // fully transparent (see `advance_video_loading`'s doc) and occludes
        // nothing, so whatever `backdrop` doesn't cover shows through. The two
        // idle sibling tiles sit well inside the settled screen's much bigger
        // footprint; unless `backdrop`'s own z is strictly *above* theirs, they
        // render in front of the video screen the user just opened. See
        // `backdrop_quad`'s doc for the mechanism.
        let mut demo = advance_to_video_tiles();
        let idle_z = demo
            .app
            .get(demo.video_tiles.tiles[1])
            .unwrap()
            .geometry
            .position
            .z;

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        let mut t = 0.0;
        while t < 0.6 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(
            demo.app
                .get(demo.video_tiles.tiles[0])
                .unwrap()
                .geometry
                .color
                .w,
            0.0,
            "sanity: must actually be in the transparent waiting state this test targets"
        );

        let backdrop_z = demo
            .app
            .get(demo.video_tiles.backdrop)
            .unwrap()
            .geometry
            .position
            .z;
        let screen_z = demo
            .app
            .get(demo.video_tiles.tiles[0])
            .unwrap()
            .geometry
            .position
            .z;
        assert!(
            backdrop_z > idle_z,
            "backdrop (z={backdrop_z}) must draw above the idle siblings (z={idle_z}), or they \
             show through the now-transparent screen tile"
        );
        assert!(
            backdrop_z < screen_z,
            "backdrop (z={backdrop_z}) must still draw below the settled screen tile itself \
             (z={screen_z}), so real content covers it once ready"
        );
    }

    #[test]
    fn video_screen_shows_loading_dots_after_a_short_delay_then_reveals_the_tile_once_ready() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        // Fine-grained ticks past the ~0.4s transition — settled, still waiting.
        // Coarser ticks would fold the whole grace period into the same
        // call that crosses the settle boundary, hiding the "not shown
        // immediately" window this test wants to observe.
        let mut t = 0.0;
        while t < 0.45 {
            demo.tick(0.05);
            t += 0.05;
        }

        // Just settled: too soon for the dots (VIDEO_DOT_SHOW_DELAY_SECS
        // grace period hasn't elapsed since settling).
        for &dot in &demo.video_tiles.loading_dots {
            assert!(
                !demo.app.get(dot).unwrap().visible,
                "dots must not flash immediately on settling"
            );
        }

        let mut t = 0.0;
        while t < video_tiles::VIDEO_DOT_SHOW_DELAY_SECS + 0.15 {
            demo.tick(0.05);
            t += 0.05;
        }
        for &dot in &demo.video_tiles.loading_dots {
            assert!(
                demo.app.get(dot).unwrap().visible,
                "dots must show once the grace period elapses"
            );
            let alpha = demo.app.get(dot).unwrap().geometry.color.w;
            assert!(
                (video_tiles::VIDEO_DOT_ALPHA_MIN..=video_tiles::VIDEO_DOT_ALPHA_MAX)
                    .contains(&alpha),
                "dot alpha {alpha} must stay within the pulse's own min/max range"
            );
        }
        assert!(!demo.app.get(demo.video_tiles.error_text).unwrap().visible);

        // First frame lands: tile's real content reappears, dots stop.
        demo.set_video_first_frame_shown();
        demo.tick(0.05);
        assert_eq!(
            demo.app
                .get(demo.video_tiles.tiles[0])
                .unwrap()
                .geometry
                .color
                .w,
            1.0,
            "tile must be fully visible again once the first real frame lands"
        );
        for &dot in &demo.video_tiles.loading_dots {
            assert!(
                !demo.app.get(dot).unwrap().visible,
                "dots must hide once ready"
            );
        }
    }

    #[test]
    fn video_screen_shows_an_error_after_the_load_timeout_elapses_with_no_frame() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();

        // `video_dots_elapsed` (the timeout clock) runs from click time, not
        // from settling — so the whole budget is `VIDEO_LOAD_TIMEOUT_SECS`
        // from here, independent of however long the transition itself took.
        demo.tick(video_tiles::VIDEO_LOAD_TIMEOUT_SECS - 0.5);
        assert!(
            !demo.app.get(demo.video_tiles.error_text).unwrap().visible,
            "must not show the error before the timeout actually elapses"
        );

        demo.tick(1.0); // now past VIDEO_LOAD_TIMEOUT_SECS
        assert!(demo.app.get(demo.video_tiles.error_text).unwrap().visible);
        for &dot in &demo.video_tiles.loading_dots {
            assert!(
                !demo.app.get(dot).unwrap().visible,
                "dots must be replaced by the error text"
            );
        }
    }

    #[test]
    fn video_load_timeout_fires_pending_video_cancel_exactly_once() {
        // Locks in the edge-triggered "exactly once, right when the timeout
        // first latches" contract its own doc promises, not just a level that
        // stays true forever after.
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();

        demo.tick(video_tiles::VIDEO_LOAD_TIMEOUT_SECS - 0.5);
        assert!(
            !demo.take_pending_video_cancel(),
            "must not fire before the timeout actually elapses"
        );

        demo.tick(1.0); // now past VIDEO_LOAD_TIMEOUT_SECS
        assert!(
            demo.take_pending_video_cancel(),
            "must fire the instant it does"
        );
        assert!(
            !demo.take_pending_video_cancel(),
            "must not fire again on a second drain the same tick"
        );

        demo.tick(1.0);
        assert!(
            !demo.take_pending_video_cancel(),
            "must not keep firing on every subsequent tick"
        );
    }

    #[test]
    fn leaving_video_screen_while_still_loading_restores_the_tiles_alpha_before_the_split() {
        // Backing out of a still-loading video must not leave the tile's
        // alpha at 0 in the outgoing split's "from" snapshot — see
        // `start_screen_to_tiles`'s own doc for why.
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0); // settled, still waiting — tile alpha is 0 here

        demo.start_screen_to_tiles(0);
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoTiles);
        for &tile in &demo.video_tiles.tiles {
            assert_eq!(
                demo.app.get(tile).unwrap().geometry.color.w,
                1.0,
                "every tile must be fully visible again back on the grid, even one that was \
                 still mid-load when backed out of"
            );
        }
    }

    #[test]
    fn leaving_video_screen_to_tiles_queues_a_stop_and_settles_back_to_the_grid() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        // Drain, as `DemoApp` would.
        demo.take_pending_video_start();
        // Let the "grow to screen" transition actually finish — in the real,
        // continuously-ticking app there are many frames between clicking a
        // tile and later clicking "back"; skipping this leaves a stale,
        // unprocessed `TransitionRequest` (targeting the screen size)
        // sitting on the tile that would otherwise get applied *after*
        // `start_screen_to_tiles`, fighting the group transition below.
        demo.tick(1.0);
        assert_eq!(
            demo.app
                .get(demo.video_tiles.tiles[0])
                .unwrap()
                .geometry
                .size,
            video_tiles::video_screen_quad(demo.viewport_size).size,
            "sanity check: the tile should be screen-sized before backing out"
        );

        demo.start_screen_to_tiles(0);
        assert_eq!(demo.state, AppState::VideoTiles);
        assert!(demo.take_pending_video_stop());
        // First tick after `split_to`: a small dt, matching a real frame —
        // long enough for `one_to_n_setup_system` to hide the tile and
        // capture its (still screen-sized) live geometry as the group
        // transition's "from" state, but short enough that the transition
        // itself (0.4s) hasn't completed within this same tick. A single
        // large `tick(1.0)` here (which the real app never does) collapses
        // setup/animate/complete into one call, so the tile is never
        // *observably* hidden between two ticks — exactly the window
        // `advance_pending_tile_reset` needs (see `PendingTileReset`'s doc) to
        // safely reset the geometry.
        demo.tick(0.02);
        assert!(
            !demo.app.get(demo.video_tiles.tiles[0]).unwrap().visible,
            "sanity check: the tile should be hidden mid-transition"
        );
        demo.tick(1.0); // finish the rest of the group transition's duration
        for &tile in &demo.video_tiles.tiles {
            assert!(
                demo.app.get(tile).unwrap().visible,
                "all 3 tiles should be visible again back on VideoTiles"
            );
        }
        // The played tile's own geometry must have shrunk back to the tile
        // shape, not stayed at whatever `start_tiles_to_screen`'s
        // `animate_to` left it at — group-transition reveals only flip
        // `Visibility`, they never resync a target's live `QuadState` back
        // to its declared geometry (see `start_screen_to_tiles`'s doc).
        let size = demo
            .app
            .get(demo.video_tiles.tiles[0])
            .unwrap()
            .geometry
            .size;
        assert_eq!(
            size,
            Vec2::new(video_tiles::TILE_WIDTH, video_tiles::TILE_HEIGHT),
            "the played tile must settle back to the grid tile's own size, not the video screen's"
        );
    }

    #[test]
    fn leaving_video_screen_straight_to_home_hides_every_tile() {
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(2);
        demo.take_pending_video_start();

        demo.start_screen_to_home(2);
        assert_eq!(demo.state, AppState::Home);
        assert!(demo.take_pending_video_stop());
        demo.tick(1.0);
        for (i, &tile) in demo.video_tiles.tiles.iter().enumerate() {
            assert!(
                !demo.app.get(tile).unwrap().visible,
                "tile {i} must not be visible on Home"
            );
        }
        for &tile in &demo.home.tiles {
            assert!(demo.app.get(tile).unwrap().visible);
        }
    }

    fn advance_to_home() -> Harness {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);
        demo
    }

    #[test]
    fn clicking_the_gallery_tile_opens_loading_and_the_home_icon_returns_home() {
        let mut demo = advance_to_home();

        let button = demo.home.tiles[1];
        let probe = demo.app.get(button).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        demo.pointer_released();
        assert_eq!(demo.state, AppState::Loading);

        let home_button = demo.nav.home;
        let probe = demo.app.get(home_button).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Home);
    }

    #[test]
    fn opening_loading_queues_a_fetch_request_and_hides_the_home_tiles() {
        let mut demo = advance_to_home();
        demo.start_home_to_loading();
        assert_eq!(demo.state, AppState::Loading);
        let request = demo
            .take_pending_gallery_fetch()
            .expect("start_home_to_loading should queue a fetch request");
        assert!(request.tile_side_px > 0);
        // Draining is destructive — a second read this same tick sees nothing.
        assert!(demo.take_pending_gallery_fetch().is_none());

        demo.tick(1.0);
        for &tile in &demo.home.tiles {
            assert!(
                !demo.app.get(tile).unwrap().visible,
                "Home's tiles should be hidden once merged into the loading logo"
            );
        }
        assert!(demo.app.get(demo.loading.logo).unwrap().visible);
    }

    #[test]
    fn loading_shows_an_error_after_the_fetch_timeout_elapses_without_all_tiles_ready() {
        let mut demo = advance_to_home();
        demo.start_home_to_loading();
        demo.tick(1.0); // settle the incoming merge — see `advance_gallery_fetch`'s "settled" gate

        assert!(!demo.app.get(demo.loading.error_text).unwrap().visible);
        // No `set_gallery_tile_image` calls at all — nothing ever bakes, so
        // this can only resolve via the timeout, never the happy path.
        let mut t = 0.0;
        while t < GALLERY_FETCH_TIMEOUT_SECS + 1.0 {
            demo.tick(0.1);
            t += 0.1;
        }
        assert_eq!(
            demo.state,
            AppState::Loading,
            "must not auto-advance to Gallery without every tile baked"
        );
        assert!(
            demo.app.get(demo.loading.error_text).unwrap().visible,
            "should show the error text once the timeout fires"
        );
        // The spinning logo must stop once the error text shows, since the
        // text sits dead center where the logo would otherwise show through.
        assert_eq!(
            demo.app.get(demo.loading.logo).unwrap().geometry.color.w,
            0.0,
            "the logo must fade out once the error text takes its place"
        );
    }

    // Reaches `Gallery` by calling `start_loading_to_gallery` directly,
    // bypassing the "every tile baked" auto-advance gate — same pattern as
    // `advance_to_video_tiles` calling `start_home_to_tiles` directly.
    // `Handle::baked_image_size` can never return `Some` in this headless
    // test world (baking needs a real `QuadPipeline`/GPU — see
    // `Demo::set_gallery_tile_image`'s doc), so the auto-advance path
    // itself is covered separately by the timeout/error test above, not by
    // actually satisfying the gate here.
    fn advance_to_gallery() -> Harness {
        let mut demo = advance_to_home();
        demo.start_home_to_loading();
        demo.take_pending_gallery_fetch();
        demo.tick(1.0);
        demo.start_loading_to_gallery();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Gallery);
        demo
    }

    #[test]
    fn gallery_tiles_and_enlarged_blend_their_border_color_with_the_theme() {
        use proteus_ui::Border;

        let mut demo = advance_to_gallery();
        demo.dark_target = true;
        let mut t = 0.0;
        while t < THEME_MORPH_DURATION_SECS + 0.5 {
            demo.tick(0.1);
            t += 0.1;
        }
        assert_eq!(demo.theme_progress, 1.0);

        let dark = violet_dark();
        let tile_border = demo
            .app
            .world()
            .get::<Border>(demo.gallery.tiles[0].id())
            .unwrap()
            .color;
        assert!(
            (tile_border.x - dark.x).abs() < 0.01 && (tile_border.y - dark.y).abs() < 0.01,
            "gallery tile border should blend to the dark-theme violet, got {tile_border:?}"
        );
        let enlarged_border = demo
            .app
            .world()
            .get::<Border>(demo.gallery.enlarged.id())
            .unwrap()
            .color;
        assert!(
            (enlarged_border.x - dark.x).abs() < 0.01,
            "gallery.enlarged's border should blend too, got {enlarged_border:?}"
        );
    }

    #[test]
    fn hovering_a_gallery_tile_ramps_glow_and_scale() {
        use proteus_ui::Glow;

        let mut demo = advance_to_gallery();
        let tile = demo.gallery.tiles[0];
        let probe = demo.app.get(tile).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        let mut t = 0.0;
        while t < screens::styles::SECS + 0.5 {
            demo.tick(0.05);
            t += 0.05;
        }

        let glow_radius = demo.app.world().get::<Glow>(tile.id()).unwrap().radius;
        assert!(
            (glow_radius - screens::styles::GLOW_PX).abs() < 0.01,
            "fully-hovered tile should reach the max glow radius, got {glow_radius}"
        );
        let scale = demo.app.get(tile).unwrap().geometry.scale;
        assert!(
            (scale - screens::styles::SCALE).abs() < 0.01,
            "fully-hovered tile should also scale up, got {scale}"
        );
    }

    // `gallery.enlarged`'s style is deliberately a glow alone: it's as big
    // as the grid's space allows, so growing would read as a wobble (see
    // `screens::styles::glow_only`). Unlike all the other lit surfaces in
    // this crate, and not an oversight to "fix".
    #[test]
    fn hovering_the_enlarged_image_ramps_glow_but_never_scales() {
        use proteus_ui::Glow;

        let mut demo = advance_to_gallery_image(0);
        let enlarged = demo.gallery.enlarged;
        let probe = demo.app.get(enlarged).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        let mut t = 0.0;
        while t < screens::styles::SECS + 0.5 {
            demo.tick(0.05);
            t += 0.05;
        }

        let glow_radius = demo.app.world().get::<Glow>(enlarged.id()).unwrap().radius;
        assert!(
            (glow_radius - screens::styles::GLOW_PX).abs() < 0.01,
            "fully-hovered enlarged image should still reach the max glow radius, got {glow_radius}"
        );
        assert_eq!(
            demo.app.get(enlarged).unwrap().geometry.scale,
            1.0,
            "but must never scale up, unlike every other lit surface"
        );
    }

    // "Fetch New Images" must fade in and out rather than pop: `Visibility`
    // alone would show and hide it at once.
    #[test]
    fn gallery_fetch_button_fades_in_and_out_instead_of_popping() {
        let opacity = |demo: &Harness| demo.app.get(demo.gallery.fetch_button).unwrap().opacity;

        let mut demo = advance_to_gallery();
        assert_eq!(
            opacity(&demo),
            1.0,
            "sanity check: fully faded in once settled in Gallery"
        );

        demo.start_gallery_to_home();
        assert_eq!(demo.state, AppState::Home);
        // Partway through the fade-out (well under the 0.6s duration) —
        // must still be visible (fading), not already popped away.
        demo.tick(0.1);
        assert!(
            demo.app.get(demo.gallery.fetch_button).unwrap().visible,
            "must still be visible while mid-fade-out"
        );
        let mid = opacity(&demo);
        assert!(
            mid > 0.0 && mid < 1.0,
            "should be partway faded out, got {mid}"
        );

        // Past the fade's duration — fully gone.
        demo.tick(1.0);
        assert!(!demo.app.get(demo.gallery.fetch_button).unwrap().visible);
        assert_eq!(opacity(&demo), 0.0);
    }

    #[test]
    fn clicking_fetch_new_images_refetches_and_hides_the_button() {
        let mut demo = advance_to_gallery();
        let first_generation = demo.gallery_fetch_generation;

        let button = demo.gallery.fetch_button;
        let probe = demo.app.get(button).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Loading);
        assert_eq!(
            demo.gallery_fetch_generation,
            first_generation.wrapping_add(1)
        );
        assert!(demo.take_pending_gallery_fetch().is_some());
        assert!(!demo.app.get(demo.gallery.fetch_button).unwrap().visible);
        // The click itself, `advance_nav_click`'s dispatch, and
        // `merge_from`'s request insert all happen within the tick above —
        // but (like `split_to`) the request is only *processed* (hiding the
        // merge's sources) on the *next* tick's schedule run. See
        // `PendingTileReset`'s doc for the identical reasoning on the
        // `split_to` side.
        demo.tick(0.05);
        for &tile in &demo.gallery.tiles {
            assert!(!demo.app.get(tile).unwrap().visible);
        }
    }

    #[test]
    fn gallery_to_home_converges_every_tile_and_hides_the_fetch_button() {
        let mut demo = advance_to_gallery();
        demo.start_gallery_to_home();
        assert_eq!(demo.state, AppState::Home);
        demo.tick(1.0);
        for &tile in &demo.home.tiles {
            assert!(demo.app.get(tile).unwrap().visible);
        }
        for &photo in &demo.gallery.tiles {
            assert!(!demo.app.get(photo).unwrap().visible);
        }
        assert!(!demo.app.get(demo.gallery.fetch_button).unwrap().visible);
        assert!(
            !demo
                .app
                .get(demo.gallery.fetch_button_label)
                .unwrap()
                .visible
        );
    }

    #[test]
    fn clicking_a_tile_opens_gallery_image_and_queues_a_hires_fetch() {
        let mut demo = advance_to_gallery();

        let tile = demo.gallery.tiles[3];
        let probe = demo.app.get(tile).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::GalleryImage(3));

        let request = demo
            .take_pending_gallery_hires_fetch()
            .expect("should queue a hires fetch for the clicked tile");
        assert_eq!(request.idx, 3);
        // Uncapped, logical pixels — see `Demo::start_gallery_to_image`'s
        // doc for why the physical-pixel cap is `DemoApp`'s job, not this
        // one's.
        assert!(request.width_px > 0 && request.height_px > 0);
        // Draining is destructive — a second read this same tick sees nothing.
        assert!(demo.take_pending_gallery_hires_fetch().is_none());

        assert!(!demo.app.get(demo.gallery.fetch_button).unwrap().visible);
    }

    #[test]
    fn hires_fetch_request_preserves_the_boxs_exact_aspect_ratio() {
        let mut demo = advance_to_gallery();
        // A deliberately awkward ratio (not a clean fraction of the cap) —
        // exactly the kind of value where rounding each axis independently
        // drifts the requested image's aspect away from the box's, and
        // rounding only the larger axis then deriving the other doesn't.
        demo.gallery_tile_aspect[5] = Vec2::new(1920.0, 1103.0);

        demo.start_gallery_to_image(5);
        let request = demo.take_pending_gallery_hires_fetch().unwrap();
        let box_size = demo.app.get(demo.gallery.enlarged).unwrap().geometry.size;

        let requested_ratio = request.width_px as f32 / request.height_px as f32;
        let box_ratio = box_size.x / box_size.y;
        assert!(
            (requested_ratio - box_ratio).abs() < 0.001,
            "requested {}x{} (ratio {requested_ratio}) must match the box's own ratio {box_ratio} \
             almost exactly, not just approximately",
            request.width_px,
            request.height_px,
        );
    }

    #[test]
    fn enlarged_view_contain_fits_the_photos_real_aspect_not_the_tiles_own_square_crop() {
        let mut demo = advance_to_gallery();
        // A portrait (2:3) photo — the grid tile's own baked image is by
        // now a center-cropped square (see `Demo::advance_gallery_tile_crop`'s
        // doc); `gallery_tile_aspect` is what `start_gallery_to_image` must
        // actually use for the *geometry*, independent of that crop.
        demo.gallery_tile_aspect[0] = Vec2::new(2.0, 3.0);

        demo.start_gallery_to_image(0);

        let size = demo.app.get(demo.gallery.enlarged).unwrap().geometry.size;
        assert!(
            size.y > size.x,
            "a portrait (2:3) photo must render taller than wide, not square: got {size:?}"
        );
        assert!(
            (size.x / size.y - 2.0 / 3.0).abs() < 0.01,
            "must preserve the photo's exact 2:3 ratio: got {size:?}"
        );
    }

    #[test]
    fn tile_crop_stashes_the_uncropped_frame_before_cropping_the_tiles_own_copy() {
        use proteus_ui::BakedImage;

        let mut demo = advance_to_gallery();
        let tile = demo.gallery.tiles[0];
        let tile_full = demo.gallery.tile_full[0];

        demo.set_gallery_tile_image(0, vec![0u8; 4], Vec2::new(2.0, 3.0));
        // Simulate the renderer's bake pass landing a real (uncropped, 2:3)
        // image on the tile — real baking needs GPU, unavailable here.
        let uncropped = BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [200.0, 300.0]);
        demo.app
            .world_mut()
            .entity_mut(tile.id())
            .insert(uncropped.clone());

        demo.tick(0.016);

        let cropped = demo.app.world().get::<BakedImage>(tile.id()).unwrap();
        assert!(
            cropped.uv_scale[1] < uncropped.uv_scale[1],
            "the tile's own copy must end up center-cropped (narrower V axis for a portrait photo)"
        );
        let stashed = demo.app.world().get::<BakedImage>(tile_full.id()).unwrap();
        assert_eq!(
            stashed, &uncropped,
            "tile_full must hold the original, uncropped frame — untouched by the crop"
        );
    }

    fn advance_to_gallery_image(idx: usize) -> Harness {
        let mut demo = advance_to_gallery();
        demo.start_gallery_to_image(idx);
        demo.take_pending_gallery_hires_fetch();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::GalleryImage(idx));
        demo
    }

    #[test]
    fn clicking_the_enlarged_image_returns_to_gallery_and_cancels_the_hires_fetch() {
        let mut demo = advance_to_gallery_image(5);

        let enlarged = demo.gallery.enlarged;
        let probe = demo.app.get(enlarged).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::Gallery);
        assert!(demo.take_pending_gallery_hires_cancel());
        assert!(demo.take_pending_gallery_hires_fetch().is_none());

        demo.tick(1.0);
        for &tile in &demo.gallery.tiles {
            assert!(demo.app.get(tile).unwrap().visible);
        }
        assert!(!demo.app.get(enlarged).unwrap().visible);
    }

    #[test]
    fn going_home_from_gallery_image_cancels_the_hires_fetch_and_hides_the_enlarged_image() {
        let mut demo = advance_to_gallery_image(7);
        demo.start_image_to_home();
        assert_eq!(demo.state, AppState::Home);
        assert!(demo.take_pending_gallery_hires_cancel());

        demo.tick(1.0);
        for &tile in &demo.home.tiles {
            assert!(demo.app.get(tile).unwrap().visible);
        }
        assert!(!demo.app.get(demo.gallery.enlarged).unwrap().visible);
        for &tile in &demo.gallery.tiles {
            assert!(
                !demo.app.get(tile).unwrap().visible,
                "the grid was never revealed on the way to Home, so it must stay hidden"
            );
        }
    }

    #[test]
    fn set_gallery_hires_image_ignores_a_stale_index() {
        let mut demo = advance_to_gallery_image(2);
        // idx 4 isn't the currently-enlarged tile (2 is) — e.g. a late
        // result for an abandoned fetch arriving after the user already
        // moved on. Must be a no-op, not applied to the wrong photo.
        demo.set_gallery_hires_image(4, vec![0u8; 4]);
        assert!(demo
            .app
            .world()
            .get::<Image>(demo.gallery.hires_overlay.id())
            .is_none());

        demo.set_gallery_hires_image(2, vec![0u8; 4]);
        assert!(demo
            .app
            .world()
            .get::<Image>(demo.gallery.hires_overlay.id())
            .is_some());
    }

    #[test]
    fn hires_overlay_stays_hidden_until_baked_and_settled_then_crossfades_in() {
        use proteus_ui::BakedImage;

        let mut demo = advance_to_gallery_image(1);
        // No bake yet (a real one needs GPU — unavailable in this headless
        // test world, see `Handle::baked_image_size`'s doc) — must stay
        // hidden and unfaded regardless of how long this ticks, matching
        // the exact bug report this crossfade fixed: the overlay must
        // never be the *only* thing shown (`gallery.enlarged`'s own
        // low-res image stays put underneath the whole time).
        demo.tick(1.0);
        assert!(!demo.app.get(demo.gallery.hires_overlay).unwrap().visible);
        assert_eq!(demo.gallery_hires_fade, 0.0);

        // Simulate the renderer's bake pass landing a real image on
        // the overlay (what `set_gallery_hires_image` + a real
        // `bake_pending_images` call would produce together).
        let overlay = demo.gallery.hires_overlay;
        demo.app
            .world_mut()
            .entity_mut(overlay.id())
            .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [800.0, 600.0]));

        demo.tick(GALLERY_HIRES_CROSSFADE_DURATION_SECS / 2.0);
        assert!(
            demo.gallery_hires_fade > 0.0 && demo.gallery_hires_fade < 1.0,
            "should be mid-crossfade"
        );
        assert!(demo.app.get(demo.gallery.hires_overlay).unwrap().visible);
        assert_eq!(
            demo.app
                .get(demo.gallery.hires_overlay)
                .unwrap()
                .geometry
                .size,
            demo.app.get(demo.gallery.enlarged).unwrap().geometry.size,
            "must stay glued to enlarged's own geometry"
        );

        demo.tick(GALLERY_HIRES_CROSSFADE_DURATION_SECS);
        assert_eq!(
            demo.gallery_hires_fade, 1.0,
            "should have finished crossfading in"
        );
    }

    #[test]
    fn enlarged_box_never_moves_once_the_hires_bake_lands_even_if_its_aspect_disagrees() {
        use proteus_ui::BakedImage;

        // Nothing may touch `enlarged`'s box after `start_gallery_to_image` sets
        // it. On some photos the low-res stand-in and the hires bake disagree on
        // the photo's exact aspect ratio by a hair, since picsum center-crops each
        // fetch to the integer size it was asked for, and the two fetches round
        // the same real aspect at very different resolutions. Re-deriving the box
        // from the hires bake's decoded aspect when it lands would make any such
        // disagreement a visible jump, on exactly the photo the user is looking at.
        // This asserts the box stays put, and only `gallery_hires_fade`/
        // `Visibility` change.
        let mut demo = advance_to_gallery_image(4);
        let size_before = demo.app.get(demo.gallery.enlarged).unwrap().geometry.size;

        // `gallery_tile_aspect[4]` is still the default `Vec2::ONE`
        // (square) — the hires bake's real shape (800x500, 8:5) disagrees
        // with it about as sharply as two independent roundings ever
        // could, making this the worst case for re-deriving the box.
        let overlay = demo.gallery.hires_overlay;
        demo.app
            .world_mut()
            .entity_mut(overlay.id())
            .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [800.0, 500.0]));

        demo.tick(0.001);

        let size_after = demo.app.get(demo.gallery.enlarged).unwrap().geometry.size;
        assert_eq!(
            size_before, size_after,
            "enlarged's box must never move/resize once built, no matter what the hires bake's \
             own real aspect turns out to be — that resize was the shift's own cause, not a fix \
             for it"
        );
    }

    #[test]
    fn reaching_home_from_splash_lays_the_tiles_out_in_a_grid() {
        // Reaches `Home` the normal way (ticking through `Splash`, no manual
        // geometry pokes), as the real app is driven, and confirms the tiles
        // end up where `home::layout` puts them.
        let mut demo = advance_to_home();
        demo.tick(1.0);
        let expected = home::layout(demo.viewport_size);
        for (i, &tile) in demo.home.tiles.iter().enumerate() {
            let position = demo.app.get(tile).unwrap().geometry.position;
            assert_eq!(position, expected[i].position, "tile {i}");
        }
    }

    #[test]
    fn clicking_moon_icon_ramps_theme_progress_and_swaps_the_active_toggle_icon() {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);
        assert_eq!(demo.theme_progress, 0.0);
        assert!(
            !demo.dark_target,
            "should start light — moon (not sun) is the clickable one"
        );

        let moon = demo.theme.moon;
        let probe = demo.app.get(moon).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        demo.pointer_pressed();
        demo.tick(0.01);
        demo.pointer_released();

        assert!(demo.dark_target, "clicking moon must set dark_target");
        assert!(
            demo.theme_progress > 0.0,
            "theme_progress should have started ramping toward dark"
        );

        let mut t2 = 0.0;
        while t2 < THEME_MORPH_DURATION_SECS + 0.5 {
            demo.tick(0.1);
            t2 += 0.1;
        }
        assert_eq!(
            demo.theme_progress, 1.0,
            "should have finished ramping to dark"
        );
        assert_eq!(
            demo.app.get(demo.background.dark).unwrap().geometry.color.w,
            1.0,
            "background_dark should be fully opaque once theme_progress reaches 1"
        );
        assert_eq!(
            demo.app
                .get(demo.loading.logo_dark)
                .unwrap()
                .geometry
                .color
                .w,
            1.0,
            "loading.logo_dark should be fully opaque too, same theme-progress crossfade"
        );
    }

    // Layout's panel says when the layout system is coming.
    #[test]
    fn the_layout_panel_says_its_coming_in_v0_6_0() {
        let mut demo = advance_to_home();
        demo.start_home_to_detail(3);
        demo.tick(1.0);
        let message = demo.example_detail.layout_message;
        assert!(demo.app.get(message).unwrap().visible);
        let text = demo.app.world().get::<Text>(message.id()).unwrap();
        assert!(text.content.contains("v0.6.0"), "got {:?}", text.content);
    }

    // `nav.lockup` must be `.non_interactive()`. Its bounding box (the whole
    // `lockup.png`, including the empty space past the visible wordmark —
    // see `nav::LOGO_TEXT_RIGHT_PX`'s doc) overlaps `nav.home`'s hit region,
    // and it's created later, so it would win clicks there and silently eat
    // clicks meant for `home`.
    #[test]
    fn nav_lockup_is_not_a_click_target() {
        use proteus_ui::Interactable;

        let demo = Harness::new();
        assert!(
            demo.app
                .world()
                .get::<Interactable>(demo.nav.lockup.id())
                .is_none(),
            "nav.lockup is decorative brand chrome, not a click target"
        );
    }

    // -----------------------------------------------------------------------
    // Viewport changes reach the demo
    // -----------------------------------------------------------------------

    // A `HostServices` that provides nothing — enough to build a `Frame` and
    // drive `DemoApp::update`, which is all the viewport test needs. Asset
    // loading, fetching and video are exercised elsewhere.
    struct NullServices;

    impl proteus_runtime::HostServices for NullServices {
        fn load_asset(&mut self, _key: &str) -> Option<std::sync::Arc<[u8]>> {
            None
        }
        fn fetch_async(&mut self, _key_or_url: &str) -> proteus_runtime::FetchId {
            proteus_runtime::FetchId(0)
        }
        fn poll_fetches(&mut self) -> Vec<proteus_runtime::FetchResult> {
            Vec::new()
        }
        fn cancel_fetch(&mut self, _id: proteus_runtime::FetchId) {}
    }

    // `Engine::resize` keeps `Frame::viewport` current, and `DemoApp` must
    // pass changes on to `Demo`. Reading the viewport only in `setup` would
    // leave the demo laid out for whatever size it opened at: background stuck
    // at its startup size, nav and theme icons pinned to the old corners.
    //
    // Drives the real `App::update` through a real `Frame` rather than calling
    // `Demo::set_viewport_size` directly — the forwarding *is* the thing under
    // test, and calling the setter would pass no matter what `update` does.
    #[test]
    fn resizing_the_viewport_relays_out_the_demo() {
        use proteus_runtime::{App, Frame, Viewport};

        let mut app = Proteus::new();
        let mut demo_app = DemoApp::new(None);
        let mut services = NullServices;

        let initial = Vec2::new(1280.0, 800.0);
        {
            let mut frame = Frame {
                proteus: &mut app,
                services: &mut services,
                viewport: Viewport::new(initial, 1.0),
            };
            demo_app.setup(&mut frame);
        }

        let background = demo_app
            .demo_mut()
            .expect("setup ran")
            .background
            .light
            .id();
        let size_of = |app: &Proteus| {
            app.world()
                .get::<QuadState>(background)
                .expect("background exists")
                .size
        };
        assert_eq!(
            size_of(&app),
            initial,
            "setup should apply the initial size"
        );

        // Resize, then run one ordinary frame.
        let resized = Vec2::new(1920.0, 1080.0);
        {
            let mut frame = Frame {
                proteus: &mut app,
                services: &mut services,
                viewport: Viewport::new(resized, 1.0),
            };
            demo_app.update(&mut frame, 0.016);
        }
        assert_eq!(
            size_of(&app),
            resized,
            "a changed viewport must reach Demo::set_viewport_size on the next frame"
        );

        // And a frame at the same size must not re-apply it — `set_viewport_size`
        // rewrites every gallery tile's *declared* geometry, which would clobber
        // a tile mid-transition if it ran every frame.
        let tile = demo_app.demo_mut().expect("demo").gallery.tiles[0];
        let marker = QuadState {
            position: glam::Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        let _ = tile.set_declared_geometry(&mut app, marker.clone());
        {
            let mut frame = Frame {
                proteus: &mut app,
                services: &mut services,
                viewport: Viewport::new(resized, 1.0),
            };
            demo_app.update(&mut frame, 0.016);
        }
        assert_eq!(
            app.world().get::<QuadState>(tile.id()).map(|q| q.position),
            Some(marker.position),
            "an unchanged viewport must not re-run the relayout"
        );
    }
}
