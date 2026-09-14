//! `proteus-demo` — the shared, shell-agnostic Proteus reference demo
//! (M12.5).
//!
//! Built once against [`proteus_sdk::Proteus`] and linked by both
//! `proteus-shell-native` and `proteus-shell-web`, replacing what was
//! previously ~17,500 lines of independently hand-duplicated demo logic
//! across the two shells. Content lands screen by screen across M12.5's
//! staged migration (see `PLANNING.md`'s M12.5 entry). Currently live: a
//! persistent background image and nav chrome, `Splash` (real animated logo,
//! wordmark, intro fade/slide-in — see `screens::splash`'s doc) → `Home`
//! (still placeholder colors), which reaches `ExamplesHome` →
//! `ExampleDetail` (Effects/Text/Transforms & Animation/Stress Tests — see
//! `screens::example_detail`'s doc for what's still out of scope beyond
//! that), `VideoTiles` → `VideoScreen` (box-cover art tiles that grow into
//! real `.mp4` playback — see `screens::video_tiles`'s doc for what's
//! deferred there: loading-state UI and morph-time crossfade polish), and
//! `Loading` → `Gallery` → `GalleryImage` (a real 12-image fetch, plus a
//! real hires fetch for whichever one is enlarged — see `screens::gallery`'s
//! doc for what's deferred there: center-cropping fetched photos and the
//! hires upgrade's crossfade).
//!
//! ## What stays a shell concern
//!
//! Rendering (GPU device/surface setup, `collect_instances`, the actual
//! draw call, and baking `Text`/`Image` components into the GPU atlas) is
//! **not** this crate's job — `proteus-sdk` itself is headless, and this
//! crate follows suit. A shell drives [`Demo`] with [`Demo::tick`]/pointer
//! input, then reads [`Demo::app`]'s `world()` to render. `examples/
//! native_preview.rs` is a minimal reference for how to wire this up
//! (including the text-baking step) — not part of this crate's public API,
//! just a `cargo run --example native_preview -p proteus-demo` harness for
//! visually confirming each migration step as content lands.
//!
//! Per-platform asset loading (reading files from disk, `fetch()`-ing
//! images, decoding video) also stays a shell concern — later steps add
//! `set_*`/`take_*` injection points to [`Demo`] (mirroring
//! `proteus-shell-web`'s existing wasm-bindgen surface, generalized) that
//! each shell calls with bytes/frames it fetched its own way.

mod app;
mod gallery_fetch;
mod screens;

pub use app::DemoApp;

use std::cell::Cell;
use std::rc::Rc;

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{
    ease_in_out_quad, ease_out_quad, Border, ComponentSpec, Glow, Handle, Image, MergeLayout,
    Proteus, QuadState, SplitStrategy, Text, TextureHandle, TransitionConfig, Visibility,
};

use screens::{
    background, example_detail, examples_home, gallery, home, loading, nav, splash, theme,
    video_tiles,
};

/// Initial background/viewport size in logical pixels, used only until the
/// shell's first [`Demo::set_viewport_size`] call — see that method's doc.
/// Matches `examples/native_preview.rs`'s own default window size, so the
/// harness never actually shows this placeholder in practice.
const DEFAULT_VIEWPORT_SIZE: Vec2 = Vec2::new(1280.0, 800.0);

/// Design-System hover constants, shared by every interactive surface via
/// [`Demo::advance_hovers`] — see [`HoverEntry`]'s doc for the mechanism.
/// Mirrors `proteus-shell-native::GLOW_DURATION`/`GLOW_MAX_RADIUS`/
/// `HOVER_SCALE_BOOST` exactly (that file's own doc comment on the last one
/// says "5%"; the constant is actually 7% there too — matching the value,
/// not the stale comment).
const HOVER_GLOW_DURATION_SECS: f32 = 0.25;
const HOVER_GLOW_MAX_RADIUS_PX: f32 = 15.0;
const HOVER_SCALE_BOOST: f32 = 0.07;

/// How long `theme_progress` takes to ramp fully from one theme to the
/// other — a touch slower than a group transition's own 0.4–0.6s; the
/// whole app re-themes at once, a bigger showcase moment than any one
/// shape morphing into another. Mirrors
/// `proteus-shell-native::THEME_MORPH_DURATION`.
const THEME_MORPH_DURATION_SECS: f32 = 0.6;

/// The one primary color — border, glow, and idle text/icon color all draw
/// from this single value everywhere in the demo (no separate per-component
/// colors). Mirrors `proteus-shell-native::violet()`.
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// The dark-theme counterpart to [`violet`] — every Border/Glow/Text
/// primary color continuously lerps between the two as `theme_progress`
/// ramps (`Demo::blend_primary_color`). Mirrors
/// `proteus-shell-native::violet_dark()`.
fn violet_dark() -> Vec4 {
    Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
}

/// Blends `handle`'s corner radius between `light`/`dark` by `p` (0=light,
/// 1=dark) — consolidates the original's ~7 hand-copied
/// `qs.corner_radius = light + (dark - light) * p` call sites (one per
/// themed widget family) into one shared call. A no-op if `handle` has no
/// live `QuadState` (e.g. hidden/destroyed).
fn blend_corner_radius(app: &mut Proteus, handle: Handle, light: f32, dark: f32, p: f32) {
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(handle.id()) {
        qs.corner_radius = light + (dark - light) * p;
    }
}

/// Blends `handle`'s Border/Glow/Text RGB — never alpha, which stays
/// independently owned by whatever hover/fade code the entity already has
/// — toward [`violet_dark`] by `p`. Writes whichever of Border/Glow/Text
/// `handle` actually carries; a no-op for whichever it lacks. Consolidates
/// the original's `advance_theme` step 5 (~10 call sites, one per themed
/// widget family) into one shared call.
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

/// Registers `handle` for the Design-System hover glow/scale treatment —
/// see [`HoverEntry`]'s doc for the mechanism and `Demo::advance_hovers`
/// for the per-tick ramp. Call once per interactive surface, during
/// `Demo::new()`, right alongside that surface's `on_click` wiring.
fn register_hover(app: &mut Proteus, hovers: &mut Vec<HoverEntry>, handle: Handle) {
    let is_hovering = Rc::new(Cell::new(false));
    {
        let flag = is_hovering.clone();
        handle.on_hover_enter(app, move |_| flag.set(true));
    }
    {
        let flag = is_hovering.clone();
        handle.on_hover_exit(app, move |_| flag.set(false));
    }
    hovers.push(HoverEntry {
        handle,
        is_hovering,
        progress: 0.0,
    });
}

/// Config shared by every group transition. Placeholder — not the original
/// demo's per-edge `BUTTON_TILES_MORPH_DURATION`/`GALLERY_GRID_MORPH_DURATION`
/// distinction yet (both happen to be 0.4s in the original anyway, for every
/// edge reachable so far).
fn group_transition_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.4,
        delay: 0.0,
        easing: ease_in_out_quad,
    }
}

/// A touch slower than `group_transition_config`'s 0.4s — a bigger, more
/// deliberate fan-out (1↔12 vs 1↔3) reads better slower. Used by every
/// transition touching `gallery`'s 12-tile grid; `Home↔Loading` (a 1-target
/// destination, the logo) stays on the standard duration. Mirrors
/// `proteus-shell-native::GALLERY_GRID_MORPH_DURATION`.
fn gallery_group_transition_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.6,
        delay: 0.0,
        easing: ease_in_out_quad,
    }
}

/// Overall fetch timeout, past which `Loading` gives up and shows
/// `loading::ERROR_TEXT` instead of proceeding to `Gallery`. Mirrors
/// `proteus-shell-native::GALLERY_FETCH_TIMEOUT`.
const GALLERY_FETCH_TIMEOUT_SECS: f32 = 10.0;

/// How long `gallery.hires_overlay` takes to fade from transparent to
/// fully opaque once it's ready to show — see
/// `Demo::advance_gallery_hires_overlay`'s doc. Mirrors
/// `proteus-shell-native::GALLERY_HIRES_CROSSFADE_DURATION`.
const GALLERY_HIRES_CROSSFADE_DURATION_SECS: f32 = 0.25;

/// Above this average FPS, a Stress Tests result is annotated as vsync-capped
/// rather than left looking like the demo tops out there on its own — both
/// shells run `PresentMode::AutoVsync`, a real, deliberate cap. Mirrors
/// `proteus-shell-native::VSYNC_FPS_CAP_THRESHOLD`.
const VSYNC_FPS_CAP_THRESHOLD: f32 = 55.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Splash,
    Home,
    ExamplesHome,
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
    /// The fetched grid is visible — click home to converge back to the
    /// nav buttons (three column-grouped merges), click "Fetch New Images"
    /// to refetch (back through `Loading`), or click a tile to enlarge it
    /// (`GalleryImage`).
    Gallery,
    /// `gallery.enlarged` showing photo `.0`'s full frame — click it (or
    /// `nav::Nav::back`) to converge back to the grid, click home to
    /// converge straight to the nav buttons. Entering this state also
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
    /// Home's "Examples" nav button.
    OpenExamplesHome,
    /// Home's "Videos" nav button.
    OpenVideoTiles,
    /// Home's "Gallery" nav button.
    OpenGallery,
    /// `gallery.fetch_button` — only meaningful from `Gallery`.
    RefetchGallery,
    /// One of `gallery`'s 12 tiles — only meaningful from `Gallery`.
    OpenGalleryImage(usize),
    /// `nav::Nav::home` — always goes to `Home`, regardless of which screen
    /// it was clicked from (`advance_nav_click` picks the right topology
    /// from `self.state` at the time it's processed).
    GoHome,
    /// One of `examples_home`'s category buttons (0–3 only — see
    /// `screens::examples_home`'s doc).
    OpenExampleDetail(usize),
    /// One of `video_tiles`' 3 tiles.
    OpenVideoScreen(usize),
    /// `nav::Nav::back` — meaningful from `ExampleDetail` (back to
    /// `ExamplesHome`), `VideoScreen` (back to `VideoTiles`), or
    /// `GalleryImage` (back to `Gallery`, also fired by clicking
    /// `gallery.enlarged` itself — see `Demo::advance_nav_click`'s match
    /// arm); `advance_nav_click` picks the right topology from
    /// `self.state`, same as `GoHome`.
    Back,
    /// `example_detail.stress.buttons[0]` — only meaningful from
    /// `ExampleDetail(3)`.
    RunBurstSpawn,
    /// `example_detail.stress.buttons[1]` — only meaningful from
    /// `ExampleDetail(3)`.
    RunTextureChurn,
    /// `theme.sun`/`theme.moon` — sets `dark_target` directly (`false`/
    /// `true`), unconditional on `self.state` (a theme switch is orthogonal
    /// to navigation, unlike every other `NavClick` variant).
    SetTheme(bool),
}

/// A transition target's *children* to reveal once the transition's own
/// duration has elapsed. `reveal_on_complete` (built into
/// `split_to`/`merge_from`) only knows about the literal target/source
/// entities it was given, not their descendants or associated standalone
/// content — first needed for `screens::home`'s nav labels, reused here for
/// `screens::examples_home`'s labels and each `ExampleDetail` category's
/// content.
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
/// from-and-to the same (already-reset) shape — no visible morph at all,
/// just an instant snap. `Demo::advance_pending_tile_reset` instead waits
/// until `tile` is actually observed hidden (proof the setup system has
/// already run and captured the real "from" state), only *then* resets
/// everything — safe whether or not `tile` is also one of the split's own
/// targets and gets revealed again later, since a hidden entity's geometry
/// can't be seen anyway.
struct PendingTileReset {
    tile: Handle,
}

/// One entity registered for the Design-System hover glow/scale treatment
/// — see `Demo::register_hover`'s doc for how it's wired up, and
/// `Demo::advance_hovers`' for the per-tick ramp/write. Mirrors the
/// original's ~9 hand-duplicated `advance_*_hover` functions
/// (`proteus-shell-native::advance_nav_hover` is the clearest single
/// example) consolidated into one generic mechanism — same runtime
/// behavior (0.25s linear ramp, 15px glow, 7% scale, suppressed during any
/// transition on this entity), not copy-pasted per screen.
struct HoverEntry {
    handle: Handle,
    /// Flipped by the `on_hover_enter`/`on_hover_exit` callbacks
    /// `register_hover` attaches — callbacks only get `&mut Proteus`, not
    /// `&mut Demo`, so this `Rc<Cell<_>>` is how the event reaches
    /// `advance_hovers` later, same pattern `nav_click` already uses for
    /// clicks (see that field's doc), just one flag per entity instead of
    /// one shared `Option<NavClick>`.
    is_hovering: Rc<Cell<bool>>,
    /// 0 (rest) → 1 (fully hovered), ramped at `dt / HOVER_GLOW_DURATION_SECS`.
    progress: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StressKind {
    BurstSpawn,
    TextureChurn,
}

/// An in-progress Stress Tests run — either kind despawns `entities` in
/// bulk once `elapsed` reaches `example_detail::STRESS_TEST_DURATION`
/// (`Demo::finalize_stress_test`) or immediately if the user navigates away
/// first (`Demo::cancel_stress_test`); neither ever despawns them one at a
/// time. `churn_iterations` only means anything for `TextureChurn`.
struct StressRun {
    kind: StressKind,
    elapsed: f32,
    /// Ticks elapsed during the run — `Demo::finalize_stress_test` divides
    /// this by `elapsed` for the result message's average-FPS figure.
    /// Mirrors `proteus-shell-native::StressTestRun::frame_count`.
    frame_count: u32,
    entities: Vec<Handle>,
    churn_iterations: u32,
}

/// One Texture Churn slot's fresh synthetic texture, computed by `Demo`
/// (pure data — a solid-color RGBA buffer, no GPU needed to generate it)
/// but not yet registered anywhere. `Demo::take_pending_texture_churn`
/// drains these each tick; the shell does the actual
/// `register_static`/`write_to_main_atlas`/`Handle::set_texture` sequence
/// (same "shell owns anything GPU-touching" convention as the crate-root
/// doc, and the same shape as `Demo::set_logo_frames`'s pre-baked-frames
/// injection, just generated on the fly instead of loaded from disk).
pub struct TextureChurnUpdate {
    pub handle: Handle,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A fresh gallery fetch to kick off — drained by the shell via
/// [`Demo::take_pending_gallery_fetch`], which owns starting
/// `gallery::TILE_COUNT` concurrent image downloads its own way (native:
/// blocking HTTP calls on background threads; web: `fetch()`) at roughly
/// `tile_side_px` each, then calling [`Demo::set_gallery_tile_image`] per
/// completed download. A fetch that never completes for some tile is fine —
/// `Demo` handles that itself (`GALLERY_FETCH_TIMEOUT_SECS`); the shell
/// doesn't need a cancellation call of its own for an abandoned fetch (e.g.
/// leaving `Loading` via the home icon before it finishes) — just stop
/// delivering results for it, same as `proteus-shell-native`'s own
/// drop-the-receiver approach.
///
/// `tile_side_px` is in the same logical-pixel units as every other size
/// `Demo` deals in — `Demo` has no notion of the display's pixel density
/// (see [`Demo::set_viewport_size`]'s doc), so it's the shell's job to
/// scale this by its own `scale_factor` (and apply whatever physical-pixel
/// cap it wants) before actually fetching, exactly the way
/// `proteus-shell-native::start_home_to_loading`/`start_gallery_to_loading`
/// do (`gallery_cell_size(..) * scale_factor`, capped at
/// `MAX_TILE_IMAGE_SIDE`) — `Demo` fetching a plain, un-scaled logical size
/// would under-fetch on any HiDPI display.
pub struct GalleryFetchRequest {
    pub tile_side_px: u32,
}

/// A hires upgrade to fetch for the currently-enlarged tile — drained by
/// the shell via [`Demo::take_pending_gallery_hires_fetch`], which owns
/// fetching (or re-fetching, at a bigger size — likely the *same* photo the
/// original low-res fetch already picked, if the shell tracked which one)
/// `idx`'s photo at exactly `width_px`×`height_px`, then calling
/// [`Demo::set_gallery_hires_image`] with the result. `width_px`/
/// `height_px` already preserve the box's exact aspect ratio (`Demo`
/// computes them from `gallery_tile_aspect[idx]`, the only side that knows
/// the photo's real aspect — see [`Demo::start_gallery_to_image`]'s doc) —
/// the shell doesn't need to know or preserve the aspect ratio itself.
///
/// Same logical-pixel/uncapped convention as [`GalleryFetchRequest::
/// tile_side_px`] — see that field's doc. The shell scaling and capping
/// this by its own `scale_factor` mirrors
/// `proteus-shell-native::start_gallery_to_image`'s own
/// `scale_factor`/`GALLERY_LARGE_IMAGE_MAX_SIDE`.
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
/// Does **not** own its [`Proteus`] — the [`crate::DemoApp`] `App` impl (and,
/// on web until M13.2, the shell) threads `&mut Proteus` into every method.
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
    /// until [`Demo::set_logo_frames`] is called. Baking the PNGs is shell
    /// I/O (see [`Demo::set_logo_frames`]'s doc); cycling which one is shown
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
    /// Tiny xorshift32 PRNG state, seeded nonzero (xorshift's one hard
    /// requirement) — Burst Spawn's random targets and Texture Churn's
    /// random size/hue don't need cryptographic-quality randomness, just
    /// enough jitter to look lively, so this crate has no `rand`/`fastrand`
    /// dependency. Matches `proteus-shell-native`'s own approach.
    stress_rng: u32,
    stress_run: Option<StressRun>,
    /// This tick's Texture Churn texture updates, drained by the shell via
    /// [`Demo::take_pending_texture_churn`] — see that type's doc.
    pending_texture_churn: Vec<TextureChurnUpdate>,
    /// Set to `Some(tile_idx)` when a tile should start playing video —
    /// drained by the shell via [`Demo::take_pending_video_start`], which
    /// owns actually decoding a file for that index (see the crate-root
    /// doc). The entity itself already shows the video texture by the time
    /// this is set (`Handle::start_video`, called synchronously); this flag
    /// only tells the shell *which* file to start decoding.
    pending_video_start: Option<usize>,
    /// Set when the currently-playing tile should stop — drained by the
    /// shell via [`Demo::take_pending_video_stop`], which owns actually
    /// killing the decode thread/releasing the GPU video texture.
    pending_video_stop: bool,
    /// Whether the shell has confirmed a real decoded frame has actually
    /// been uploaded for the currently-playing video — set via
    /// [`Demo::set_video_first_frame_shown`], reset to `false` each time
    /// [`Demo::start_tiles_to_screen`] starts a new video. Drives
    /// [`Demo::advance_video_loading`]'s loading-dots visibility, same
    /// role as `proteus-shell-native::PlayingVideo::first_frame_shown`.
    video_first_frame_shown: bool,
    /// Elapsed time (seconds) driving the loading dots' pulse phase — reset
    /// to 0 each time [`Demo::start_tiles_to_screen`] starts a new video, so
    /// the sequence always begins at dot 0 rather than an arbitrary phase.
    /// Also doubles as the "how long have we been waiting" clock
    /// [`Demo::advance_video_loading`] checks against
    /// `video_tiles::VIDEO_LOAD_TIMEOUT_SECS`. Mirrors
    /// `proteus-shell-native::video_dots_elapsed`.
    video_dots_elapsed: f32,
    /// Latches once `video_dots_elapsed` crosses `video_tiles::
    /// VIDEO_LOAD_TIMEOUT_SECS` with no frame shown yet — swaps the loading
    /// dots for `video_tiles::VideoTiles::error_text`. Reset to `false`
    /// each time [`Demo::start_tiles_to_screen`] starts a new video.
    /// Mirrors `proteus-shell-native::video_load_timed_out`.
    video_load_timed_out: bool,
    /// Set the same instant `video_load_timed_out` first latches, drained
    /// by the shell via [`Demo::take_pending_video_cancel`] — unlike
    /// `video_load_timed_out` itself (a level, read every tick to decide
    /// dots-vs-error visibility), this is an edge: exactly one `true` read
    /// per timeout, telling the shell "abort whatever fetch/decode is still
    /// in flight, `Demo` hasn't torn anything down (the screen stays on
    /// `VideoScreen`, now showing the error text), so don't stop playback,
    /// just stop wasting bandwidth." Native's own local `.mp4`/`ffmpeg`
    /// decode has no in-flight *fetch* to abort on a timeout, so it never
    /// needed this — the web shell's real HLS segment fetch does (see
    /// `proteus-shell-web`'s own former `take_video_cancel`, which this
    /// mirrors).
    pending_video_cancel: bool,
    /// How long we've been *continuously* settled-and-waiting (tile not
    /// mid-morph, resting on `VideoScreen`, no frame yet) — unlike
    /// `video_dots_elapsed` (which runs from click time and never resets
    /// early), this resets to 0 the instant that condition stops holding,
    /// so it always measures just the current wait. Gates the dots'
    /// `video_tiles::VIDEO_DOT_SHOW_DELAY_SECS` grace period. Mirrors
    /// `proteus-shell-native::video_settled_elapsed`.
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
    /// `GALLERY_FETCH_TIMEOUT_SECS`, the error path — one shared counter,
    /// mirroring `proteus-shell-native::gallery_fetch_elapsed`.
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
    /// over its spot). Mirrors
    /// `proteus-shell-native::gallery_logo_error_fade`.
    gallery_logo_error_fade: f32,
    /// Bumped by `Demo::begin_gallery_fetch` every time a fetch (first or
    /// re-) kicks off. Paired with `gallery_tile_fetch_generation` so the
    /// `Loading` → `Gallery` auto-advance can't be satisfied by a tile that
    /// still shows a *previous* fetch's image — its own current-round fetch
    /// might still be pending, or might have failed. Mirrors
    /// `proteus-shell-native::gallery_fetch_generation`.
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
    /// asynchronous (the shell's own per-frame job — see the crate-root
    /// doc), so this can't happen synchronously inside
    /// `set_gallery_tile_image` itself; there's nothing to stash/crop yet
    /// at that point.
    pending_gallery_tile_crop: [bool; gallery::TILE_COUNT],
    /// This tick's gallery fetch request, if any — drained by the shell via
    /// [`Demo::take_pending_gallery_fetch`]. See [`GalleryFetchRequest`]'s
    /// doc.
    pending_gallery_fetch: Option<GalleryFetchRequest>,
    /// This tick's hires fetch request, if any — drained by the shell via
    /// [`Demo::take_pending_gallery_hires_fetch`]. See
    /// [`GalleryHiresFetchRequest`]'s doc.
    pending_gallery_hires_fetch: Option<GalleryHiresFetchRequest>,
    /// Set whenever `GalleryImage` is left (either destination) — drained
    /// by the shell via [`Demo::take_pending_gallery_hires_cancel`], which
    /// should stop delivering results for whatever hires fetch it has in
    /// flight (same "just stop delivering results, nothing to actually
    /// interrupt" convention as [`Demo::take_pending_gallery_fetch`]'s own
    /// doc). Fires unconditionally on exit, whether or not a fetch was
    /// actually still pending — cheap and always correct, matching
    /// `proteus-shell-native::cancel_gallery_hires_fetch`'s own
    /// unconditional call sites.
    pending_gallery_hires_cancel: bool,
    /// `gallery.hires_overlay`'s crossfade-in progress (0 → 1, never
    /// reverses within one visit) — see
    /// `Demo::advance_gallery_hires_overlay`'s doc. Reset to `0.0` at the
    /// start of every fresh `GalleryImage` visit (`start_gallery_to_image`),
    /// so a previous visit's fully-faded-in overlay never flashes in
    /// instantly for a new tile before its own hires image is ready.
    gallery_hires_fade: f32,
    /// Fade-in (once fully settled in `Gallery`) / fade-out (the instant a
    /// `Gallery`→elsewhere morph starts) progress for `gallery.fetch_button`/
    /// `.fetch_button_label` — see `Demo::advance_gallery_button_fade`'s
    /// doc. Mirrors `proteus-shell-native::gallery_button_fade`.
    gallery_button_fade: f32,
    /// Every entity registered for the Design-System hover treatment — see
    /// [`HoverEntry`]'s doc. Populated by `Demo::register_hover` calls
    /// during `Demo::new()`, one per interactive surface; drained/ramped
    /// every tick by `Demo::advance_hovers`.
    hovers: Vec<HoverEntry>,
    /// `true` once the user has clicked toward dark — flips instantly on
    /// click; `theme_progress` is what actually ramps toward it.
    /// Mirrors `proteus-shell-native::dark_target`.
    dark_target: bool,
    /// 0.0 = fully light, 1.0 = fully dark. Ramped toward `dark_target` by
    /// `Demo::advance_theme`, which derives every themed entity's corner
    /// radius/color/image-crossfade alpha from this one scalar. Mirrors
    /// `proteus-shell-native::theme_progress`.
    theme_progress: f32,
    /// Fade-in progress (0..1) for `[theme.sun, theme.moon]` — shares
    /// `screens::nav`'s icon fade-in timing, tracked separately. Mirrors
    /// `proteus-shell-native::theme_icon_fade`.
    theme_icon_fade: [f32; 2],
    /// Fade-in progress (0..1) for `nav.lockup` — visible once past
    /// `Splash`, forever after (including on `Home` itself — this row is
    /// persistent brand chrome, not a Home-only convenience). Mirrors
    /// `proteus-shell-native::logo_fade`.
    nav_lockup_fade: f32,
    /// Fade-in/out progress (0..1) for `[nav.home, nav.back]` — `home`
    /// shares `nav_lockup_fade`'s target (same "visible forever past
    /// Splash" rule); `back` only targets 1 while resting on
    /// `ExampleDetail`/`VideoScreen`/`GalleryImage`. Mirrors
    /// `proteus-shell-native::nav_icon_fade`.
    nav_icon_fade: [f32; 2],
    /// Fade progress (0..1) for `nav.home_selected`/`.home_selected_dark`'s
    /// shared envelope — targets 1 only while resting on `Home`. The
    /// *final* alpha each of the two actually gets is this value
    /// hard-gated by `dark_target` (`Demo::advance_theme`), not a plain
    /// write of this field. Mirrors
    /// `proteus-shell-native::home_selected_fade`.
    nav_home_selected_fade: f32,
    splash: splash::Splash,
    home: home::Home,
    examples_home: examples_home::ExamplesHome,
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
        let examples_home = examples_home::spawn(proteus);
        let example_detail = example_detail::spawn(proteus);
        let nav = nav::spawn(proteus);
        let video_tiles = video_tiles::spawn(proteus);
        let loading = loading::spawn(proteus);
        let gallery = gallery::spawn(proteus, DEFAULT_VIEWPORT_SIZE);
        let theme = theme::spawn(proteus);

        // Populated below, per interactive surface, via `register_hover` —
        // see [`HoverEntry`]'s doc.
        let mut hovers: Vec<HoverEntry> = Vec::new();

        // Every transition *target*/standalone-content entity starts
        // hidden — `component()` always spawns visible by default (see
        // `proteus_sdk::Visibility`'s own doc), so initial visibility for
        // anything not shown from frame one has to be set explicitly here,
        // via the escape hatch. `ComponentSpec` has no `.hidden()` builder,
        // deliberately: this is the state machine's concern, not something
        // a screen's own spawn function should have to know about itself.
        let hide = |app: &mut Proteus, handles: &[Handle]| {
            for &h in handles {
                app.world_mut()
                    .entity_mut(h.id())
                    .insert(Visibility::HIDDEN);
            }
        };
        hide(proteus, &home.nav_buttons);
        hide(proteus, &home.nav_labels);
        hide(proteus, &examples_home.buttons);
        hide(proteus, &examples_home.labels);
        hide(
            proteus,
            &[example_detail.panel, nav.home, nav.back, nav.lockup],
        );
        for idx in 0..6 {
            let content = example_detail.content_handles(idx);
            hide(proteus, &content);
        }
        hide(proteus, &[example_detail.stress.warning_text]);
        hide(proteus, &video_tiles.tiles);
        hide(proteus, &[video_tiles.backdrop, video_tiles.error_text]);
        hide(proteus, &video_tiles.loading_dots);
        hide(proteus, &[loading.logo, loading.error_text]);
        hide(proteus, &gallery.tiles);
        // `fetch_button_label` deliberately stays out of this — it relies
        // entirely on cascading from `fetch_button`'s own `Visibility`
        // (toggled solely by `Demo::advance_gallery_button_fade`, which
        // never touches the label directly, matching source's own
        // identical setup): still effectively hidden right now since its
        // parent is, but its own raw flag needs to stay `VISIBLE` forever
        // so a *later* `fetch_button` reveal doesn't have to also
        // remember to un-hide the label separately.
        hide(proteus, &[gallery.fetch_button]);
        hide(proteus, &[gallery.enlarged, gallery.hires_overlay]);
        hide(proteus, &gallery.tile_full);
        hide(proteus, &[theme.sun, theme.moon]);

        let nav_click: Rc<Cell<Option<NavClick>>> = Rc::new(Cell::new(None));
        {
            let flag = nav_click.clone();
            home.nav_buttons[0]
                .on_click(proteus, move |_| flag.set(Some(NavClick::OpenVideoTiles)));
        }
        {
            let flag = nav_click.clone();
            home.nav_buttons[1].on_click(proteus, move |_| flag.set(Some(NavClick::OpenGallery)));
        }
        {
            let flag = nav_click.clone();
            home.nav_buttons[2]
                .on_click(proteus, move |_| flag.set(Some(NavClick::OpenExamplesHome)));
        }
        for &button in &home.nav_buttons {
            register_hover(proteus, &mut hovers, button);
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
        register_hover(proteus, &mut hovers, nav.home);
        register_hover(proteus, &mut hovers, nav.back);
        for idx in 0..6 {
            let flag = nav_click.clone();
            examples_home.buttons[idx].on_click(proteus, move |_| {
                flag.set(Some(NavClick::OpenExampleDetail(idx)))
            });
        }
        for &button in &examples_home.buttons {
            register_hover(proteus, &mut hovers, button);
        }
        {
            let flag = nav_click.clone();
            example_detail.stress.buttons[0]
                .on_click(proteus, move |_| flag.set(Some(NavClick::RunBurstSpawn)));
        }
        {
            let flag = nav_click.clone();
            example_detail.stress.buttons[1]
                .on_click(proteus, move |_| flag.set(Some(NavClick::RunTextureChurn)));
        }
        for &button in &example_detail.stress.buttons {
            register_hover(proteus, &mut hovers, button);
        }
        for idx in 0..3 {
            let flag = nav_click.clone();
            video_tiles.tiles[idx].on_click(proteus, move |_| {
                flag.set(Some(NavClick::OpenVideoScreen(idx)))
            });
        }
        for &tile in &video_tiles.tiles {
            register_hover(proteus, &mut hovers, tile);
        }
        {
            let flag = nav_click.clone();
            gallery
                .fetch_button
                .on_click(proteus, move |_| flag.set(Some(NavClick::RefetchGallery)));
        }
        register_hover(proteus, &mut hovers, gallery.fetch_button);
        for idx in 0..gallery::TILE_COUNT {
            let flag = nav_click.clone();
            gallery.tiles[idx].on_click(proteus, move |_| {
                flag.set(Some(NavClick::OpenGalleryImage(idx)))
            });
        }
        for &tile in &gallery.tiles {
            register_hover(proteus, &mut hovers, tile);
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
        // Glow only, no scale-boost — see `Demo::advance_gallery_enlarged_
        // hover_scale`'s doc for why this one entity's hover reaction is
        // deliberately incomplete relative to the shared engine.
        register_hover(proteus, &mut hovers, gallery.enlarged);
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
        register_hover(proteus, &mut hovers, theme.sun);
        register_hover(proteus, &mut hovers, theme.moon);

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
            stress_rng: 0x9E3779B9,
            stress_run: None,
            pending_texture_churn: Vec::new(),
            pending_video_start: None,
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
            hovers,
            dark_target: false,
            theme_progress: 0.0,
            theme_icon_fade: [0.0; 2],
            nav_lockup_fade: 0.0,
            nav_icon_fade: [0.0; 2],
            nav_home_selected_fade: 0.0,
            splash,
            home,
            examples_home,
            example_detail,
            nav,
            video_tiles,
            loading,
            gallery,
            theme,
        }
    }

    /// Injects the background's light-theme image bytes — the shell reads
    /// the file (or `fetch()`s it, on web) its own way and hands over the
    /// bytes; baking them into `main_atlas` is the shell's own per-frame job
    /// too (see the crate-root doc), same convention as `Text`/the logo
    /// frames. Call once, before the first `tick`.
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

    /// Injects `nav.home`'s idle-art bytes — same shell-does-the-I/O
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
    /// inverted-role convention this pair uses (mirrors `proteus-shell-
    /// native::SUN_ICON_PATH`'s own doc, `sun-idle-dark.png`).
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

    /// Injects one video tile's box-cover art — same shell-does-the-I/O
    /// convention as [`Demo::set_background_image`]. `idx` is 0/1/2
    /// (left/center/right); a tile whose image never arrives just keeps its
    /// solid placeholder `TILE_COLORS` fill (matches
    /// `proteus-shell-native`'s own per-tile graceful degradation — call
    /// this only for tiles whose bytes the shell actually managed to read).
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
    /// shell-does-the-I/O convention as [`Demo::set_background_image`]/
    /// [`Demo::set_tile_image`], but a tile can be re-fetched
    /// (`start_gallery_to_loading`'s refetch), unlike either of those, so
    /// this also frees the tile's prior baked image/texture first
    /// (`Handle::free_resources`) — without that, the shell's next bake
    /// pass would see the tile already has a baked image (from the
    /// previous fetch) and skip re-baking the fresh bytes entirely, per
    /// the generic "bake anything with an `Image` but no `BakedImage` yet"
    /// convention `examples/native_preview.rs` uses. Stamps this tile as
    /// belonging to the current fetch generation — see
    /// `gallery_tile_fetch_generation`'s doc. Call only for tiles whose
    /// bytes the shell actually managed to fetch; a tile that never gets a
    /// call here just keeps showing its plain white placeholder (or
    /// whichever photo it last had, on a refetch) and blocks the
    /// `Loading` → `Gallery` auto-advance until `GALLERY_FETCH_TIMEOUT_SECS`
    /// gives up on it.
    ///
    /// `aspect` is the photo's real (width, height) ratio, known to the
    /// shell before baking even happens (e.g. from whatever catalog/API
    /// response supplied `bytes`). Stashed in `gallery_tile_aspect` and
    /// used later by `Demo::start_gallery_to_image` to contain-fit the
    /// *enlarged* view to the photo's true shape (portrait/landscape/
    /// square) — get this wrong and every enlarged photo comes out looking
    /// square regardless of the source's real proportions. Also queues a
    /// crop for the tile's own square grid cell, applied once baking
    /// completes — see [`Demo::advance_gallery_tile_crop`]'s doc.
    pub fn set_gallery_tile_image(
        &mut self,
        proteus: &mut Proteus,
        idx: usize,
        bytes: Vec<u8>,
        aspect: Vec2,
    ) {
        let tile = self.gallery.tiles[idx];
        tile.free_resources(proteus);
        self.pending_gallery_tile_crop[idx] = true;
        proteus
            .world_mut()
            .entity_mut(tile.id())
            .insert(Image::new(bytes));
        self.gallery_tile_fetch_generation[idx] = self.gallery_fetch_generation;
        self.gallery_tile_aspect[idx] = aspect;
    }

    /// Injects a hires upgrade for the currently-enlarged tile — same
    /// shell-does-the-I/O convention as [`Demo::set_gallery_tile_image`].
    /// Targets `gallery.hires_overlay`, not `gallery.enlarged` itself —
    /// see that field's doc for why: `enlarged`'s own low-res image must
    /// stay in place the whole time so there's always *something* to show,
    /// while the overlay bakes in the background and
    /// `Demo::advance_gallery_hires_overlay` crossfades it in once ready.
    /// Guarded by `idx` still matching the *currently* enlarged tile:
    /// [`Demo::take_pending_gallery_hires_cancel`] tells the shell to stop
    /// delivering results the moment `GalleryImage` is left, but a result
    /// that was already in flight at that exact instant could still land
    /// right after — this guard is what makes that race harmless rather
    /// than needing the shell to get the timing exactly right.
    pub fn set_gallery_hires_image(&mut self, proteus: &mut Proteus, idx: usize, bytes: Vec<u8>) {
        if self.state != AppState::GalleryImage(idx) {
            return;
        }
        self.gallery.hires_overlay.free_resources(proteus);
        // The one entity that wants a bigger cap than the rest of the grid —
        // only one hires image is ever resident, so it can afford a larger
        // footprint than the 12 simultaneous thumbnails. Was a separate
        // bigger-cap bake pass in the M12 native shell; now just a per-entity
        // `Image::max_side` the generic renderer bake honours.
        proteus
            .world_mut()
            .entity_mut(self.gallery.hires_overlay.id())
            .insert(Image::new(bytes).with_max_side(900));
    }

    /// Resizes/repositions everything that depends on the viewport size but
    /// isn't already re-derived from it every tick — the background and
    /// `gallery`'s grid. (`nav`'s icons and `theme`'s sun/moon toggle read
    /// `self.viewport_size` fresh every tick, in `advance_nav_icons`/
    /// `advance_theme`, so they don't need a resize-time call here — see
    /// those methods' own doc.) Call on every resize (and once up front with
    /// the real initial size, since [`Demo::new`] only has a placeholder to
    /// spawn with). Logical pixels, same convention as
    /// [`Demo::pointer_moved`].
    pub fn set_viewport_size(&mut self, proteus: &mut Proteus, size: Vec2) {
        self.viewport_size = size;
        for handle in [self.background.light, self.background.dark] {
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(handle.id()) {
                qs.size = size;
            }
        }
        for (tile, state) in self.gallery.tiles.into_iter().zip(gallery::layout(size)) {
            tile.set_declared_geometry(proteus, state);
        }
    }

    /// Injects Splash's animated logo mark's pre-baked frames — the shell
    /// loads/decodes/registers the 19 `frame-NN.png` files into `main_atlas`
    /// its own way (`fs::read` natively, `fetch()` on web) and wraps each
    /// with `Proteus::texture`, same shell-does-the-I/O convention as
    /// `Text`/`Image` baking (see the crate-root doc). Call once, before the
    /// first `tick`; shows `frames[0]` immediately if non-empty, matching
    /// `proteus-shell-native`'s own "start on frame 1" behavior.
    pub fn set_logo_frames(&mut self, proteus: &mut Proteus, frames: Vec<TextureHandle>) {
        self.logo_frames = frames;
        self.logo_frame_index = 0;
        self.logo_frame_elapsed = 0.0;
        if let Some(&first) = self.logo_frames.first() {
            self.splash.button.set_texture(proteus, first);
        }
    }

    /// Injects `loading.logo_dark`'s pre-baked frames (`frame-NN-dark.png`)
    /// — same shell-does-the-I/O convention as [`Demo::set_logo_frames`],
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
        self.advance_intro(proteus, dt);
        self.advance_state(proteus, dt);
        self.advance_nav_click(proteus);
        self.advance_pending_reveals(proteus, dt);
        self.advance_pending_tile_reset(proteus);
        self.advance_logo_animation(proteus, dt);
        self.advance_loading_logo_animation(proteus, dt);
        self.advance_gallery_tile_crop(proteus);
        self.advance_gallery_fetch(proteus, dt);
        self.advance_gallery_button_fade(proteus, dt);
        self.advance_gallery_hires_overlay(proteus, dt);
        self.advance_example_animation(proteus, dt);
        self.advance_stress_test(proteus, dt);
        self.advance_stress_warning_visibility(proteus);
        self.advance_hovers(proteus, dt);
        self.advance_gallery_enlarged_hover_scale(proteus);
        self.advance_nav_icons(proteus, dt);
        self.advance_tile_hover(proteus);
        self.advance_video_crossfade(proteus);
        self.advance_video_loading(proteus, dt);
        self.advance_theme(proteus, dt);
        // Must run after advance_theme — see this fn's own doc for why.
        self.advance_gallery_error_fade(proteus, dt);
        splash::recenter(proteus, &self.splash, self.intro_slide_offset);
        if self.state == AppState::Home {
            self.apply_examples_home_layout(proteus);
        }
        self.apply_example_detail_layout(proteus);
        self.apply_gallery_fetch_button_layout(proteus);
    }

    /// Intro fade (waits `splash::INTRO_DELAY_SECS`, then plays once,
    /// 0 → 1, never reverses) + slide-in, in lockstep. Burns off the delay
    /// first; any leftover `dt` in the same tick carries into the fade
    /// itself rather than being dropped (same pattern as `ActiveTransition`'s
    /// delay handling). Mirrors
    /// `proteus-shell-native::advance_intro_and_hover`'s fade/slide portion —
    /// hover isn't part of this crate yet.
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
        let alpha = ease_out_quad(raw_t);
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

        // Compute the real target geometry immediately before the split
        // starts — same ordering `start_examples_to_detail` uses for its
        // own merge target, and the fix for a real bug an earlier version
        // of this function had: calling `home::layout` repeatedly on some
        // earlier recurring gate (rather than once, right here) left the
        // buttons stuck at their identical spawn-time placeholder geometry
        // whenever that gate never actually fired, which is a materially
        // different (and much easier to hit) risk than the original's own
        // "one call, right before the split" — see `home::layout`'s doc.
        let states = home::layout(proteus, &self.home);
        for (button, state) in self.home.nav_buttons.into_iter().zip(states) {
            button.set_declared_geometry(proteus, state);
        }

        let button = self.splash.button;
        let targets = self.home.nav_buttons;
        button.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        self.queue_reveal(
            self.home.nav_labels.to_vec(),
            group_transition_config().duration,
        );
        self.state = AppState::Home;
    }

    /// Processes whichever `NavClick` fired this frame (see `nav_click`'s
    /// doc) and drives the corresponding transition. The guard on each arm
    /// re-checks `self.state` at processing time (not just at the moment
    /// the click happened) — since `self.state` flips the instant a
    /// transition starts, this also doubles as "ignore a click that arrived
    /// while a transition triggered by an earlier click this same frame is
    /// already underway."
    fn advance_nav_click(&mut self, proteus: &mut Proteus) {
        let Some(click) = self.nav_click.take() else {
            return;
        };
        match click {
            NavClick::OpenExamplesHome if self.state == AppState::Home => {
                self.start_home_to_examples(proteus);
            }
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
                AppState::ExamplesHome => self.start_examples_to_home(proteus),
                AppState::ExampleDetail(_) => self.start_detail_to_home(proteus),
                AppState::VideoTiles => self.start_tiles_to_home(proteus),
                AppState::VideoScreen(idx) => self.start_screen_to_home(proteus, idx),
                AppState::Loading => self.start_loading_to_home(proteus),
                AppState::Gallery => self.start_gallery_to_home(proteus),
                AppState::GalleryImage(_) => self.start_image_to_home(proteus),
                _ => {}
            },
            NavClick::OpenExampleDetail(idx) if self.state == AppState::ExamplesHome => {
                self.start_examples_to_detail(proteus, idx);
            }
            NavClick::OpenVideoScreen(idx) if self.state == AppState::VideoTiles => {
                self.start_tiles_to_screen(proteus, idx);
            }
            NavClick::Back => match self.state {
                AppState::ExampleDetail(_) => self.start_detail_to_examples(proteus),
                AppState::VideoScreen(idx) => self.start_screen_to_tiles(proteus, idx),
                AppState::GalleryImage(_) => self.start_image_to_gallery(proteus),
                _ => {}
            },
            NavClick::RunBurstSpawn if self.state == AppState::ExampleDetail(3) => {
                self.run_burst_spawn(proteus);
            }
            NavClick::RunTextureChurn if self.state == AppState::ExampleDetail(3) => {
                self.run_texture_churn(proteus);
            }
            NavClick::SetTheme(dark) => {
                self.dark_target = dark;
            }
            _ => {}
        }
    }

    /// 3 simultaneous 1→2 `GridSlice` splits, one per nav button — the
    /// reverse of `start_examples_to_home`. Mirrors
    /// `proteus-shell-native::start_home_to_examples` exactly.
    fn start_home_to_examples(&mut self, proteus: &mut Proteus) {
        for col in 0..3 {
            let source = self.home.nav_buttons[col];
            let targets = [
                self.examples_home.buttons[col * 2],
                self.examples_home.buttons[col * 2 + 1],
            ];
            source.split_to(
                proteus,
                &targets,
                group_transition_config(),
                SplitStrategy::GridSlice { cols: 1, rows: 2 },
            );
        }
        self.queue_reveal(
            self.examples_home.labels.to_vec(),
            group_transition_config().duration,
        );
        self.state = AppState::ExamplesHome;
    }

    /// 3 simultaneous 2→1 `Grid` merges, one per nav button — the reverse
    /// of `start_home_to_examples`.
    fn start_examples_to_home(&mut self, proteus: &mut Proteus) {
        for col in 0..3 {
            let dest = self.home.nav_buttons[col];
            let sources = [
                self.examples_home.buttons[col * 2],
                self.examples_home.buttons[col * 2 + 1],
            ];
            dest.merge_from(
                proteus,
                &sources,
                group_transition_config(),
                MergeLayout::Grid { cols: 1, rows: 2 },
            );
        }
        self.state = AppState::Home;
    }

    /// 3 independent single-target `Slice` splits, one per nav button — a
    /// degenerate 1→1 crossfade dressed up as a trivial split (button `i`
    /// goes straight to tile `i`, not a fan-out), same shape as
    /// `proteus-shell-native::start_nav_to_tiles`. `VideoScreen` (video
    /// playback) isn't migrated yet, so unlike the original this is the
    /// only edge out of `Home`'s "Videos" button for now.
    ///
    /// Uses `split_to_with_states` (each target's state built explicitly via
    /// `video_tiles::tile_target_state`), not plain `split_to` — a tile's
    /// *own* `declared_geometry` isn't guaranteed to already carry the
    /// "white if real box art is baked" override `tile_target_state`
    /// applies, so relying on it silently drops that override on whatever
    /// bake this crossfade produces. Mirrors
    /// `proteus-shell-native::start_nav_to_tiles`'s own explicit per-target
    /// `state` construction exactly.
    fn start_home_to_tiles(&mut self, proteus: &mut Proteus) {
        for i in 0..3 {
            let source = self.home.nav_buttons[i];
            let target = self.video_tiles.tiles[i];
            let state = video_tiles::tile_target_state(proteus, target, i);
            source.split_to_with_states(
                proteus,
                &[(target, state)],
                group_transition_config(),
                SplitStrategy::Slice,
            );
        }
        self.state = AppState::VideoTiles;
    }

    /// The exact mirror of `start_home_to_tiles` — since each morph is
    /// already a degenerate 1→1 crossfade in *either* direction, going back
    /// is just `split_to` again with source/target swapped, not a merge.
    /// Mirrors `proteus-shell-native::start_tiles_to_nav`.
    fn start_tiles_to_home(&mut self, proteus: &mut Proteus) {
        for i in 0..3 {
            let source = self.video_tiles.tiles[i];
            let target = self.home.nav_buttons[i];
            source.split_to(
                proteus,
                &[target],
                group_transition_config(),
                SplitStrategy::Slice,
            );
        }
        self.state = AppState::Home;
    }

    /// Grows the clicked tile into the video screen (a 1→1 morph, not a
    /// group transition — same shape as `Handle::animate_to`'s doc
    /// describes) and marks it to start showing video, crossfading in from
    /// the box-cover art in lockstep with the geometry morph
    /// (`Demo::advance_video_crossfade` owns the ramp itself — this just
    /// starts it at `0.0` instead of `start_video`'s own instant-cut
    /// default). Queues `idx` for the shell to actually start decoding
    /// (`take_pending_video_start`) — `video_t` starts at `0.0` (fully box
    /// art) regardless of how quickly the shell manages to actually get a
    /// real frame decoded, same "brief black gap behind the fading-in art"
    /// the original's own loading path has before its first frame too.
    /// Mirrors `proteus-shell-native::start_tiles_to_screen`/
    /// `start_video_playback`.
    fn start_tiles_to_screen(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        let target = video_tiles::video_screen_quad(self.viewport_size);
        tile.animate_to(proteus, target, group_transition_config());
        tile.start_video(proteus);
        tile.set_video_crossfade(proteus, 0.0);
        self.pending_video_start = Some(idx);
        // Fresh loading-UI state for this visit — see each field's own doc.
        // Mirrors `proteus-shell-native::start_video_playback`'s identical
        // resets (its own `first_frame_shown` lives on a freshly-constructed
        // `PlayingVideo` instead, same effect).
        self.video_first_frame_shown = false;
        self.video_dots_elapsed = 0.0;
        self.video_load_timed_out = false;
        self.pending_video_cancel = false;
        self.state = AppState::VideoScreen(idx);
    }

    /// One 1→3 `Slice` split — the clicked (screen-sized) tile fans back
    /// out to all 3 grid slots, including its own. Mirrors
    /// `proteus-shell-native::start_screen_to_tiles`/`stop_video_playback`.
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
    /// is baked" override, matching
    /// `proteus-shell-native::start_screen_to_tiles`'s own explicit
    /// per-target `state` construction exactly.
    fn start_screen_to_tiles(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        // Undo `advance_video_loading`'s "hide the tile's own art while
        // waiting" override *before* `split_to_with_states` bakes its own
        // "from" snapshot below — otherwise backing out of a still-loading
        // video would bake the tile's momentarily-invisible alpha into that
        // snapshot, and the whole outgoing morph back to the grid would show
        // nothing instead of fading back in. Unconditional (not gated on
        // `ready`): harmless if the tile was already fully visible. Mirrors
        // `proteus-shell-native::stop_video_playback`'s identical ordering.
        if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color.w = 1.0;
        }
        tile.stop_video(proteus);
        self.pending_video_stop = true;
        let targets: Vec<(Handle, QuadState)> = (0..3)
            .map(|i| {
                let t = self.video_tiles.tiles[i];
                let state = video_tiles::tile_target_state(proteus, t, i);
                (t, state)
            })
            .collect();
        tile.split_to_with_states(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        // See `PendingTileReset`'s doc for why this can't happen
        // synchronously here.
        self.pending_tile_reset = Some(PendingTileReset { tile });
        self.state = AppState::VideoTiles;
    }

    /// One 1→3 `Slice` split straight to the nav buttons, skipping
    /// `VideoTiles`' grid entirely — the `VideoScreen`-to-`Home` escape
    /// hatch, same shape as `start_detail_to_home`. The other two tiles
    /// never participate in this split (only the playing one does), so —
    /// unlike `start_screen_to_tiles`, where all 3 tiles are targets and
    /// get revealed by the split itself — they're hidden explicitly here;
    /// otherwise they'd be left visible at their old grid position, which
    /// would be wrong given nothing on `Home` should show any tile at all.
    /// Mirrors `proteus-shell-native::start_screen_to_nav`.
    fn start_screen_to_home(&mut self, proteus: &mut Proteus, idx: usize) {
        let tile = self.video_tiles.tiles[idx];
        // See `start_screen_to_tiles`'s identical restore for why this must
        // happen before `split_to` bakes its own "from" snapshot below.
        if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color.w = 1.0;
        }
        tile.stop_video(proteus);
        self.pending_video_stop = true;
        let targets = self.home.nav_buttons;
        tile.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        // `tile` isn't a target of *this* split (only `home.nav_buttons`
        // are), so nothing reveals it here to expose its still-screen-sized
        // live geometry right now — but leaving it un-reset would resurface
        // the exact same staleness the next time `start_home_to_tiles` uses
        // this same tile as a target (whose reveal, again, only flips
        // `Visibility`, never resyncs geometry). See `PendingTileReset`'s
        // doc for why this can't happen synchronously here.
        self.pending_tile_reset = Some(PendingTileReset { tile });
        for (i, &other) in self.video_tiles.tiles.iter().enumerate() {
            if i != idx {
                proteus
                    .world_mut()
                    .entity_mut(other.id())
                    .insert(Visibility::HIDDEN);
            }
        }
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
    /// two edges that begin a fetch. Mirrors
    /// `proteus-shell-native::start_home_to_loading`/
    /// `start_gallery_to_loading`'s shared setup.
    fn begin_gallery_fetch(&mut self, proteus: &mut Proteus) {
        self.gallery_fetch_generation = self.gallery_fetch_generation.wrapping_add(1);
        self.gallery_fetch_elapsed = 0.0;
        self.gallery_error_shown = false;
        self.gallery_logo_error_fade = 1.0;
        proteus
            .world_mut()
            .entity_mut(self.loading.error_text.id())
            .insert(Visibility::HIDDEN);
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
    /// Mirrors `proteus-shell-native::apply_loading_logo_frame` exactly.
    fn apply_loading_logo_frame(&mut self, proteus: &mut Proteus) {
        if let Some(&frame) = self.logo_frames.get(self.loading_logo_frame_index) {
            self.loading.logo.set_texture(proteus, frame);
        }
        if let Some(&frame) = self
            .loading_logo_frames_dark
            .get(self.loading_logo_frame_index)
        {
            self.loading.logo_dark.set_texture(proteus, frame);
        }
    }

    /// One 3→1 `Horizontal` merge — all 3 nav buttons converge onto
    /// `loading.logo`, then a fresh fetch begins. Mirrors
    /// `proteus-shell-native::start_home_to_loading`.
    fn start_home_to_loading(&mut self, proteus: &mut Proteus) {
        self.begin_gallery_fetch(proteus);
        let sources = self.home.nav_buttons;
        self.loading.logo.merge_from(
            proteus,
            &sources,
            group_transition_config(),
            MergeLayout::Horizontal,
        );
        self.state = AppState::Loading;
    }

    /// One 1→3 `Slice` split back to the nav buttons — the error escape
    /// hatch (clicking home while `Loading`, fetching or erroring) as well
    /// as the ordinary "Loading" → "Home" back-navigation. Mirrors
    /// `proteus-shell-native::start_loading_to_home`.
    fn start_loading_to_home(&mut self, proteus: &mut Proteus) {
        let targets = self.home.nav_buttons;
        self.loading.logo.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        proteus
            .world_mut()
            .entity_mut(self.loading.error_text.id())
            .insert(Visibility::HIDDEN);
        self.state = AppState::Home;
    }

    /// One 1→`gallery::TILE_COUNT` `GridSlice` split — `loading.logo` fans
    /// out into the grid once every tile's current-generation image has
    /// arrived (`advance_gallery_fetch`). `GridSlice` rather than flat
    /// `Slice` (unlike `Home`↔`Loading`'s single-target merge/split) so each
    /// tile radiates from its own quadrant instead of zigzagging across one
    /// shared axis — see `proteus_ui::SplitStrategy::GridSlice`'s doc.
    /// `gallery.fetch_button`/`.fetch_button_label` aren't split targets
    /// (only the 12 tiles are) — their own fade-in is `Demo::advance_
    /// gallery_button_fade`'s job, not queued here at all: it derives
    /// "settled" from the tiles' own real visibility, which already tracks
    /// this split's actual completion more precisely than a fixed-duration
    /// guess would. Mirrors `proteus-shell-native::start_loading_to_gallery`.
    fn start_loading_to_gallery(&mut self, proteus: &mut Proteus) {
        let targets = self.gallery.tiles;
        self.loading.logo.split_to(
            proteus,
            &targets,
            gallery_group_transition_config(),
            SplitStrategy::GridSlice {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        self.state = AppState::Gallery;
    }

    /// One `gallery::TILE_COUNT`→1 `Grid` merge — the reverse of
    /// `start_loading_to_gallery`, triggered by clicking "Fetch New
    /// Images". `fetch_button`/`.fetch_button_label` fade out on their own
    /// (`Demo::advance_gallery_button_fade`, the instant `self.state` stops
    /// being `Gallery` — which happens synchronously below) rather than
    /// hiding immediately here. Mirrors
    /// `proteus-shell-native::start_gallery_to_loading`.
    fn start_gallery_to_loading(&mut self, proteus: &mut Proteus) {
        self.begin_gallery_fetch(proteus);
        let sources = self.gallery.tiles;
        self.loading.logo.merge_from(
            proteus,
            &sources,
            gallery_group_transition_config(),
            MergeLayout::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        self.state = AppState::Loading;
    }

    /// Three simultaneous `N`→1 `Grid` merges, one per nav button — the
    /// grid's 4 columns split 1+2+1 across the 3 buttons (column 0 alone →
    /// button 0, columns 1–2 → button 1, column 3 alone → button 2, each
    /// group's own tiles arranged as their own sub-grid) rather than one
    /// 12→1 merge, so each tile converges toward whichever button sits
    /// closest to its own column instead of every tile converging on one
    /// shared target. See `gallery::column_group_tiles`'s doc. Mirrors
    /// `proteus-shell-native::start_gallery_to_home`(also named
    /// `start_gallery_to_nav` there).
    fn start_gallery_to_home(&mut self, proteus: &mut Proteus) {
        const GROUPS: [(usize, usize); 3] = [(0, 1), (1, 2), (3, 1)];
        for (dest, &(start_col, width)) in self.home.nav_buttons.into_iter().zip(GROUPS.iter()) {
            let sources: Vec<Handle> = gallery::column_group_tiles(start_col, width)
                .into_iter()
                .map(|idx| self.gallery.tiles[idx])
                .collect();
            dest.merge_from(
                proteus,
                &sources,
                gallery_group_transition_config(),
                MergeLayout::Grid {
                    cols: width,
                    rows: gallery::ROWS,
                },
            );
        }
        // `fetch_button`/`.fetch_button_label` fade out on their own
        // (`Demo::advance_gallery_button_fade`) rather than hiding
        // immediately here — see `start_gallery_to_loading`'s doc for the
        // same call.
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
    /// Also queues a hires fetch for the shell, sized from `enlarged`'s own
    /// target geometry — the *larger* axis rounded to an integer once, the
    /// other axis then derived from *that* rounded integer via the exact
    /// aspect ratio (never rounded independently — see the inline comment
    /// below for why that one extra degree of freedom is enough to visibly
    /// shift the image the instant the hires crossfade completes). Both
    /// values are in the same logical-pixel units as `self.viewport_size`
    /// and *uncapped* — `Demo` has no notion of the display's actual pixel
    /// density (see [`Demo::set_viewport_size`]'s doc), so it can't decide
    /// how many *physical* pixels "sharp enough" means; scaling by the
    /// shell's own `scale_factor` and applying whatever physical-pixel cap
    /// bounds the network fetch is entirely the shell's job (mirrors
    /// `proteus-shell-native::start_gallery_to_image`'s own
    /// `scale_factor`/`GALLERY_LARGE_IMAGE_MAX_SIDE` — both applied there,
    /// after this same rounding, to `physical_w`/`physical_h`, never to
    /// this method's logical `target_size`). `Demo` computes both axes
    /// itself, since it's the only side that actually knows the photo's
    /// real aspect ratio (`aspect`, above) — the shell just scales/caps
    /// and fetches, no aspect-ratio bookkeeping of its own needed. Mirrors
    /// `proteus-shell-native::start_gallery_to_image`, minus its
    /// crossfade-overlay bookkeeping — see `screens::gallery`'s fidelity
    /// note.
    fn start_gallery_to_image(&mut self, proteus: &mut Proteus, idx: usize) {
        let aspect = self.gallery_tile_aspect[idx];
        let target = gallery::large_image_quad(aspect, self.viewport_size);
        let target_size = target.size;
        self.gallery.enlarged.set_declared_geometry(proteus, target);
        self.gallery
            .enlarged
            .copy_baked_image_from(proteus, self.gallery.tile_full[idx]);

        // Clear any bake/bytes left over from a *previous* visit's hires
        // fetch — without this, `advance_gallery_hires_overlay`'s
        // `has_bake` check would see the stale `BakedImage` and start
        // crossfading the wrong photo in immediately, before this visit's
        // own fetch has even started.
        self.gallery.hires_overlay.free_resources(proteus);
        proteus
            .world_mut()
            .entity_mut(self.gallery.hires_overlay.id())
            .remove::<Image>()
            .insert(Visibility::HIDDEN);
        self.gallery_hires_fade = 0.0;

        // `fetch_button`/`.fetch_button_label` fade out on their own
        // (`Demo::advance_gallery_button_fade`) rather than hiding
        // immediately here — see `start_gallery_to_loading`'s doc for the
        // same call.
        let sources = self.gallery.tiles;
        self.gallery.enlarged.merge_from(
            proteus,
            &sources,
            gallery_group_transition_config(),
            MergeLayout::Grid {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );

        // Round only the larger axis, then derive the other from *that*
        // already-rounded integer via the exact aspect ratio — rounding
        // both axes independently (as an earlier version of this code
        // did) lets the fetched image's actual aspect ratio drift slightly
        // from the box's exact one, which shows up as a small content
        // shift the instant the crossfade swaps the (stretched) low-res
        // stand-in for the (correctly-shaped) hires image. Same technique
        // `examples/native_preview/gallery_fetch.rs::fetch_dimensions`
        // already uses for the low-res fetch. Deliberately uncapped here —
        // see this method's own doc for why capping belongs to the shell.
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
        self.state = AppState::GalleryImage(idx);
    }

    /// One 1→`gallery::TILE_COUNT` `GridSlice` split — the reverse of
    /// `start_gallery_to_image`, triggered by clicking `gallery.enlarged`
    /// itself or `nav::Nav::back`. Cancels the hires fetch unconditionally
    /// (matches `proteus-shell-native::cancel_gallery_hires_fetch`'s own
    /// unconditional call sites — see `pending_gallery_hires_cancel`'s
    /// doc), whether or not one had actually landed yet.
    fn start_image_to_gallery(&mut self, proteus: &mut Proteus) {
        self.pending_gallery_hires_cancel = true;
        self.pending_gallery_hires_fetch = None;
        proteus
            .world_mut()
            .entity_mut(self.gallery.hires_overlay.id())
            .insert(Visibility::HIDDEN);
        let targets = self.gallery.tiles;
        self.gallery.enlarged.split_to(
            proteus,
            &targets,
            gallery_group_transition_config(),
            SplitStrategy::GridSlice {
                cols: gallery::COLS,
                rows: gallery::ROWS,
            },
        );
        // `fetch_button`/`.fetch_button_label` fade in on their own
        // (`Demo::advance_gallery_button_fade`) — see
        // `start_loading_to_gallery`'s doc for why this isn't queued here.
        self.state = AppState::Gallery;
    }

    /// One 1→3 `Slice` split straight to the nav buttons — the
    /// `GalleryImage`-to-`Home` escape hatch, skipping `Gallery`'s grid
    /// entirely, same shape as `start_detail_to_home`/`start_screen_to_home`.
    /// The 12 real tiles were never revealed in the first place (they're
    /// still hidden from `start_gallery_to_image`'s own merge — see
    /// `gallery::Gallery::enlarged`'s doc), so unlike those two there's
    /// nothing extra to hide here.
    fn start_image_to_home(&mut self, proteus: &mut Proteus) {
        self.pending_gallery_hires_cancel = true;
        self.pending_gallery_hires_fetch = None;
        proteus
            .world_mut()
            .entity_mut(self.gallery.hires_overlay.id())
            .insert(Visibility::HIDDEN);
        let targets = self.home.nav_buttons;
        self.gallery.enlarged.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        self.state = AppState::Home;
    }

    /// One 6→1 `Grid` merge — all 6 category buttons converge onto the
    /// shared `example_detail.panel`. The panel's target geometry is fixed
    /// *before* the merge starts (mirrors
    /// `proteus-shell-native::start_examples_to_detail`'s own "set the real
    /// final resting state up front" ordering), so the morph animates
    /// straight to the correct spot instead of snapping after.
    fn start_examples_to_detail(&mut self, proteus: &mut Proteus, idx: usize) {
        let target = example_detail::panel_target(idx, self.viewport_size);
        self.example_detail
            .panel
            .set_declared_geometry(proteus, target);

        // Row-major order, to match `MergeLayout::Grid`'s expectation —
        // `examples_home.buttons` is itself column-major (`buttons[col*2]`
        // = top, `[col*2+1]` = bottom).
        let buttons = self.examples_home.buttons;
        let sources: Vec<Handle> = (0..2)
            .flat_map(|row| (0..3).map(move |col| buttons[col * 2 + row]))
            .collect();
        self.example_detail.panel.merge_from(
            proteus,
            &sources,
            group_transition_config(),
            MergeLayout::Grid { cols: 3, rows: 2 },
        );

        self.queue_reveal(
            self.example_detail.content_handles(idx),
            group_transition_config().duration,
        );
        self.active_example_category = Some(idx);
        self.state = AppState::ExampleDetail(idx);
    }

    /// One 1→6 `GridSlice` split — the reverse of `start_examples_to_detail`.
    fn start_detail_to_examples(&mut self, proteus: &mut Proteus) {
        let buttons = self.examples_home.buttons;
        let targets: Vec<Handle> = (0..2)
            .flat_map(|row| (0..3).map(move |col| buttons[col * 2 + row]))
            .collect();
        self.example_detail.panel.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::GridSlice { cols: 3, rows: 2 },
        );
        self.cancel_stress_test(proteus);
        self.hide_active_example_content(proteus);
        self.state = AppState::ExamplesHome;
    }

    /// One 1→3 `Slice` split straight to the nav buttons — the
    /// `ExampleDetail`-to-`Home` escape hatch, skipping `ExamplesHome`'s
    /// grid entirely. Mirrors
    /// `proteus-shell-native::start_detail_to_home`.
    fn start_detail_to_home(&mut self, proteus: &mut Proteus) {
        let targets = self.home.nav_buttons;
        self.example_detail.panel.split_to(
            proteus,
            &targets,
            group_transition_config(),
            SplitStrategy::Slice,
        );
        self.cancel_stress_test(proteus);
        self.hide_active_example_content(proteus);
        self.state = AppState::Home;
    }

    /// Hides whichever category is currently showing and clears
    /// `active_example_category` — called on the way out of
    /// `ExampleDetail`, since (unlike `screens::home`'s labels or
    /// `screens::examples_home`'s labels) this content is standalone, not a
    /// child of anything the transition system itself hides/reveals — see
    /// `screens::example_detail`'s module doc.
    fn hide_active_example_content(&mut self, proteus: &mut Proteus) {
        if let Some(idx) = self.active_example_category.take() {
            for handle in self.example_detail.content_handles(idx) {
                proteus
                    .world_mut()
                    .entity_mut(handle.id())
                    .insert(Visibility::HIDDEN);
            }
        }
    }

    /// Queues `entities` to become `Visibility::VISIBLE` once `duration`
    /// seconds have elapsed — see `PendingReveal`'s doc.
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
                    proteus
                        .world_mut()
                        .entity_mut(e.id())
                        .insert(Visibility::VISIBLE);
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
    /// live `QuadState`/`Border`/`Glow` back to anything (same "reveal
    /// doesn't touch content" rule [`Handle::copy_baked_image_from`]'s doc
    /// already covers for `BakedImage`) — the *other two* tiles, faded out
    /// by `Demo::advance_video_crossfade` while this one was playing (see
    /// that function's own doc), would otherwise stay stuck at that faded
    /// alpha forever: reported directly as "the tile backgrounds on the
    /// non-transitioning tiles are missing" the very first time this fade
    /// was ported. Mirrors `proteus-shell-native::settle_tile_idle`, called
    /// for all 3 tiles from `settle(AppState::VideoTiles)`.
    ///
    /// `video_tiles::tile_target_state`'s own `color` is `tile_quad`'s
    /// placeholder tint unless real box-cover art is baked, in which case
    /// it's untinted opaque white — a bare `tile_quad(idx)` would discard
    /// that and revert to the tint even with real art already loaded (a
    /// different real bug, reported directly as "tiles keep color tint
    /// from the original bg colors"). Mirrors `proteus-shell-
    /// native::settle_tile_geometry`'s own `BakedImage`-gated white
    /// override exactly.
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
            let state = video_tiles::tile_target_state(proteus, tile, i);
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
                *qs = state;
            }
            if let Some(mut border) = proteus.world_mut().get_mut::<Border>(tile.id()) {
                border.color.w = 1.0;
            }
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(tile.id()) {
                glow.radius = 0.0;
                glow.color.w = 1.0;
            }
        }
    }

    /// Finalizes `examples_home`'s 6 category buttons' grid layout once
    /// their labels have baked — see `screens::examples_home::layout`'s
    /// doc. Only safe to call while `state == Home`: once past that, the
    /// buttons can be mid-merge (their live `QuadState` driven by the
    /// transition system itself), and overwriting it here would fight that
    /// animation — gated by the one call site in `tick`, not here, so the
    /// safety condition stays visible at the call site.
    fn apply_examples_home_layout(&mut self, proteus: &mut Proteus) {
        if let Some(states) = examples_home::layout(proteus, &self.examples_home) {
            let buttons = self.examples_home.buttons;
            for (button, state) in buttons.into_iter().zip(states) {
                button.set_declared_geometry(proteus, state);
            }
        }
    }

    /// Positions the active category's heading/rows every tick — cheap and
    /// idempotent (see `example_detail::layout_content`'s doc), safe
    /// unconditionally: the panel's own `QuadState` never changes once
    /// `start_examples_to_detail` sets it, so there's no live transition
    /// here to fight, unlike `apply_examples_home_layout`.
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
    /// off to Home: the button is either mid-morph (its current frame gets
    /// baked into the Slice transition's snapshot, same as any other texture
    /// content) or already hidden, so there's nothing left to animate.
    /// Mirrors `proteus-shell-native::advance_logo_animation` exactly.
    fn advance_logo_animation(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.logo_frames.is_empty() || self.state != AppState::Splash {
            return;
        }
        self.logo_frame_elapsed += dt;
        while self.logo_frame_elapsed >= splash::LOGO_FRAME_DURATION {
            self.logo_frame_elapsed -= splash::LOGO_FRAME_DURATION;
            self.logo_frame_index = (self.logo_frame_index + 1) % self.logo_frames.len();
            let frame = self.logo_frames[self.logo_frame_index];
            self.splash.button.set_texture(proteus, frame);
        }
    }

    /// Same frame-sweep mechanics as `advance_logo_animation`, but for
    /// `loading.logo` — a separate index/elapsed pair (see that field's
    /// doc) since this one loops forever while `Loading` is active, rather
    /// than playing once. Mirrors
    /// `proteus-shell-native::advance_loading_logo_animation`.
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
    /// yet (`Handle::baked_image_size`) — baking is the shell's own
    /// per-frame job, so this can lag an arbitrary number of ticks behind
    /// the `set_gallery_tile_image` call. Once it has: stashes the
    /// still-uncropped frame onto `gallery.tile_full[idx]`
    /// (`Handle::copy_baked_image_from`), *then* center-crops the tile's
    /// own copy to a square in place (`Handle::center_crop_to_square`) —
    /// in that order, since the crop mutates the tile's `BakedImage`
    /// in place and would otherwise poison what gets stashed. Called every
    /// tick, before `advance_gallery_fetch` — so by the moment that
    /// function's "every tile baked" check can ever pass and advance to
    /// `Gallery`, every tile that just finished baking this same tick has
    /// already been stashed/cropped too, not left to catch up next tick.
    /// Mirrors `proteus-shell-native::bake_pending_images`'s gallery-tile
    /// branch (`center_crop_to_square` cloned + `gallery_tile_full_baked`
    /// stash), split out since baking itself stays a shell concern here.
    fn advance_gallery_tile_crop(&mut self, proteus: &mut Proteus) {
        for idx in 0..gallery::TILE_COUNT {
            if !self.pending_gallery_tile_crop[idx] {
                continue;
            }
            let tile = self.gallery.tiles[idx];
            if tile.baked_image_size(proteus).is_none() {
                continue;
            }
            self.gallery.tile_full[idx].copy_baked_image_from(proteus, tile);
            tile.center_crop_to_square(proteus);
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
    /// actually appears, not from when the click happened. Mirrors
    /// `proteus-shell-native::advance_gallery_fetch`'s timeout/error half
    /// and `advance_demo`'s `Loading`-arm auto-advance check, combined into
    /// one function since this crate has no separate settle-tick/drive-tick
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
            proteus
                .world_mut()
                .entity_mut(self.loading.error_text.id())
                .insert(Visibility::VISIBLE);
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
    /// settled in `Gallery` (i.e. after whichever incoming grid morph
    /// revealed the tiles has actually completed — not just started), and
    /// out the instant a `Gallery`→elsewhere morph begins. Paced by
    /// `gallery_group_transition_config()`'s own duration so it lands at
    /// `0`/`1` right as that morph completes/starts. The button has no
    /// background fill (same transparent-idle-fill convention as the nav
    /// buttons), so border/glow/label alpha are what actually reads as
    /// "fading in/out" — without this, the button only ever popped
    /// instantly to fully visible/invisible, since neither had any other
    /// alpha owner. Reported directly ("should quickly fade in and out, not
    /// just pop in"). "Settled" is read from the tiles' own real
    /// `Visibility` rather than a fixed-duration timer (which is what this
    /// crate used before this fix, via `queue_reveal`) — group transitions
    /// never write a target's own `QuadState`/`Visibility` until the whole
    /// group actually completes (see `Handle::split_to`'s doc), so this
    /// tracks the *real* completion, not a guess that happens to usually
    /// match it. Mirrors `proteus-shell-native::advance_gallery_button_fade`
    /// exactly, modulo that "settled" substitution (source has a single
    /// crate-wide `self.transition` flag this crate's state machine doesn't
    /// track — same kind of per-entity substitute `Demo::advance_hovers`'
    /// doc already explains for the identical reason).
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
        let vis = if fade > 0.0 {
            Visibility::VISIBLE
        } else {
            Visibility::HIDDEN
        };
        proteus
            .world_mut()
            .entity_mut(self.gallery.fetch_button.id())
            .insert(vis);
        if let Some(mut border) = proteus
            .world_mut()
            .get_mut::<Border>(self.gallery.fetch_button.id())
        {
            border.color.w = fade;
        }
        if let Some(mut glow) = proteus
            .world_mut()
            .get_mut::<Glow>(self.gallery.fetch_button.id())
        {
            glow.color.w = fade;
        }
        if let Some(mut label) = proteus
            .world_mut()
            .get_mut::<Text>(self.gallery.fetch_button_label.id())
        {
            label.color.w = fade;
        }
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
    /// Mirrors `proteus-shell-native::advance_gallery_error_fade` exactly.
    fn advance_gallery_error_fade(&mut self, proteus: &mut Proteus, dt: f32) {
        let target = if self.gallery_error_shown { 0.0 } else { 1.0 };
        // Reuses the same source constant `screens::theme`'s own sun/moon
        // fade does (`proteus-shell-native::NAV_ICON_FADE_DURATION`) — not
        // a coincidence, this crate just doesn't have one shared name for
        // it, mirroring source's own reuse of that same constant here.
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
    /// geometry (position/size/scale/corner_radius — including mid-morph
    /// values, since `enlarged` is still animating during the
    /// `Gallery↔GalleryImage` transition) and crossfades its alpha in once
    /// two things are both true: it has a real baked image (the hires fetch
    /// landed and the shell's generic bake pass picked it up —
    /// `Handle::baked_image_size`), and `enlarged` has fully settled
    /// (`visible`, not mid-transition). Gating on *both* — not just "has a
    /// bake" — matters because the hires fetch routinely finishes well
    /// before the ~0.6s grid morph does; crossfading in mid-morph would
    /// read as the sharp image popping in ahead of the tile finishing its
    /// own growth, so the low-res stand-in holds until the morph settles,
    /// only *then* crossfades. `gallery_hires_fade` only ramps up, never
    /// down, within one visit — `start_gallery_to_image` resets it to
    /// `0.0` for the next one. A no-op whenever `self.state` isn't
    /// `GalleryImage` at all. Mirrors
    /// `proteus-shell-native::advance_gallery_hires_overlay`.
    ///
    /// Deliberately does *not* re-derive `enlarged`'s box from the hires
    /// bake's own decoded pixel size — an earlier version of this function
    /// did exactly that (`Handle::baked_image_size` → `gallery::
    /// large_image_quad`, replacing the box the instant the hires bake
    /// landed), reasoning that the low-res fetch's own real decoded shape
    /// and the hires fetch's real decoded shape are two independent
    /// approximations of "the same" aspect ratio that can disagree by a
    /// hair even after `start_gallery_to_image`'s own rounding fix. True,
    /// but source (`proteus-shell-native::advance_gallery_hires_overlay`,
    /// checked directly) never does this either — `gallery_enlarged_base`'s
    /// box is set once, in `start_gallery_to_image`, from
    /// `gallery_tile_aspect[idx]` (the photo's real catalog aspect, known
    /// before *either* fetch happens), and never touched again. Reported
    /// bug (a real photo's low-res→hires crossfade visibly shifting a px
    /// or two, always the same direction, only on some photos — picsum
    /// center-crops each fetch to whatever integer aspect it was asked
    /// for, and the low-res and hires requests round that same real aspect
    /// at wildly different target resolutions, so they occasionally land
    /// on very slightly different picsum crops): the box-correction above
    /// was the actual cause, not a fix for it — resizing/repositioning the
    /// box the instant the hires bake lands *is* a visible geometry pop
    /// whenever those two crops disagree, exactly the moment a user is
    /// looking right at the image. Matching source (box never moves, both
    /// low-res and hires just stretch into the one box `start_gallery_to_
    /// image` already built) turns that same tiny crop disagreement into
    /// an imperceptible sub-pixel stretch of content within a static box,
    /// instead of a shifting box around static content.
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
        let vis = if has_bake && base.visible {
            Visibility::VISIBLE
        } else {
            Visibility::HIDDEN
        };
        proteus
            .world_mut()
            .entity_mut(self.gallery.hires_overlay.id())
            .insert(vis);
    }

    /// Finalizes `gallery.fetch_button`'s geometry once its label has
    /// baked — see `screens::gallery::fetch_button_quad`'s doc. Safe
    /// unconditionally, every tick: unlike `apply_examples_home_layout`,
    /// the button is never itself a group-transition target (only the 12
    /// tiles are — see `start_loading_to_gallery`'s doc), so there's no
    /// live transition on this entity to fight.
    fn apply_gallery_fetch_button_layout(&mut self, proteus: &mut Proteus) {
        if let Some(qs) = gallery::fetch_button_quad(proteus, &self.gallery, self.viewport_size) {
            self.gallery.fetch_button.set_declared_geometry(proteus, qs);
        }
    }

    /// Drives `example_detail`'s "Continuous Animation" box while
    /// `ExampleDetail(2)` is active — paused (not reset) otherwise, so it
    /// resumes from wherever it left off rather than restarting each time.
    /// Mirrors `proteus-shell-native::advance_example_animation`'s own
    /// `self.state != AppState::ExampleDetail(2)` guard.
    fn advance_example_animation(&mut self, proteus: &mut Proteus, dt: f32) {
        if self.state != AppState::ExampleDetail(2) {
            return;
        }
        self.transforms_anim_elapsed += dt;
        example_detail::advance_continuous_animation(
            proteus,
            &self.example_detail,
            self.transforms_anim_elapsed,
        );
    }

    fn next_random_u32(&mut self) -> u32 {
        let mut x = self.stress_rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.stress_rng = x;
        x
    }

    fn random_unit_f32(&mut self) -> f32 {
        (self.next_random_u32() as f32) / (u32::MAX as f32)
    }

    /// A fresh random target for one Burst Spawn particle — uniform within
    /// `panel`'s bounds (minus padding and half the item size), excluding a
    /// `RESULT_TEXT_RESERVED_HEIGHT_PX` strip at the panel's bottom so
    /// particles never sit under the result text. Scale 0.6–1.4×, a random
    /// hue. Mirrors `proteus-shell-native::random_burst_target`.
    fn random_burst_target(&mut self, panel: &QuadState) -> QuadState {
        let half_size = example_detail::BURST_ITEM_SIZE / 2.0;
        let half_w = (panel.size.x / 2.0 - example_detail::PANEL_PADDING_PX - half_size).max(0.0);
        let x = panel.position.x + (self.random_unit_f32() * 2.0 - 1.0) * half_w;

        let half_h = (panel.size.y / 2.0 - example_detail::PANEL_PADDING_PX - half_size).max(0.0);
        let y_min = panel.position.y - half_h + example_detail::RESULT_TEXT_RESERVED_HEIGHT_PX;
        let y_max = panel.position.y + half_h;
        let y = y_min + self.random_unit_f32() * (y_max - y_min).max(0.0);

        let scale = 0.6 + self.random_unit_f32() * 0.8;
        let rgb = example_detail::hsv_to_rgb(self.random_unit_f32() * 360.0);

        QuadState {
            position: Vec3::new(x, y, example_detail::CONTENT_Z),
            size: Vec2::splat(example_detail::BURST_ITEM_SIZE),
            rotation: 0.0,
            scale,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(rgb.x, rgb.y, rgb.z, 1.0),
            corner_radius: 4.0,
        }
    }

    /// One Texture Churn slot's rest position — a fixed
    /// `example_detail::TEXTURE_CHURN_COLS`-column grid centered on `panel`.
    fn texture_churn_slot_quad(i: usize, panel: &QuadState) -> QuadState {
        let cols = example_detail::TEXTURE_CHURN_COLS;
        let rows = example_detail::TEXTURE_CHURN_SLOTS.div_ceil(cols);
        let size = example_detail::TEXTURE_CHURN_SLOT_SIZE;
        let gap = example_detail::TEXTURE_CHURN_GAP_PX;
        let total_w = cols as f32 * size + (cols as f32 - 1.0) * gap;
        let total_h = rows as f32 * size + (rows as f32 - 1.0) * gap;
        let col = (i % cols) as f32;
        let row = (i / cols) as f32;
        let x = panel.position.x - total_w / 2.0 + size / 2.0 + col * (size + gap);
        let y = panel.position.y + total_h / 2.0 - size / 2.0 - row * (size + gap);
        QuadState {
            position: Vec3::new(x, y, example_detail::CONTENT_Z),
            size: Vec2::splat(size),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: 8.0,
        }
    }

    /// Spawns `example_detail::BURST_SPAWN_COUNT` particles and starts a
    /// `STRESS_TEST_DURATION`-second run — no-op if one's already in
    /// progress. Mirrors `proteus-shell-native::run_burst_spawn`.
    fn run_burst_spawn(&mut self, proteus: &mut Proteus) {
        if self.stress_run.is_some() {
            return;
        }
        let Some(panel) = proteus.get(self.example_detail.panel) else {
            return;
        };
        let panel = panel.geometry;
        let entities: Vec<Handle> = (0..example_detail::BURST_SPAWN_COUNT)
            .map(|_| {
                let target = self.random_burst_target(&panel);
                proteus.component(ComponentSpec::new(target).non_interactive())
            })
            .collect();
        self.stress_run = Some(StressRun {
            kind: StressKind::BurstSpawn,
            elapsed: 0.0,
            frame_count: 0,
            entities,
            churn_iterations: 0,
        });
    }

    /// Spawns `example_detail::TEXTURE_CHURN_SLOTS` fixed slots (flat white
    /// until `advance_texture_churn_entities` gives each its first texture
    /// next tick) and starts a run — no-op if one's already in progress.
    /// Mirrors `proteus-shell-native::run_texture_churn`.
    fn run_texture_churn(&mut self, proteus: &mut Proteus) {
        if self.stress_run.is_some() {
            return;
        }
        let Some(panel) = proteus.get(self.example_detail.panel) else {
            return;
        };
        let panel = panel.geometry;
        let entities: Vec<Handle> = (0..example_detail::TEXTURE_CHURN_SLOTS)
            .map(|i| {
                let qs = Self::texture_churn_slot_quad(i, &panel);
                proteus.component(ComponentSpec::new(qs).non_interactive())
            })
            .collect();
        self.stress_run = Some(StressRun {
            kind: StressKind::TextureChurn,
            elapsed: 0.0,
            frame_count: 0,
            entities,
            churn_iterations: 0,
        });
    }

    /// The top-level Stress Tests driver — advances `elapsed`, finalizes
    /// once `STRESS_TEST_DURATION` is reached, otherwise dispatches to
    /// whichever kind is running. Mirrors
    /// `proteus-shell-native::advance_stress_test`.
    fn advance_stress_test(&mut self, proteus: &mut Proteus, dt: f32) {
        let Some(run) = &mut self.stress_run else {
            return;
        };
        run.elapsed += dt;
        run.frame_count += 1;
        if run.elapsed >= example_detail::STRESS_TEST_DURATION {
            self.finalize_stress_test(proteus);
            return;
        }
        match run.kind {
            StressKind::BurstSpawn => self.advance_burst_spawn_entities(proteus),
            StressKind::TextureChurn => self.advance_texture_churn_entities(),
        }
    }

    /// Re-targets every *idle* particle (its prior `animate_to` has
    /// settled) to a fresh random position — a sustained sweep across the
    /// whole run rather than a one-shot spawn, since each particle
    /// retriggers roughly every `BURST_SPAWN_ITEM_DURATION` seconds.
    /// Mirrors `proteus-shell-native::advance_burst_spawn_entities`.
    fn advance_burst_spawn_entities(&mut self, proteus: &mut Proteus) {
        let Some(run) = &self.stress_run else { return };
        if run.kind != StressKind::BurstSpawn {
            return;
        }
        let entities = run.entities.clone();
        let Some(panel) = proteus.get(self.example_detail.panel) else {
            return;
        };
        let panel = panel.geometry;
        let config = TransitionConfig {
            duration: example_detail::BURST_SPAWN_ITEM_DURATION,
            delay: 0.0,
            easing: ease_in_out_quad,
        };
        for entity in entities {
            let idle = proteus.get(entity).is_some_and(|d| d.transition.is_none());
            if idle {
                let target = self.random_burst_target(&panel);
                entity.animate_to(proteus, target, config);
            }
        }
    }

    /// Churns every slot's texture *every* tick (unlike Burst Spawn's
    /// idle-gated retrigger — a texture swap is instantaneous, so there's
    /// no animation to wait out between cycles) — computes each slot's
    /// fresh synthetic RGBA buffer and queues it via
    /// `pending_texture_churn` for the shell to actually register. Mirrors
    /// `proteus-shell-native::advance_texture_churn_entities`/`churn_texture`.
    fn advance_texture_churn_entities(&mut self) {
        let Some(run) = &self.stress_run else { return };
        if run.kind != StressKind::TextureChurn {
            return;
        }
        let entities = run.entities.clone();
        for &handle in &entities {
            let width = (example_detail::TEXTURE_CHURN_SIZE_MIN
                + self.random_unit_f32() * example_detail::TEXTURE_CHURN_SIZE_SPREAD)
                as u32;
            let height = (example_detail::TEXTURE_CHURN_SIZE_MIN
                + self.random_unit_f32() * example_detail::TEXTURE_CHURN_SIZE_SPREAD)
                as u32;
            let rgb = example_detail::hsv_to_rgb(self.random_unit_f32() * 360.0);
            let pixel = [
                (rgb.x * 255.0) as u8,
                (rgb.y * 255.0) as u8,
                (rgb.z * 255.0) as u8,
                255,
            ];
            let rgba = pixel.repeat((width * height) as usize);
            self.pending_texture_churn.push(TextureChurnUpdate {
                handle,
                width,
                height,
                rgba,
            });
        }
        if let Some(run) = &mut self.stress_run {
            run.churn_iterations += entities.len() as u32;
        }
    }

    /// Drains this tick's Texture Churn updates for the shell to register —
    /// see [`TextureChurnUpdate`]'s doc. Call once per tick, after
    /// [`Demo::tick`], whenever GPU resources are available.
    pub fn take_pending_texture_churn(&mut self) -> Vec<TextureChurnUpdate> {
        std::mem::take(&mut self.pending_texture_churn)
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
    /// means the shell should stop delivering results for whatever hires
    /// fetch it currently has in flight. See `pending_gallery_hires_cancel`'s
    /// doc.
    pub fn take_pending_gallery_hires_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_gallery_hires_cancel)
    }

    /// Drains this tick's pending video-start request, if any — `Some(idx)`
    /// means the shell should probe/decode whichever file index `idx`
    /// (0/1/2, matching `screens::video_tiles`' left/center/right tiles)
    /// maps to and start pushing frames into its `VideoFrameSender` (see
    /// the crate-root doc: decoding stays a shell concern). The entity
    /// already shows the video texture by the time this fires — see
    /// `pending_video_start`'s doc.
    pub fn take_pending_video_start(&mut self) -> Option<usize> {
        self.pending_video_start.take()
    }

    /// Drains this tick's pending video-stop request — `true` means the
    /// shell should stop whatever decode is currently running and release
    /// its GPU video texture (e.g. `QuadPipeline::suspend_video`).
    pub fn take_pending_video_stop(&mut self) -> bool {
        std::mem::take(&mut self.pending_video_stop)
    }

    /// Tells `Demo` a real decoded video frame has actually landed for the
    /// currently-playing tile — the shell's own job is just detecting that
    /// (e.g. `QuadPipeline::consume_video_frame` returning `true`) and
    /// calling this once; `Demo` has no way to see the GPU texture itself.
    /// Drives `Demo::advance_video_loading`'s loading-dots/error visibility.
    /// A no-op call (e.g. after the tile has already moved on) is harmless —
    /// this just sets a flag `start_tiles_to_screen` resets on the next
    /// visit anyway. Mirrors `proteus-shell-native`'s own
    /// `PlayingVideo::first_frame_shown` latch (set the same way, from
    /// `consume_video_frame`'s return value).
    pub fn set_video_first_frame_shown(&mut self) {
        self.video_first_frame_shown = true;
    }

    /// Drains this tick's pending video-cancel signal — `true` means a load
    /// just timed out and the shell should abort whatever fetch/decode is
    /// still in flight for it, *without* stopping/tearing down playback the
    /// way [`Demo::take_pending_video_stop`] means: `Demo` hasn't given up
    /// on this tile, it's just showing the error text now instead of
    /// pulsing dots, and playback may yet succeed if a response is close.
    /// See `pending_video_cancel`'s own doc for why this exists (native's
    /// local decode never needed it; the web shell's real network fetch
    /// does).
    pub fn take_pending_video_cancel(&mut self) -> bool {
        std::mem::take(&mut self.pending_video_cancel)
    }

    /// Ends the current run naturally: despawns every entity in bulk and
    /// reports the result via `stress.result_text`. `Text` doesn't support
    /// in-place content changes (see `StressContent::result_text`'s doc),
    /// so updating `.content` alone wouldn't actually re-render — freeing
    /// the old `BakedText` forces the shell's next bake pass to pick it
    /// back up. Mirrors `proteus-shell-native::finalize_stress_test`.
    fn finalize_stress_test(&mut self, proteus: &mut Proteus) {
        let Some(run) = self.stress_run.take() else {
            return;
        };
        for &entity in &run.entities {
            entity.destroy(proteus);
        }
        let avg_fps = run.frame_count as f32 / run.elapsed;
        // A real, deliberate cap (both shells run `PresentMode::AutoVsync`),
        // not a stress-test bottleneck — a result that bumped up against it
        // deserves a callout rather than reading like this demo can't push
        // past ~60 FPS on its own.
        let vsync_note = if avg_fps >= VSYNC_FPS_CAP_THRESHOLD {
            " (FPS capped at 60 by vsync)"
        } else {
            ""
        };
        let result = match run.kind {
            StressKind::BurstSpawn => format!(
                "Burst Spawn: {} entities, avg {avg_fps:.1} FPS over {:.1}s{vsync_note}",
                run.entities.len(),
                example_detail::STRESS_TEST_DURATION
            ),
            StressKind::TextureChurn => format!(
                "Texture Churn: {} register/evict cycles, avg {avg_fps:.1} FPS over {:.1}s{vsync_note}",
                run.churn_iterations,
                example_detail::STRESS_TEST_DURATION
            ),
        };
        let result_text = self.example_detail.stress.result_text;
        if let Some(mut text) = proteus.world_mut().get_mut::<Text>(result_text.id()) {
            text.content = result;
        }
        result_text.free_resources(proteus);
    }

    /// Ends the current run early, if any — despawns its entities without
    /// reporting a result (distinct from `finalize_stress_test`, the
    /// natural-completion path). Called whenever the user navigates away
    /// from `ExampleDetail(3)` mid-run: this content is standalone (see
    /// `screens::example_detail`'s module doc), so without this it would
    /// keep existing and rendering over whatever comes next.
    fn cancel_stress_test(&mut self, proteus: &mut Proteus) {
        if let Some(run) = self.stress_run.take() {
            for entity in run.entities {
                entity.destroy(proteus);
            }
        }
    }

    /// Shows the photosensitivity warning only once *settled* and idle on
    /// `ExampleDetail(3)` with no test running — hidden the instant either
    /// button starts a run, reappears once it ends. Called every tick,
    /// unconditionally, so it also self-corrects on the way out of
    /// `ExampleDetail(3)` (no separate exit hook needed) — see
    /// `ExampleDetail::content_handles`'s doc for why this entity is
    /// managed separately from the rest of the category's content.
    ///
    /// `panel`'s own `Visibility` stands in for the original's single
    /// crate-wide `self.transition.is_none()` gate (this crate's state
    /// machine doesn't track one): `self.state` flips to `ExampleDetail(3)`
    /// the instant the merge *starts*, well before the panel has actually
    /// finished morphing into place — but during an N→1 merge, the
    /// *destination* (`panel`) is exactly what `n_to_one_setup_system`
    /// hides for the transition's whole duration, revealing it again only
    /// once every virtual has completed and `reveal_on_complete` fires (the
    /// N *source* entities are what actually animate, as virtual clones —
    /// `panel` itself never gets its own `ActiveTransition` mid-merge, so
    /// checking that directly — the first thing tried here — doesn't work).
    /// So gating on `panel.visible` is not a workaround but the literal
    /// signal already being maintained for exactly this purpose. Without
    /// this gate, the warning popped in mid-transition instead of only once
    /// things had actually settled — a real bug, not a hypothetical
    /// (reported directly against the running app).
    fn advance_stress_warning_visibility(&mut self, proteus: &mut Proteus) {
        let panel_visible = proteus
            .get(self.example_detail.panel)
            .map(|d| d.visible)
            .unwrap_or(false);
        let visible =
            panel_visible && self.state == AppState::ExampleDetail(3) && self.stress_run.is_none();
        let vis = if visible {
            Visibility::VISIBLE
        } else {
            Visibility::HIDDEN
        };
        proteus
            .world_mut()
            .entity_mut(self.example_detail.stress.warning_text.id())
            .insert(vis);
    }

    /// Ramps every registered [`HoverEntry`]'s progress toward 1 while
    /// hovered, 0 otherwise, and writes the result onto that entity's
    /// `Glow.radius`/`QuadState.scale` — see that type's doc for the
    /// mechanism and the constants this mirrors. Suppressed (ramps toward
    /// 0 regardless of the hover flag) while `handle` itself has an active
    /// transition (`ComponentData::transition.is_some()`) — a settled
    /// per-entity check, not a single crate-wide "is *anything*
    /// transitioning" flag like the original's `self.transition` (this
    /// crate's state machine doesn't track one), but equivalent in
    /// practice: every entity the original suppresses hover on during a
    /// transition is itself one of that transition's own sources/targets,
    /// so it already has its own active transition at that moment too.
    /// Mirrors `proteus-shell-native::advance_nav_hover`'s ramp/write
    /// shape, generalized — see the crate-root doc's design note for why
    /// this is one shared function instead of one per screen.
    fn advance_hovers(&mut self, proteus: &mut Proteus, dt: f32) {
        for entry in &mut self.hovers {
            let suppressed = proteus
                .get(entry.handle)
                .map(|d| d.transition.is_some())
                .unwrap_or(true);
            let target = if !suppressed && entry.is_hovering.get() {
                1.0
            } else {
                0.0
            };
            let step = dt / HOVER_GLOW_DURATION_SECS;
            if entry.progress < target {
                entry.progress = (entry.progress + step).min(target);
            } else if entry.progress > target {
                entry.progress = (entry.progress - step).max(target);
            }
            let progress = entry.progress;
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(entry.handle.id()) {
                glow.radius = progress * HOVER_GLOW_MAX_RADIUS_PX;
            }
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(entry.handle.id()) {
                qs.scale = 1.0 + progress * HOVER_SCALE_BOOST;
            }
        }
    }

    /// `gallery.enlarged`'s hover reaction is glow-only, no scale-boost —
    /// it's already as big as the grid's own box allows, so growing it
    /// further on hover would read as an odd wobble rather than an
    /// affordance, unlike every other hover-registered surface in this
    /// crate. `Demo::advance_hovers`' shared engine has no per-entity way to
    /// opt out of the scale half of its ramp, so this forces `enlarged`'s
    /// scale back to a flat `1.0` immediately after that generic pass runs
    /// — its own glow (driven by the same `HoverEntry`, registered
    /// alongside everything else in `Demo::new`) is untouched. Mirrors
    /// `proteus-shell-native::advance_gallery_enlarged_hover`'s own explicit
    /// "no scale-boost" design call.
    fn advance_gallery_enlarged_hover_scale(&mut self, proteus: &mut Proteus) {
        if let Some(mut qs) = proteus
            .world_mut()
            .get_mut::<QuadState>(self.gallery.enlarged.id())
        {
            qs.scale = 1.0;
        }
    }

    /// Drives `video_tiles.tile_overlays`/`tile_labels` — the hover-only
    /// black tint + title label, plus the screen-scale bump once resting as
    /// the video screen. Glow/scale on the tile itself are the shared
    /// `Demo::advance_hovers` engine's job (registered in `Demo::new`); this
    /// reads each tile's already-ramped progress back out of `self.hovers`
    /// rather than duplicating that ramp. Also suppresses hover entirely on
    /// the settled, playing screen tile — clicking/hovering it is a no-op,
    /// so there's nothing to invite feedback for (same `Handle::
    /// set_interactive` mutual-exclusion pattern `nav.home` already uses for
    /// its own resting-suppression). Mirrors
    /// `proteus-shell-native::advance_tile_hover` exactly — its own
    /// `transitioning` suppression is already covered for free by
    /// `advance_hovers`' per-entity `ActiveTransition` check: `tiles[idx]`
    /// genuinely gets one during `start_tiles_to_screen`'s 1:1 `animate_to`.
    fn advance_tile_hover(&mut self, proteus: &mut Proteus) {
        let screen_focus_idx = match self.state {
            AppState::VideoScreen(idx) => Some(idx),
            _ => None,
        };
        for i in 0..3 {
            let tile = self.video_tiles.tiles[i];
            tile.set_interactive(proteus, screen_focus_idx != Some(i));

            let progress = self
                .hovers
                .iter()
                .find(|h| h.handle.id() == tile.id())
                .map(|h| h.progress)
                .unwrap_or(0.0);

            // Recomputed every tick from the tile's own *current* geometry —
            // tile-shaped in grid view, the screen's very different
            // proportions once settled, anything in between mid-morph.
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
            let label_scale = if screen_focus_idx == Some(i) {
                video_tiles::TILE_LABEL_SCREEN_SCALE
            } else {
                1.0
            };
            if let Some(mut label_qs) = proteus.world_mut().get_mut::<QuadState>(label.id()) {
                label_qs.scale = label_scale;
            }
        }
    }

    /// Ramps the currently-playing tile's `VideoCrossfade.video_t` from
    /// `0.0` (box-cover poster art) to `1.0` (live video) in lockstep with
    /// `start_tiles_to_screen`'s own geometry morph — eased the same way
    /// (`ease_in_out_quad`, matching `group_transition_config()`'s own
    /// choice, since `TransitionData` doesn't expose which easing fn is
    /// actually driving it), so both read as one motion instead of two
    /// separate effects. Once the morph settles (`proteus.get(tile)
    /// .transition` goes `None`), forces `video_t` to `1.0` outright: local
    /// `.mp4` playback (the only kind this crate does — no HLS/network
    /// fetch) decodes its first frame fast enough that it's essentially
    /// always ready well before the ~0.4s morph itself finishes in the
    /// common case; `Demo::advance_video_loading` takes over from there for
    /// the genuinely-slow-decode case.
    ///
    /// While actually mid-morph (not yet settled), also mirrors
    /// `proteus-shell-native::advance_tiles_to_screen_fade`'s other half —
    /// checked directly against source after a real, reported bug ("you can
    /// see the other tiles over the transitioning tile"): this crate's own
    /// F5d fix only ever ported the `video_t` ramp above, missing two more
    /// things source's own function does in the same breath:
    /// - Fades the clicked tile's *own* alpha toward `0.0` in lockstep with
    ///   `video_t` (`1.0 - eased_t`, gated on `!ready` — a fast decode that's
    ///   already showing a real frame before the morph even finishes must
    ///   *not* have this fade it back out, only to have `advance_video_
    ///   loading` snap it back to `1.0` the instant the transition
    ///   settles — a one-frame flicker on exactly the path that never had a
    ///   problem). This is what actually reveals `advance_video_loading`'s
    ///   `backdrop` starting *during* the morph, not just once settled —
    ///   without it, the tile stayed fully opaque (showing whatever
    ///   `VideoCrossfade` blended, poster art or a real frame) for the
    ///   entire morph and only snapped transparent the instant it settled,
    ///   reading as an abrupt pop rather than a dissolve.
    /// - Fades the *other two* tiles' own alpha, `Border.color.w`, and
    ///   `Glow` (radius forced to `0`, color alpha faded too) toward `0.0`,
    ///   over *half* the morph's own duration (`fade_t` reaches `1.0` at
    ///   `raw_t == 0.5`) — without this, the two untouched tiles just sit
    ///   there fully opaque for the whole morph. Once the growing/settled
    ///   screen's own opacity is *also* fading toward `0.0` (the point
    ///   above), z-order between it and the idle siblings stops being
    ///   enough to hide them on its own — `advance_video_loading`'s
    ///   `backdrop` is deliberately z-ordered to still occlude these two
    ///   once faded (see that entity's own doc), but only for the *tracked*
    ///   tile's footprint; fading the siblings themselves is still needed
    ///   so they don't just sit there fully visible next to/behind it.
    ///   `advance_hovers`/`advance_tile_hover` already zero these tiles'
    ///   hover-only decorations (overlay/label) whenever nothing's actually
    ///   hovering them, which is always true here (the mouse is over the
    ///   *clicked* tile) — this only needs to additionally fade each tile's
    ///   own base appearance.
    ///
    /// Only the *forward* direction gets any of this: `start_screen_to_
    /// tiles`'s reverse morph is a baked-slice crossfade (a frozen snapshot
    /// up front, and a fresh full-opacity target state for all 3 tiles —
    /// see `video_tiles::tile_target_state`'s doc), so there's no *live*
    /// content to fade in the first place, matching source's own identical
    /// asymmetry. Called every tick, unconditionally — a no-op outside
    /// `VideoScreen` (nothing has `VideoCrossfade` then) and a no-op on
    /// `Handle::set_video_crossfade`'s own end once `stop_video` has removed
    /// it.
    fn advance_video_crossfade(&mut self, proteus: &mut Proteus) {
        let AppState::VideoScreen(idx) = self.state else {
            return;
        };
        let tile = self.video_tiles.tiles[idx];
        let raw_t = proteus
            .get(tile)
            .and_then(|d| d.transition)
            .map(|t| t.progress);
        tile.set_video_crossfade(proteus, raw_t.map(ease_in_out_quad).unwrap_or(1.0));

        let Some(raw_t) = raw_t else {
            return;
        };
        let eased_t = ease_in_out_quad(raw_t);

        if !self.video_first_frame_shown {
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(tile.id()) {
                qs.color.w = 1.0 - eased_t;
            }
        }

        let fade_t = (raw_t * 2.0).min(1.0);
        let fade_alpha = 1.0 - ease_out_quad(fade_t);
        for (i, &other) in self.video_tiles.tiles.iter().enumerate() {
            if i == idx {
                continue;
            }
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(other.id()) {
                qs.color.w = fade_alpha;
            }
            if let Some(mut border) = proteus.world_mut().get_mut::<Border>(other.id()) {
                border.color.w = fade_alpha;
            }
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(other.id()) {
                glow.radius = 0.0;
                glow.color.w = fade_alpha;
            }
        }
    }

    /// Video screen loading UI: a black backdrop tracking the tile's live
    /// geometry (so the screen never looks broken mid-morph, before the
    /// first real frame even lands), 3 phase-staggered pulsing dots once
    /// settled and still waiting, replaced by inline error text after
    /// `video_tiles::VIDEO_LOAD_TIMEOUT_SECS`. Runs unconditionally, every
    /// tick, same as source — every branch below is gated on locally
    /// computed booleans rather than an early return, so a click away from
    /// `VideoScreen` mid-wait still correctly hides everything on the very
    /// next tick. Mirrors `proteus-shell-native::advance_video_loading`
    /// exactly, with one structural difference: source inspects its own
    /// hand-rolled `self.transition` to tell "mid-morph" from "settled";
    /// this crate reads the tile's own `ActiveTransition` component
    /// directly (`d.transition`, same technique `advance_video_crossfade`
    /// above already uses) — `self.state` here flips to `VideoScreen(idx)`
    /// immediately rather than waiting for the morph to settle (see
    /// `start_tiles_to_screen`'s doc), so it alone already covers what
    /// source needs its `video_idx`/`self.transition` pair for.
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

        let backdrop_visible = video_idx.is_some();
        proteus
            .world_mut()
            .entity_mut(self.video_tiles.backdrop.id())
            .insert(if backdrop_visible {
                Visibility::VISIBLE
            } else {
                Visibility::HIDDEN
            });
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
                    // the morph it's currently at) and strictly above the
                    // untouched idle siblings, throughout the entire morph
                    // and once settled alike.
                    qs.position.z = (video_tiles::TILE_Z + tile_state.geometry.position.z) / 2.0;
                    qs.size = tile_state.geometry.size;
                    qs.scale = tile_state.geometry.scale;
                    qs.corner_radius = tile_state.geometry.corner_radius;
                }
            }
        }

        // `ready` mirrors `proteus-shell-native`'s own
        // `playing_video.is_some_and(|p| p.first_frame_shown)` check.
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
        // through again. `stop_video`'s own callers already restore this
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
            proteus
                .world_mut()
                .entity_mut(dot.id())
                .insert(if dots_visible {
                    Visibility::VISIBLE
                } else {
                    Visibility::HIDDEN
                });
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
        proteus
            .world_mut()
            .entity_mut(self.video_tiles.error_text.id())
            .insert(if error_visible {
                Visibility::VISIBLE
            } else {
                Visibility::HIDDEN
            });
    }

    /// Drives the whole light/dark theme system off `theme_progress` — see
    /// the crate-root design note for the full mechanism this consolidates
    /// from the original's own `advance_theme`. Runs every tick,
    /// unconditionally, regardless of `AppState`, so a component already
    /// reflects the current theme by the time it becomes visible, with no
    /// visibility branching needed here. Per-screen corner-radius/color
    /// blend calls (`blend_corner_radius`/`blend_primary_color`) and
    /// dark-overlay crossfades land here incrementally as each screen's own
    /// fidelity pass wires them up — this function only owns what's
    /// screen-agnostic: the ramp itself, the sun/moon toggle, and
    /// `background`'s crossfade (persistent chrome, not owned by any one
    /// screen).
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
        self.theme.sun.set_interactive(proteus, self.dark_target);
        self.theme.moon.set_interactive(proteus, !self.dark_target);

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
        // *away* going dark, so its alpha runs inverted from every other
        // overlay.
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

        // Fade sun/moon in once past Splash, forever after — same
        // "chrome_visible" gate `apply_nav_visibility`-adjacent code uses
        // elsewhere in this crate (state flips synchronously here, unlike
        // the original's explicit from/to transition tracking, so a plain
        // `self.state` check is the equivalent simplification already used
        // throughout this crate, not a new one).
        let chrome_visible = if self.state == AppState::Splash {
            0.0
        } else {
            1.0
        };
        if chrome_visible > 0.0 {
            proteus
                .world_mut()
                .entity_mut(self.theme.sun.id())
                .insert(Visibility::VISIBLE);
            proteus
                .world_mut()
                .entity_mut(self.theme.moon.id())
                .insert(Visibility::VISIBLE);
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

        // `home`'s nav buttons — corner radius (a currently-no-op blend,
        // `NAV_BUTTON_CORNER_RADIUS_DARK` equals the light value in the
        // original too, but wired anyway per this pass's design decision
        // to match the original's actual behavior rather than only its
        // *currently* visible differences) and Border/Glow/label RGB.
        for &button in &self.home.nav_buttons {
            blend_corner_radius(proteus, button, home::CORNER_RADIUS, home::CORNER_RADIUS, p);
            blend_primary_color(proteus, button, p);
        }
        for &label in &self.home.nav_labels {
            blend_primary_color(proteus, label, p);
        }

        // `nav.home`/`nav.back` — Glow only (border+glyph are baked into
        // the PNG art itself, and neither has a `Text` label), and both
        // icons' dark overlays above (unlike this pair) are a plain
        // continuous crossfade, not hard-gated — see `screens::nav`'s doc.
        blend_primary_color(proteus, self.nav.home, p);
        blend_primary_color(proteus, self.nav.back, p);

        // `home_selected`/`home_selected_dark` share `nav_home_selected_fade`
        // (`Demo::advance_nav_icons`' envelope) but split it by a hard
        // `dark_target` gate rather than a true bilinear (page-selected ×
        // theme) blend — mirrors `proteus-shell-native::advance_theme`
        // step 8 exactly, including its own doc's reasoning for why this one
        // spot isn't a continuous lerp like everything else in this
        // function.
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

        // `examples_home`'s 6 category buttons — same corner-radius/
        // Border/Glow/label treatment as `home`'s nav buttons above
        // (`examples_home::CORNER_RADIUS`'s dark counterpart is likewise
        // numerically identical, wired anyway per this pass's design
        // decision — see that constant's own doc).
        for &button in &self.examples_home.buttons {
            blend_corner_radius(
                proteus,
                button,
                examples_home::CORNER_RADIUS,
                examples_home::CORNER_RADIUS,
                p,
            );
            blend_primary_color(proteus, button, p);
        }
        for &label in &self.examples_home.labels {
            blend_primary_color(proteus, label, p);
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

        // The 4 category titles — Text only. Deliberately narrower scope
        // than `content_handles` (each category's own row labels/content
        // boxes stay a fixed light-treatment violet, never blended) —
        // mirrors `proteus-shell-native::advance_theme`'s own selective
        // scope exactly, not an oversight.
        for &heading in &self.example_detail.headings {
            blend_primary_color(proteus, heading, p);
        }

        // Stress Tests' 2 buttons + labels — same treatment as `home`'s nav
        // buttons; unlike every other category's own content, these do get
        // the live theme lerp (see `ExampleDetail::headings`' doc).
        for &button in &self.example_detail.stress.buttons {
            blend_primary_color(proteus, button, p);
        }
        for &label in &self.example_detail.stress.button_labels {
            blend_primary_color(proteus, label, p);
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
            // (the tile↔screen morph itself), leave corner_radius alone —
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
        // during a `Loading`↔`Gallery` `GridSlice` transition (only the
        // virtuals animate), so it's always safe to reassert corner radius
        // unconditionally here, no "actively morphing" guard needed (unlike
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
        // NAV_BUTTON pair, matching source's own semantic pairing (see
        // `gallery::FETCH_BUTTON_CORNER_RADIUS`'s own doc).
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
    /// site" shape as `advance_theme`. Mirrors
    /// `proteus-shell-native::advance_nav_icons` exactly, including its
    /// per-icon fade-envelope/target split.
    ///
    /// `home` is up on every state past `Splash` — including `Home` itself,
    /// unlike `back` (idle-screens-only) — since it's also persistent brand
    /// chrome, not just a "how do I get home" affordance; the `home_selected`
    /// overlay is what actually communicates "you're here" on top of it, not
    /// visibility of the icon itself. Called before `advance_theme` (matches
    /// the original's `advance_demo(); advance_nav_icons(); advance_theme();`
    /// order) so that function always has the final say on `home_selected`'s
    /// actual alpha split for the frame.
    fn advance_nav_icons(&mut self, proteus: &mut Proteus, dt: f32) {
        // `home` fades in/out with `Splash` alone — no group-transition
        // `from`/`to` distinction to make here, since (unlike the original)
        // this crate flips `self.state` synchronously at the *start* of the
        // splash→home morph rather than only once it lands (see
        // `Demo::advance_state`) — so a plain state check already carries
        // the "the instant the morph begins, not once it ends" timing the
        // original gets from checking its in-flight `transition.from`.
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
        // (mirrors `advance_tile_hover`'s `is_idle_screen` suppression for
        // the same reason). Toggling `Interactable` off is what actually
        // suppresses it: `advance_hovers`' generic ramp only reads each
        // entity's `is_hovering` flag, which `hit_test_system` itself stops
        // updating (and immediately fires a hover-exit for) once an entity
        // drops out of the interactable set — same mechanism `advance_theme`
        // already uses for the sun/moon mutual-exclusion toggle. Idempotent
        // every tick, same as that call.
        self.nav
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
            proteus
                .world_mut()
                .entity_mut(self.nav.lockup.id())
                .insert(Visibility::VISIBLE);
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
            qs.color.w = self.nav_lockup_fade;
        }

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
                proteus
                    .world_mut()
                    .entity_mut(icon.id())
                    .insert(Visibility::VISIBLE);
            }
            let step = dt / nav::FADE_DURATION_SECS;
            if self.nav_icon_fade[i] < target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] + step).min(target);
            } else if self.nav_icon_fade[i] > target {
                self.nav_icon_fade[i] = (self.nav_icon_fade[i] - step).max(target);
            }
            if target <= 0.0 && self.nav_icon_fade[i] <= 0.0 {
                proteus
                    .world_mut()
                    .entity_mut(icon.id())
                    .insert(Visibility::HIDDEN);
            }

            // Position + fade only — hover glow/scale is `advance_hovers`'
            // job (both icons are already registered via `register_hover`
            // in `Demo::new`); this only additionally ties the glow's own
            // alpha to the icon's fade envelope so it can't show through
            // before the icon itself has faded in.
            if let Some(mut qs) = proteus.world_mut().get_mut::<QuadState>(icon.id()) {
                qs.position.x = xs[i];
                qs.position.y = y;
                qs.color.w = self.nav_icon_fade[i];
            }
            if let Some(mut glow) = proteus.world_mut().get_mut::<Glow>(icon.id()) {
                glow.color.w = self.nav_icon_fade[i];
            }
        }
    }

    /// Pointer position in **world-space** (viewport-center origin, Y-up) —
    /// see [`proteus_sdk::Proteus::pointer_moved`]'s doc for the exact
    /// contract and the conversion a caller needs from window/CSS pixels.
    pub fn pointer_moved(&mut self, proteus: &mut Proteus, pos: Option<Vec2>) {
        proteus.pointer_moved(pos);
    }

    pub fn pointer_pressed(&mut self, proteus: &mut Proteus) {
        proteus.pointer_pressed();
    }

    pub fn pointer_released(&mut self, proteus: &mut Proteus) {
        proteus.pointer_released();
    }

    /// The hires upgrade's crossfade overlay entity — see
    /// `screens::gallery::Gallery::hires_overlay`'s doc. Baking stays a
    /// shell concern (this crate is headless — see the crate-root doc), but
    /// unlike every other baked entity, this one specifically wants a
    /// *bigger* resize cap than the rest of the gallery grid — only one
    /// hires image is ever resident at a time, so it can afford a much
    /// larger on-screen footprint than any of the 12 simultaneous tile
    /// thumbnails (mirrors `proteus-shell-native::bake_gallery_hires_image`'s
    /// own separate, bigger-cap bake pass). Exposed so the shell can single
    /// this one entity out for that treatment instead of applying one
    /// resize cap to every baked image uniformly.
    pub fn gallery_hires_overlay(&self) -> Handle {
        self.gallery.hires_overlay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test harness — owns the [`Proteus`] the M13.1 port moved out of
    /// [`Demo`], and runs the full engine-style frame each `tick`
    /// (`Proteus::tick` → `Demo::advance` → `Proteus::refresh_cascades`, the
    /// same order [`crate::DemoApp`]'s engine does). Derefs to `Demo` for
    /// reads; `.app` is the world these tests inspect directly via
    /// `Proteus::get` / `world()`.
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

    /// Forward a `Demo` method that now takes `&mut Proteus` as its first
    /// arg, so tests can keep calling `harness.start_foo()`.
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
        fn start_home_to_examples();
        fn start_examples_to_home();
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
        fn start_examples_to_detail(idx: usize);
        fn start_detail_to_examples();
        fn start_detail_to_home();
        fn run_burst_spawn();
        fn run_texture_churn();
        fn set_gallery_hires_image(idx: usize, bytes: Vec<u8>);
        fn set_gallery_tile_image(idx: usize, bytes: Vec<u8>, aspect: Vec2);
        fn pointer_moved(pos: Option<Vec2>);
        fn pointer_pressed();
        fn pointer_released();
    }

    /// How long a test needs to tick to be certain `Demo` has moved past
    /// `Splash` — derived from the real constants (delay + intro + hold,
    /// plus a small margin) rather than a hardcoded literal, so a change to
    /// any of them (including a deliberate temporary one, e.g. while
    /// visually verifying the Splash sequence — see `splash::HOLD_SECS`'s
    /// own doc) can't silently desync every test that needs to get past
    /// Splash first.
    fn past_splash_secs() -> f32 {
        splash::INTRO_DELAY_SECS + splash::INTRO_DURATION_SECS + splash::HOLD_SECS + 0.5
    }

    /// `collect_instances` draws root entities in ascending `position.z`
    /// order; a tie falls back to ECS iteration order, which tracks
    /// archetype creation (roughly "when this exact set of components was
    /// first seen"), not spawn time or draw intent. `background` gains its
    /// `Image` component *after* every other screen has already spawned
    /// (via `Demo::set_background_image`, called from the shell once
    /// startup finishes) — landing it in a newer archetype than content
    /// spawned earlier with a *matching* z, which silently drew it on top,
    /// hiding that content entirely. Every root entity meant to be visible
    /// over the background must have a strictly greater z — this guards
    /// that invariant for every screen reachable from a fresh `Demo`,
    /// rather than relying on visual inspection to catch the next one.
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
        for &button in &demo.home.nav_buttons {
            assert_above(&demo, button, "home.nav_buttons");
        }

        // VideoTiles, then back to Home before continuing on to ExamplesHome.
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

        // ExamplesHome.
        demo.start_home_to_examples();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::ExamplesHome);
        for &button in &demo.examples_home.buttons {
            assert_above(&demo, button, "examples_home.buttons");
        }
        assert_above(&demo, demo.nav.home, "nav.home");

        // ExampleDetail(0).
        demo.start_examples_to_detail(0);
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::ExampleDetail(0));
        assert_above(&demo, demo.example_detail.panel, "example_detail.panel");
        assert_above(&demo, demo.nav.back, "nav.back");
    }

    /// Regression test for the Splash intro fade sequence
    /// (`Demo::advance_intro`/`Demo::advance_state`'s Splash arm): invisible
    /// for `INTRO_DELAY_SECS`, fades 0 → 1 over `INTRO_DURATION_SECS`, then
    /// holds at full opacity for `HOLD_SECS` before handing off to `Home`.
    /// Every other Splash-adjacent test fast-forwards straight past all of
    /// this with `past_splash_secs()`; this one actually samples mid-flight,
    /// so a regression in the ramp itself (as opposed to "does it eventually
    /// reach Home") gets caught.
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

    fn advance_to_stress_tests() -> Harness {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        demo.start_home_to_examples();
        demo.tick(1.0);
        demo.start_examples_to_detail(3);
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::ExampleDetail(3));
        demo
    }

    #[test]
    fn burst_spawn_runs_then_despawns_and_reports_a_result() {
        let mut demo = advance_to_stress_tests();

        demo.run_burst_spawn();
        let run = demo.stress_run.as_ref().expect("run should have started");
        assert_eq!(run.entities.len(), example_detail::BURST_SPAWN_COUNT);
        let sample = run.entities[0];

        // A second trigger while one's already running must no-op, not
        // spawn a duplicate batch.
        demo.run_burst_spawn();
        assert_eq!(
            demo.stress_run.as_ref().unwrap().entities.len(),
            example_detail::BURST_SPAWN_COUNT
        );

        let mut t = 0.0;
        while t < example_detail::STRESS_TEST_DURATION + 0.5 {
            demo.tick(0.1);
            t += 0.1;
        }
        assert!(demo.stress_run.is_none(), "run should have finalized");
        assert!(
            demo.app.get(sample).is_none(),
            "particles must be despawned once the run finalizes"
        );
        let result = demo
            .app
            .world()
            .get::<Text>(demo.example_detail.stress.result_text.id())
            .unwrap();
        assert!(
            result.content.contains("Burst Spawn"),
            "result text should report what ran, got {:?}",
            result.content
        );
        // Regression guard: the message used to omit the avg-FPS figure
        // `proteus-shell-native::finalize_stress_test` reports — reported
        // directly as "the test result text doesn't match the demo."
        assert!(
            result.content.contains("avg") && result.content.contains("FPS"),
            "result text should report the run's average FPS, got {:?}",
            result.content
        );
    }

    #[test]
    fn texture_churn_queues_updates_and_counts_iterations() {
        let mut demo = advance_to_stress_tests();

        demo.run_texture_churn();
        assert_eq!(
            demo.stress_run.as_ref().unwrap().entities.len(),
            example_detail::TEXTURE_CHURN_SLOTS
        );

        demo.tick(0.1);
        let updates = demo.take_pending_texture_churn();
        assert_eq!(
            updates.len(),
            example_detail::TEXTURE_CHURN_SLOTS,
            "every slot should churn every tick"
        );
        for u in &updates {
            assert_eq!(u.rgba.len(), (u.width * u.height * 4) as usize);
        }

        let mut t = 0.0;
        while t < example_detail::STRESS_TEST_DURATION + 0.5 {
            demo.tick(0.1);
            t += 0.1;
        }
        assert!(demo.stress_run.is_none());
        let result = demo
            .app
            .world()
            .get::<Text>(demo.example_detail.stress.result_text.id())
            .unwrap();
        assert!(result.content.contains("Texture Churn"));
    }

    #[test]
    fn navigating_away_mid_run_cancels_it_without_a_result() {
        let mut demo = advance_to_stress_tests();

        demo.run_burst_spawn();
        let sample = demo.stress_run.as_ref().unwrap().entities[0];
        demo.tick(0.1);

        demo.start_detail_to_examples();

        assert!(demo.stress_run.is_none());
        assert!(
            demo.app.get(sample).is_none(),
            "particles must be despawned when navigating away mid-run"
        );
        let result = demo
            .app
            .world()
            .get::<Text>(demo.example_detail.stress.result_text.id())
            .unwrap();
        assert_eq!(
            result.content, " ",
            "cancelling shouldn't report a result — only a natural finish does"
        );
    }

    /// Simulates a real pointer click on each of the 6 category buttons
    /// rather than calling `start_examples_to_detail` directly — the
    /// earlier z-order test caught geometry mistakes but not a
    /// missing/absent `on_click` registration, which is exactly the bug
    /// this test would have caught (`examples_home.buttons[3]`'s "Stress
    /// Tests" tile was left out of the wiring loop in `Demo::new` when Step
    /// 4 landed; 4/5 — "Layout"/"3D" — the same way, until the "not built
    /// yet" placeholder landed). Each `idx` gets a fresh `Demo` —
    /// deliberately not a single instance round-tripping through all six:
    /// repeatedly re-declaring several `examples_home.buttons`' geometry
    /// (needed headlessly, since `examples_home::layout` never resolves
    /// without real text baking) before a *second* `GridSlice` split
    /// surfaced a separate, not-yet-root-caused issue where
    /// `reveal_on_complete` never fires for that second split — reproduced
    /// only in the no-GPU fallback path (`one_to_n_setup_system`'s
    /// baked-crossfade branch requires `GpuContext`/`QuadPipeline`, absent
    /// here); the real app always has those, so it's very likely
    /// fallback-path-only, but hasn't been confirmed against a GPU-backed
    /// run. Worth another look if category navigation ever stops responding
    /// after visiting several categories in a row.
    #[test]
    fn each_wired_examples_home_category_button_opens_its_own_example_detail() {
        for idx in 0..6 {
            let mut demo = Harness::new();
            let mut t = 0.0;
            while t < past_splash_secs() {
                demo.tick(0.05);
                t += 0.05;
            }
            demo.start_home_to_examples();
            demo.tick(1.0);
            assert_eq!(demo.state, AppState::ExamplesHome);

            // Isolate this button from its five siblings, which headlessly
            // all still sit at the same fallback position — safe here
            // since it's this `Demo`'s first-ever `set_declared_geometry`
            // call on it (see the doc above for why a *second* one, later,
            // isn't).
            let button = demo.examples_home.buttons[idx];
            let mut geometry = demo.app.get(button).unwrap().geometry;
            geometry.position = Vec3::new(5000.0, 5000.0, 0.5);
            button.set_declared_geometry(&mut demo.app, geometry);

            demo.pointer_moved(Some(Vec2::new(5000.0, 5000.0)));
            demo.pointer_pressed();
            demo.tick(1.0);

            assert_eq!(
                demo.state,
                AppState::ExampleDetail(idx),
                "clicking category button {idx} should open ExampleDetail({idx})"
            );
        }
    }

    /// Regression test for a real bug: `advance_stress_warning_visibility`
    /// only checked `self.state == ExampleDetail(3)`, but `self.state`
    /// flips to that the instant the merge *starts* (see
    /// `Demo::advance_nav_icons`'s doc for the same "flips synchronously,
    /// not once landed" convention) — so the warning popped in immediately,
    /// well before the panel had actually finished morphing into place.
    /// Reported directly against the running app.
    #[test]
    fn stress_warning_stays_hidden_until_the_panel_transition_settles() {
        let mut demo = advance_to_home();
        demo.start_home_to_examples();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::ExamplesHome);

        demo.start_examples_to_detail(3);
        assert_eq!(demo.state, AppState::ExampleDetail(3));

        // One small tick: the merge request has been inserted but not yet
        // processed into an `ActiveTransition` (that happens on the *next*
        // `Proteus::tick` — see `PendingTileReset`'s doc for the same
        // one-tick-later convention elsewhere in this crate), and the
        // panel is nowhere near its resting shape yet either way.
        demo.tick(0.05);
        assert!(
            !demo
                .app
                .get(demo.example_detail.stress.warning_text)
                .unwrap()
                .visible,
            "warning must stay hidden while the panel is still mid-transition"
        );

        // Still mid-transition (well under group_transition_config()'s 0.4s
        // duration).
        demo.tick(0.1);
        assert!(
            !demo
                .app
                .get(demo.example_detail.stress.warning_text)
                .unwrap()
                .visible,
            "warning must stay hidden partway through the transition"
        );

        // Past the transition's duration — settled.
        demo.tick(0.5);
        assert!(
            demo.app
                .get(demo.example_detail.stress.warning_text)
                .unwrap()
                .visible,
            "warning should appear once the panel has actually settled"
        );
    }

    #[test]
    fn clicking_videos_nav_button_opens_video_tiles_and_back_returns_home() {
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);

        // `home::layout` spreads the 3 buttons out even without real text
        // baking (flat `FALLBACK_SIZE` for every label — see that
        // function's doc), so reading the real, distinct position here
        // needs no manual isolation, unlike `examples_home`'s buttons
        // (which still rely on real baking for their own grid sizing).
        let button = demo.home.nav_buttons[0];
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

    #[test]
    fn hovering_a_tile_fades_in_its_overlay_and_title_label() {
        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        let probe = demo.app.get(tile).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        let mut t = 0.0;
        while t < HOVER_GLOW_DURATION_SECS + 0.5 {
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
        demo.tick(1.0); // settle the tile<->screen morph
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

    /// Regression test for a real bug, reported directly: "tiles keep color
    /// tint from the original bg colors. tiles shouldn't have any color
    /// tint." `advance_pending_tile_reset` used to reset a tile's whole
    /// `QuadState` — including `color` — straight back to `tile_quad`'s
    /// placeholder tint, discarding the untinted white `set_tile_image`
    /// establishes once real box-cover art lands. So the *first* trip
    /// through the video screen and back was fine, but the tint came back
    /// (multiplying whatever real art was showing) every time after.
    #[test]
    fn returning_from_video_screen_keeps_real_box_art_untinted() {
        use proteus_ui::BakedImage;

        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        // Simulate the shell's bake pass having landed real box-cover art —
        // real baking needs GPU, unavailable in this headless test world.
        demo.app
            .world_mut()
            .entity_mut(tile.id())
            .insert(BakedImage {
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
                page: 0,
                pixel_size: [400.0, 600.0],
            });
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
        // Regression test for a real, user-reported bug: "the tile
        // backgrounds on the non-transitioning tiles are missing when
        // transitioning back from playback to the tiles." Root cause: a
        // split's own reveal only ever flips `Visibility` — it never
        // rewrites a target's live `QuadState` — so the *other two* tiles,
        // faded fully transparent by `Demo::advance_video_crossfade` while
        // the clicked one was playing, stayed stuck at that alpha forever
        // once `advance_pending_tile_reset` only ever reset the *clicked*
        // tile's own `QuadState` (Border/Glow got a fix first, but that
        // alone wasn't the whole bug — this is the other half).
        use proteus_ui::BakedImage;

        let mut demo = advance_to_video_tiles();
        let tiles = demo.video_tiles.tiles;
        for &tile in &tiles {
            demo.app
                .world_mut()
                .entity_mut(tile.id())
                .insert(BakedImage {
                    uv_offset: [0.0, 0.0],
                    uv_scale: [1.0, 1.0],
                    page: 0,
                    pixel_size: [400.0, 600.0],
                });
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
                demo.app
                    .get(demo.video_tiles.tiles[idx])
                    .unwrap()
                    .geometry
                    .color
                    .w,
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

    /// Regression test for a real bug, reported directly as "z index issues
    /// with tiles and screen": `video_tiles.tiles[0..3]` are all root
    /// entities tied at the exact same z — `collect_instances` breaks that
    /// tie by iteration order, not visual intent, so the growing/settled
    /// video screen could draw *under* whichever sibling tile(s) happened
    /// to iterate later, clipped by their (untransformed) footprint
    /// wherever it overlapped the much bigger screen. Asserts the settled
    /// screen tile's own z has actually risen above its still-tile-shaped
    /// siblings', not just that geometry/color came out right.
    /// Regression test for a real bug, reported directly, complementing
    /// `returning_from_video_screen_keeps_real_box_art_untinted`: that test
    /// only proves the *settled* steady state is correct (which
    /// `advance_pending_tile_reset` alone already guarantees, regardless of
    /// this fix) — this one proves the transition's own mid-flight bake
    /// *target* is correct too, which is what's actually visible "on
    /// transition," per the user's own words. `tile[idx]` is both the
    /// source and one of the 3 targets of `start_screen_to_tiles`'s split
    /// (returning to its own grid slot) — before `split_to_with_states`,
    /// that target's state resolved from `declared_geometry(tile)`, which
    /// at this exact moment is still screen-sized (not yet reset), so the
    /// virtual converging back into this slot would visibly animate toward
    /// the *wrong* (stale, screen) shape/color instead of the real tile
    /// grid target.
    #[test]
    fn screen_to_tiles_virtual_targets_the_real_grid_shape_not_the_stale_screen_shape() {
        use bevy_ecs::query::With;
        use proteus_ui::{ActiveTransition, BakedImage, Virtual};

        let mut demo = advance_to_video_tiles();
        let tile = demo.video_tiles.tiles[0];
        demo.app
            .world_mut()
            .entity_mut(tile.id())
            .insert(BakedImage {
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
                page: 0,
                pixel_size: [400.0, 600.0],
            });
        if let Some(mut qs) = demo.app.world_mut().get_mut::<QuadState>(tile.id()) {
            qs.color = Vec4::ONE;
        }

        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();
        demo.tick(1.0);
        assert_eq!(demo.state, AppState::VideoScreen(0));

        demo.start_screen_to_tiles(0);
        // One small tick: enough for `one_to_n_setup_system` to spawn the
        // virtuals, nowhere near enough to finish the 0.4s morph.
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

    /// Regression test for a real gap, reported directly: "there is a
    /// crossfade of the tile background image, and the video background
    /// when in playback. That transition is not happening in the native
    /// preview." `Handle::start_video` alone always cuts instantly
    /// (`video_t: 1.0`, its own documented default) — `Demo::
    /// start_tiles_to_screen` must override that back down and
    /// `Demo::advance_video_crossfade` must ramp it back up in step with
    /// the geometry morph, or the crossfade this test is named for simply
    /// never happens.
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

        // Partway through the ~0.4s morph — neither endpoint yet.
        demo.tick(0.2);
        let mid = video_t(&demo, 0).expect("still playing");
        assert!(
            mid > 0.0 && mid < 1.0,
            "should be ramping partway through the morph, got {mid}"
        );

        // Past the morph's duration — settled.
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

        // Mid-morph: backdrop is visible and tracking from the very start —
        // `Demo::advance_video_crossfade` fades the clicked tile's own art
        // out *during* the morph too (not just once settled), so backdrop
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
        // Regression test for the real, user-reported "you can see the
        // other tiles over the transitioning tile" bug's actual root cause:
        // `proteus-shell-native::advance_tiles_to_screen_fade` also fades
        // the *other two* (non-clicked) tiles' own alpha/Border/Glow out
        // during the morph — a whole half of that source function this
        // crate's earlier F5d port had missed entirely. Also checks the
        // other half of the fix: Border/Glow don't ride the group-
        // transition's own QuadState interpolation, so returning to
        // VideoTiles must explicitly restore them (`advance_pending_tile_
        // reset`) or they'd stay faded forever after the first video.
        let mut demo = advance_to_video_tiles();
        demo.start_tiles_to_screen(0);
        demo.take_pending_video_start();

        // Partway through the morph: the other two tiles should already be
        // partly faded (fade reaches 0 at *half* the morph's own progress).
        demo.tick(0.15);
        for &idx in &[1usize, 2] {
            let tile = demo.app.get(demo.video_tiles.tiles[idx]).unwrap();
            assert!(
                tile.geometry.color.w < 1.0,
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
                tile.geometry.color.w, 0.0,
                "tile {idx} must be fully faded out once settled"
            );
        }

        // Back to the grid: every tile's Border/Glow must be fully restored,
        // not stuck at whatever alpha the fade-out left them at.
        demo.start_screen_to_tiles(0);
        let mut t = 0.0;
        while t < 1.0 {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::VideoTiles);
        for &tile in &demo.video_tiles.tiles {
            assert_eq!(
                demo.app.get(tile).unwrap().geometry.color.w,
                1.0,
                "tile's own alpha (not just Border/Glow) must be fully restored — a split's own \
                 reveal never rewrites a target's live QuadState on its own"
            );
            let border = demo.app.world().get::<Border>(tile.id()).unwrap();
            assert_eq!(border.color.w, 1.0, "border alpha must be fully restored");
            let glow = demo.app.world().get::<Glow>(tile.id()).unwrap();
            assert_eq!(glow.color.w, 1.0, "glow alpha must be fully restored");
            assert_eq!(glow.radius, 0.0, "glow radius must be back at rest");
        }
    }

    #[test]
    fn settled_video_backdrop_draws_above_the_idle_sibling_tiles_not_just_below_the_screen() {
        // Regression test for a real, user-reported bug: once settled and
        // waiting for the first real frame, the (now fully transparent —
        // see `advance_video_loading`'s own doc) tile no longer occludes
        // anything, so whatever `backdrop` doesn't cover shows through. The
        // two untouched idle sibling tiles sit well inside the settled
        // screen's much bigger footprint; if `backdrop`'s own z isn't
        // strictly *above* theirs, they render "in front of" the video
        // screen the user just opened. See `backdrop_quad`'s own doc for
        // the full mechanism (this crate's global z-sort vs. source's
        // draw-in-spawn-order renderer, which never had this problem).
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
        // Fine-grained ticks past the ~0.4s morph — settled, still waiting.
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
        // from here, independent of however long the morph itself took.
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
        // `take_pending_video_cancel` didn't exist before the web shell
        // needed it (native's local `.mp4` decode has no in-flight fetch to
        // abort on a timeout) — this locks in the edge-triggered "exactly
        // once, right when the timeout first latches" contract its own doc
        // promises, not just a level that stays true forever after.
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
        // Regression test: backing out of a still-loading video used to
        // leave the tile's alpha baked at 0 into the outgoing split's own
        // "from" snapshot — see `start_screen_to_tiles`'s own doc for why.
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
        demo.take_pending_video_start(); // drain, as the shell would
                                         // Let the "grow to screen" morph actually finish — in the real,
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
        // large `tick(1.0)` here (as the real app never does, but as this
        // test used to) collapses setup/animate/complete into one call, so
        // the tile is never *observably* hidden between two ticks — exactly
        // the window `advance_pending_tile_reset` needs (see
        // `PendingTileReset`'s doc) to safely reset the geometry.
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
        for &button in &demo.home.nav_buttons {
            assert!(demo.app.get(button).unwrap().visible);
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
    fn clicking_gallery_nav_button_opens_loading_and_back_via_home_icon_returns_home() {
        let mut demo = advance_to_home();

        // `home::layout` spreads the 3 buttons out even without real text
        // baking — see the identical note on
        // `clicking_videos_nav_button_opens_video_tiles_and_back_returns_home`.
        let button = demo.home.nav_buttons[1];
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
    fn opening_loading_queues_a_fetch_request_and_hides_the_nav_buttons() {
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
        for &button in &demo.home.nav_buttons {
            assert!(
                !demo.app.get(button).unwrap().visible,
                "nav buttons should be hidden once merged into the loading logo"
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
        // Regression guard for a real gap (part of F6): the spinning logo
        // used to keep looping forever behind the error text, which now
        // sits dead center where the logo would otherwise show through.
        assert_eq!(
            demo.app.get(demo.loading.logo).unwrap().geometry.color.w,
            0.0,
            "the logo must fade out once the error text takes its place"
        );
    }

    /// Reaches `Gallery` by calling `start_loading_to_gallery` directly,
    /// bypassing the "every tile baked" auto-advance gate — same pattern as
    /// `advance_to_video_tiles` calling `start_home_to_tiles` directly.
    /// `Handle::baked_image_size` can never return `Some` in this headless
    /// test world (baking needs a real `QuadPipeline`/GPU — see
    /// `Demo::set_gallery_tile_image`'s doc), so the auto-advance path
    /// itself is covered separately by the timeout/error test above, not by
    /// actually satisfying the gate here.
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
        while t < HOVER_GLOW_DURATION_SECS + 0.5 {
            demo.tick(0.05);
            t += 0.05;
        }

        let glow_radius = demo.app.world().get::<Glow>(tile.id()).unwrap().radius;
        assert!(
            (glow_radius - HOVER_GLOW_MAX_RADIUS_PX).abs() < 0.01,
            "fully-hovered tile should reach the max glow radius, got {glow_radius}"
        );
        let scale = demo.app.get(tile).unwrap().geometry.scale;
        assert!(
            (scale - (1.0 + HOVER_SCALE_BOOST)).abs() < 0.01,
            "fully-hovered tile should also scale up, got {scale}"
        );
    }

    /// Regression test for `gallery.enlarged`'s deliberately incomplete
    /// hover reaction: glow yes, scale no (see `Demo::advance_gallery_
    /// enlarged_hover_scale`'s doc for why) — a real, source-verified
    /// design difference from every other hover-registered surface in this
    /// crate, not an oversight worth "fixing" to match the others.
    #[test]
    fn hovering_the_enlarged_image_ramps_glow_but_never_scales() {
        use proteus_ui::Glow;

        let mut demo = advance_to_gallery_image(0);
        let enlarged = demo.gallery.enlarged;
        let probe = demo.app.get(enlarged).unwrap().geometry.position;
        demo.pointer_moved(Some(Vec2::new(probe.x, probe.y)));
        let mut t = 0.0;
        while t < HOVER_GLOW_DURATION_SECS + 0.5 {
            demo.tick(0.05);
            t += 0.05;
        }

        let glow_radius = demo.app.world().get::<Glow>(enlarged.id()).unwrap().radius;
        assert!(
            (glow_radius - HOVER_GLOW_MAX_RADIUS_PX).abs() < 0.01,
            "fully-hovered enlarged image should still reach the max glow radius, got {glow_radius}"
        );
        assert_eq!(
            demo.app.get(enlarged).unwrap().geometry.scale,
            1.0,
            "but must never scale up, unlike every other hover-registered surface"
        );
    }

    /// Regression test for a real gap, reported directly: "the 'fetch new
    /// images' should quickly fade in and out, not just pop in." Neither
    /// direction had any alpha ramp before this fix — `fetch_button`/
    /// `.fetch_button_label` only ever popped instantly to fully visible
    /// (via a fixed-duration `queue_reveal`) or fully hidden (a direct
    /// `Visibility::HIDDEN` insert), since `Border`/`Glow`/`Text` alpha had
    /// no other owner and stayed at their spawn-time value (already fully
    /// opaque) regardless of `Visibility`.
    #[test]
    fn gallery_fetch_button_fades_in_and_out_instead_of_popping() {
        use proteus_ui::Border;

        let border_alpha = |demo: &Harness| {
            demo.app
                .world()
                .get::<Border>(demo.gallery.fetch_button.id())
                .unwrap()
                .color
                .w
        };

        let mut demo = advance_to_gallery();
        assert_eq!(
            border_alpha(&demo),
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
        let mid = border_alpha(&demo);
        assert!(
            mid > 0.0 && mid < 1.0,
            "should be partway faded out, got {mid}"
        );

        // Past the fade's duration — fully gone.
        demo.tick(1.0);
        assert!(!demo.app.get(demo.gallery.fetch_button).unwrap().visible);
        assert_eq!(border_alpha(&demo), 0.0);
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
        for &button in &demo.home.nav_buttons {
            assert!(demo.app.get(button).unwrap().visible);
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
        // doc for why the physical-pixel cap is the shell's job, not this
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
        // Simulate the shell's bake pass landing a real (uncropped, 2:3)
        // image on the tile — real baking needs GPU, unavailable here.
        let uncropped = BakedImage {
            uv_offset: [0.0, 0.0],
            uv_scale: [1.0, 1.0],
            page: 0,
            pixel_size: [200.0, 300.0],
        };
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
        for &button in &demo.home.nav_buttons {
            assert!(demo.app.get(button).unwrap().visible);
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

        // Simulate the shell's generic bake pass landing a real image on
        // the overlay (what `set_gallery_hires_image` + a real
        // `bake_pending_images` call would produce together).
        let overlay = demo.gallery.hires_overlay;
        demo.app
            .world_mut()
            .entity_mut(overlay.id())
            .insert(BakedImage {
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
                page: 0,
                pixel_size: [800.0, 600.0],
            });

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

        // Regression test for a real, user-reported bug: on some photos
        // (not all — it depends on exactly how each of the low-res and
        // hires fetch's own independent aspect-rounding lands), the
        // low-res stand-in and the hires bake disagree on the photo's
        // exact aspect ratio by a hair, since picsum center-crops each
        // fetch to whatever integer aspect it was asked for and the two
        // fetches round the same real aspect at wildly different target
        // resolutions. An earlier version of `advance_gallery_hires_
        // overlay` "corrected" `enlarged`'s box to the hires bake's own
        // decoded aspect the instant it landed — which meant *any* such
        // disagreement was a visible geometry pop, on exactly the photo
        // the user is looking at. Source (`proteus-shell-native::
        // advance_gallery_hires_overlay`, checked directly) never
        // touches the box after `start_gallery_to_image` sets it — this
        // asserts our port now matches that: the box stays put, and only
        // `gallery_hires_fade`/`Visibility` change.
        let mut demo = advance_to_gallery_image(4);
        let size_before = demo.app.get(demo.gallery.enlarged).unwrap().geometry.size;

        // `gallery_tile_aspect[4]` is still the default `Vec2::ONE`
        // (square) — the hires bake's real shape (800x500, 8:5) disagrees
        // with it about as sharply as two independent roundings ever
        // could, making this the worst case for the old "correct the box"
        // behavior.
        let overlay = demo.gallery.hires_overlay;
        demo.app
            .world_mut()
            .entity_mut(overlay.id())
            .insert(BakedImage {
                uv_offset: [0.0, 0.0],
                uv_scale: [1.0, 1.0],
                page: 0,
                pixel_size: [800.0, 500.0],
            });

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
    fn home_layout_arranges_all_3_buttons_in_one_horizontal_row() {
        use proteus_ui::BakedText;

        let mut demo = Harness::new();
        // Simulate the shell's text-baking pass landing on all 3 labels —
        // real baking needs a font atlas/GPU, unavailable in this headless
        // test world.
        let nav_labels = demo.home.nav_labels;
        for &label in &nav_labels {
            demo.app
                .world_mut()
                .entity_mut(label.id())
                .insert(BakedText {
                    uv_offset: [0.0, 0.0],
                    uv_scale: [1.0, 1.0],
                    page: 0,
                    pixel_size: [100.0, 20.0],
                });
        }

        let states = home::layout(&demo.app, &demo.home);
        assert_eq!(
            states[0].position.y, states[1].position.y,
            "all 3 must share one row, not be stacked vertically"
        );
        assert_eq!(states[1].position.y, states[2].position.y);
        assert!(
            states[0].position.x < states[1].position.x
                && states[1].position.x < states[2].position.x,
            "must be laid out left-to-right, in title order: {states:?}"
        );
    }

    #[test]
    fn home_layout_spreads_buttons_out_even_when_no_label_has_baked_yet() {
        // No baking simulated at all — every label falls back to
        // FALLBACK_SIZE. Regression guard: an earlier version of
        // `home::layout` returned `Option<[QuadState; 3]>`, `None` unless
        // *every* label had baked, applied on a recurring gate rather than
        // once — in a headless world (or any world where baking is merely
        // slow) that gate never fired, so buttons stayed stuck at their
        // identical spawn-time placeholder position forever. The real
        // function must always produce a spread-out row, fallback or not.
        let demo = Harness::new();
        let states = home::layout(&demo.app, &demo.home);
        assert_eq!(states[0].position.y, states[1].position.y);
        assert_eq!(states[1].position.y, states[2].position.y);
        assert!(
            states[0].position.x < states[1].position.x
                && states[1].position.x < states[2].position.x,
            "must spread left-to-right even using the flat fallback size for every button: \
             {states:?}"
        );
    }

    /// Regression test for a real bug, reported directly: `examples_home`'s
    /// grid `layout()` sized each button to its *own* label width instead of
    /// its column's shared width (the wider of its top/bottom pair) — its
    /// own doc comment already described the correct, column-shared
    /// behavior, the implementation just didn't match it. Uses 6 visibly
    /// different label widths so any per-button (rather than per-column)
    /// sizing shows up immediately.
    #[test]
    fn examples_home_layout_shares_each_columns_width_across_its_top_and_bottom_button() {
        use proteus_ui::BakedText;

        let mut demo = Harness::new();
        let widths = [40.0, 220.0, 90.0, 150.0, 260.0, 20.0];
        let eh_labels = demo.examples_home.labels;
        for (&label, &w) in eh_labels.iter().zip(widths.iter()) {
            demo.app
                .world_mut()
                .entity_mut(label.id())
                .insert(BakedText {
                    uv_offset: [0.0, 0.0],
                    uv_scale: [1.0, 1.0],
                    page: 0,
                    pixel_size: [w, 20.0],
                });
        }

        let states =
            examples_home::layout(&demo.app, &demo.examples_home).expect("every label baked");
        for col in 0..3 {
            assert_eq!(
                states[col * 2].size.x,
                states[col * 2 + 1].size.x,
                "column {col}'s top/bottom buttons must share one width: {states:?}"
            );
        }
        // And it's the *wider* of the pair, not (say) always the top's:
        // column 0 is [40.0, 220.0], so its shared width must come from
        // the bottom button's 220px label, not the top's 40px one.
        assert!(
            (states[0].size.x - (220.0 + 2.0 * 15.0)).abs() < 0.01,
            "column 0's shared width should be driven by its wider (bottom) label, got {:?}",
            states[0].size.x
        );
    }

    #[test]
    fn reaching_home_from_splash_spreads_the_3_nav_buttons_out_horizontally() {
        // End-to-end regression test for the same bug
        // `home_layout_spreads_buttons_out_even_when_no_label_has_baked_yet`
        // guards directly — reaches `Home` the normal way (ticking through
        // `Splash`, no manual geometry pokes), matching exactly how the
        // real app is driven, and confirms the 3 buttons actually end up
        // distinct once there.
        let mut demo = Harness::new();
        let mut t = 0.0;
        while t < past_splash_secs() {
            demo.tick(0.05);
            t += 0.05;
        }
        assert_eq!(demo.state, AppState::Home);
        let positions: Vec<f32> = demo
            .home
            .nav_buttons
            .iter()
            .map(|&b| demo.app.get(b).unwrap().geometry.position.x)
            .collect();
        assert!(
            positions[0] < positions[1] && positions[1] < positions[2],
            "buttons must be spread left to right, not stacked on top of each other: {positions:?}"
        );
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

    /// Regression test for a real F3 bug: `nav.lockup` was missing
    /// `.non_interactive()`, and its bounding box (the full `lockup.png`,
    /// including trailing whitespace past the visible wordmark —
    /// `nav::LOGO_TEXT_RIGHT_PX`'s doc) overlaps `nav.home`'s own hit
    /// region. Clicking `home` right at that overlap hit `lockup` instead
    /// (spawned later, and hit-testing is last-hit-wins), silently eating
    /// the click — caught by
    /// `clicking_videos_nav_button_opens_video_tiles_and_back_returns_home`
    /// itself once F4's new content shifted entity spawn order enough to
    /// expose it, but asserted directly here so it can't regress silently
    /// again.
    /// Regression test for a real gap, reported directly: `examples_home`'s
    /// "Layout"/"3D" category buttons (4/5) used to be inert placeholders —
    /// spawned for grid-layout parity but never wired to a click handler at
    /// all, unlike `proteus-shell-native`'s own `example_buttons`, all 6 of
    /// which are clickable. Now wired the same way as 0–3, landing on a
    /// dedicated "not built yet" message instead of source's own literal
    /// blank card (a deliberate improvement, not a fidelity gap — a blank
    /// card reads as a bug, an explicit message doesn't).
    #[test]
    fn clicking_layout_or_3d_shows_the_not_built_yet_placeholder() {
        use proteus_ui::Text;

        for idx in [4, 5] {
            let mut demo = Harness::new();
            let mut t = 0.0;
            while t < past_splash_secs() {
                demo.tick(0.05);
                t += 0.05;
            }
            demo.start_home_to_examples();
            demo.tick(1.0);
            let button = demo.examples_home.buttons[idx];
            let mut geometry = demo.app.get(button).unwrap().geometry;
            geometry.position = Vec3::new(5000.0, 5000.0, 0.5);
            button.set_declared_geometry(&mut demo.app, geometry);
            demo.pointer_moved(Some(Vec2::new(5000.0, 5000.0)));
            demo.pointer_pressed();
            demo.tick(1.0);
            assert_eq!(demo.state, AppState::ExampleDetail(idx));

            let content = demo.example_detail.content_handles(idx);
            assert!(
                content.contains(&demo.example_detail.placeholder_message),
                "category {idx} should show the shared placeholder message"
            );
            let text = demo
                .app
                .world()
                .get::<Text>(demo.example_detail.placeholder_message.id())
                .unwrap();
            assert!(
                text.content.to_lowercase().contains("not built yet")
                    || text.content.to_lowercase().contains("isn't built yet"),
                "message should say the feature isn't built yet, got {:?}",
                text.content
            );
        }
    }

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
}
