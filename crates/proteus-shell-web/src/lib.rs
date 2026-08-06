//! `proteus-shell-web` — WebGL2 / WASM shell.
//!
//! Exposes two JavaScript-callable entry points:
//!
//! - `proteus_init(canvas_id)` — legacy stub kept for backward compatibility.
//! - `ProteusApp.init(canvas_id)` → `ProteusApp` — full WebGL2 demo.
//!
//! ## JavaScript usage
//!
//! ```js
//! import init, { ProteusApp } from './pkg/proteus_shell_web.js';
//! await init();
//! const app = await ProteusApp.init('my-canvas');
//!
//! let last = null;
//! function frame(ts) {
//!   const dt = last !== null ? ts - last : 0;
//!   last = ts;
//!   app.tick(dt);          // dt in milliseconds
//!   requestAnimationFrame(frame);
//! }
//! requestAnimationFrame(frame);
//! ```
//!
//! ## Architecture
//!
//! Identical to `proteus-shell-native` but uses the wgpu browser backend:
//! - `wgpu::Backends::all()` — prefers BROWSER_WEBGPU (Chrome 113+/Firefox 119+),
//!   falls back to GL/WebGL2 on older browsers
//! - `wgpu::SurfaceTarget::Canvas(canvas)` instead of a winit surface
//! - `wgpu::Limits::downlevel_webgl2_defaults()` as a conservative baseline
//!   (safe under both WebGPU and WebGL2)
//! - No `pollster`; `init` is `async fn` called directly from JS via `await`
//!
//! ## Backend selection rationale
//!
//! `Backends::GL` (WebGL2-only) hangs at `request_adapter` in Chrome builds
//! where native WebGPU is also present — the GL adapter future stalls waiting
//! on internal wgpu machinery that expects the WebGPU path.  `Backends::all()`
//! resolves this: wgpu picks BROWSER_WEBGPU first (fast, no stall), and falls
//! back to WebGL2 if the browser doesn't support WebGPU.
//!
//! ## Demo scene
//!
//! Kept identical to `proteus-shell-native` scene-for-scene so the two shells
//! never drift (see PLANNING.md's M9.6 note on a prior divergence): the
//! animated Proteus mark (brand/animated-logo, color-light treatment, 19
//! frames sweeping continuously while idle) plus the "PROTEUS" wordmark,
//! treated as one composite (centered as a whole, scaled to
//! `COMPOSITE_SCALE`, sliding in from the left as it fades in) standing in
//! for a "START" button — glows on hover, splits into three video tiles on
//! click (1→N Slice, baked crossfade), and — on clicking any tile — morphs
//! that tile directly into a video screen (plain 1→1) while the other two
//! fade out, reversing the same way on click.

use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Legacy stub (always compiled — keep for backward compatibility)
// ---------------------------------------------------------------------------

/// Legacy entry point.  The full `ProteusApp` class is preferred.
#[wasm_bindgen]
pub async fn proteus_init(canvas_id: String) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());
    log::info!("Proteus initializing on canvas #{canvas_id} (legacy stub)");
    Ok(())
}

// ---------------------------------------------------------------------------
// Full ProteusApp — wasm32 only
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod inner {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use glam::{Vec2, Vec3, Vec4};

    use proteus_render::{
        validate_atlas_config, AtlasConfig, FontAtlas, GpuContext, QuadPipeline, TextureId,
    };
    use proteus_ui::{
        collect_instances, ease_in_out_quad, ease_out_quad, transition::TransitionConfig,
        BakedImage, BakedText, Border, ChildOf, DropShadow, Entity, Glow, GroupSource, GroupTarget,
        HoveredEntity, Image, Interactable, InteractionEvents, Lifecycle, MergeLayout,
        NToOneRequest, OneToNRequest, Opacity, PointerInput, ProteusWorld, QuadState,
        SplitStrategy, Text, TextureRef, TransitionRequest, VideoCrossfade, VideoPlayer,
        Visibility,
    };

    // -------------------------------------------------------------------------
    // Design tokens  (identical to proteus-shell-native)
    // -------------------------------------------------------------------------

    /// App background — #cdc7ed. Only visible at the very edges/before the
    /// background image loads — it's sized to cover the whole canvas every
    /// frame (see `advance_background`).
    const BG_COLOR: wgpu::Color = wgpu::Color {
        r: 0xCD as f64 / 255.0,
        g: 0xC7 as f64 / 255.0,
        b: 0xED as f64 / 255.0,
        a: 1.0,
    };

    /// Design System — Color-light treatment primary (#735acc). Border, glow,
    /// and idle text/icon color all draw from this one value, per the UI
    /// Design System Spec ("there are no separate per-component colors").
    /// Also the mark/wordmark's own color (see `violet()`'s original use),
    /// and the video tiles/screen's border/glow color.
    fn violet() -> Vec4 {
        Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
    }

    /// Design System — Color-dark treatment primary (#b6a8ff). Used as the
    /// video tile's hover-label color: the "opposite treatment's primary"
    /// for Color-light is Color-dark.
    fn violet_dark() -> Vec4 {
        Vec4::new(182.0 / 255.0, 168.0 / 255.0, 1.0, 1.0)
    }

    /// Border and label color — white.
    fn white() -> Vec4 {
        Vec4::ONE
    }

    /// Convert a fully-saturated, full-value HSV color (`hue_deg` in
    /// degrees, wrapped to `[0, 360)`; s=1, v=1 fixed) to RGB — standard
    /// six-sector conversion. Used only by the Transforms & Animation
    /// screen's continuous rainbow hue cycle
    /// (`advance_example_animation`), which has no other place in this
    /// demo to live given every other color is a fixed design token.
    fn hsv_to_rgb(hue_deg: f32) -> Vec3 {
        let h = hue_deg.rem_euclid(360.0) / 60.0;
        let x = 1.0 - (h % 2.0 - 1.0).abs();
        match h as i32 {
            0 => Vec3::new(1.0, x, 0.0),
            1 => Vec3::new(x, 1.0, 0.0),
            2 => Vec3::new(0.0, 1.0, x),
            3 => Vec3::new(0.0, x, 1.0),
            4 => Vec3::new(x, 0.0, 1.0),
            _ => Vec3::new(1.0, 0.0, x),
        }
    }

    /// Tiny xorshift32 PRNG step — this file has no `rand`/`fastrand`
    /// dependency, and Burst Spawn's random target positions/colors/scales
    /// don't need cryptographic-quality randomness, just enough jitter to
    /// look lively. `state` must be seeded nonzero (xorshift's one hard
    /// requirement) — see `stress_rng_state`'s doc.
    fn next_random_u32(state: &mut u32) -> u32 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *state = x;
        x
    }

    /// One xorshift32 step, mapped to `[0.0, 1.0)`.
    fn random_unit_f32(state: &mut u32) -> f32 {
        (next_random_u32(state) as f32) / (u32::MAX as f32)
    }

    const BORDER_WIDTH: f32 = 5.0;

    /// Seconds to wait before the entry fade begins.
    const INTRO_DELAY: f32 = 1.0;
    /// Seconds for the entry fade (opacity 0 → 1).
    const INTRO_DURATION: f32 = 0.6;
    /// Seconds the splash holds, fully settled, before auto-advancing to tiles.
    const SPLASH_HOLD_DURATION: f32 = 1.5;
    /// Seconds for a full 0 → 30 px (or 30 → 0 px) hover glow sweep.
    const GLOW_DURATION: f32 = 0.25;
    const GLOW_MAX_RADIUS: f32 = 15.0;
    /// Design System: "5% scale-up" on hover/focus for buttons, tiles, and icon buttons.
    const HOVER_SCALE_BOOST: f32 = 0.07;

    /// Seconds for the button ↔ tiles morph, either direction.
    const BUTTON_TILES_MORPH_DURATION: f32 = 0.4;

    // -------------------------------------------------------------------------
    // Animated logo (replaces the old navy-circle "START" button)
    // -------------------------------------------------------------------------
    //
    // brand/animated-logo/Loading Animation Spec.dc.html: a 19-frame loop, one
    // hatch band fading to 60% opacity at a time, sweeping the diagonal.
    // Unlike `set_tile_image`, there's no `Image`/`bake_pending_images`
    // round-trip here — JS fetches all 19 frames once at startup and hands
    // each straight to `add_logo_frame`, which bakes it into main_atlas
    // immediately (eternal — see the eviction-safety note on
    // `TextureRegistry::register_static`, this content must never be evicted
    // mid-loop) and stores its `(TextureId, BakedImage)` for
    // `advance_logo_animation` to cycle through.

    /// color-light treatment (Deep Violet mark, white hatch) — reads with the
    /// most contrast against this app's mid-gray (#BBBBBB) background; see the
    /// Brand Spec's color table for the other three treatments.
    const LOGO_FRAME_COUNT: usize = 19;

    /// Brand spec's suggested playback rate (~11fps).
    const LOGO_FRAME_DURATION: f32 = 0.09;

    /// Source frame art (208×288) is noticeably larger than the mark's
    /// on-screen footprint (144×200, `LOGO_MARK_WIDTH`/`LOGO_MARK_HEIGHT`) —
    /// downscale before packing into `main_atlas`, same reasoning as
    /// `MAX_TILE_IMAGE_SIDE`. Matters much more here than for a single tile:
    /// adding the color-dark frame set (for the Loading screen's
    /// theme-crossfaded logo) doubled this animation's atlas footprint to 38
    /// eternal entries, which at native resolution alone exceeded the
    /// atlas's remaining capacity (confirmed via a real `main_atlas full`
    /// failure on native).
    const LOGO_FRAME_MAX_SIDE: u32 = 220;

    /// The mark's native aspect ratio is 104:144 (13:18, portrait) — the
    /// button keeps roughly the same on-screen footprint as the old
    /// 200px-diameter circle by matching its height.
    const LOGO_MARK_HEIGHT: f32 = 200.0;
    const LOGO_MARK_WIDTH: f32 = LOGO_MARK_HEIGHT * 104.0 / 144.0;

    // -------------------------------------------------------------------------
    // Wordmark — "PROTEUS", to the right of the animated mark
    // -------------------------------------------------------------------------
    //
    // A `Text` child of the button (M10 composition — same pattern as the old
    // "START" label, tile titles, etc.), not a replacement for the mark
    // itself. Brand Spec's Typography section: Inter weight 700 (the
    // embedded font is exactly this — see proteus-render's font_atlas.rs),
    // all caps, letter-spacing 0.06em, same color as the mark. Sized by
    // scaling the pre-built reference lockup
    // (`brand/logo/assets/lockup-color-light.svg`, mark 144×0.305≈43.9px
    // tall, wordmark font-size 20px, gap 14.3px) up to this demo's
    // LOGO_MARK_HEIGHT — preserves the reference's proportions rather than
    // guessing new ones.

    const WORDMARK_TEXT: &str = "PROTEUS";
    const WORDMARK_SIZE_PX: f32 = 90.0;
    /// Brand Spec: "letter-spacing 0.06em" — 0.06 × font size.
    const WORDMARK_LETTER_SPACING_PX: f32 = WORDMARK_SIZE_PX * 0.06;
    /// Gap between the mark's right edge and the wordmark's left edge.
    const WORDMARK_GAP_PX: f32 = 65.0;

    /// Mark + wordmark render as one composite (see `advance_intro_and_hover`'s
    /// centering/slide-in logic) at this fraction of their natural size.
    /// Applied as `button`'s own `QuadState::scale` rather than shrinking
    /// every size/gap constant above: the wordmark is a *child* of `button`,
    /// and `hierarchy::compose_with_parent` already scales a child's local
    /// position/size by the parent's composed scale, so one `scale` factor
    /// moves and shrinks both the mark and the wordmark together, correctly,
    /// for free. It also means the wordmark still bakes at its full
    /// `WORDMARK_SIZE_PX` resolution and is only *displayed* smaller —
    /// crisper than rasterizing directly at the smaller size.
    const COMPOSITE_SCALE: f32 = 0.75;

    /// How far left of its resting (centered) position the composite starts,
    /// sliding in as it fades in — see `advance_intro_and_hover`.
    const INTRO_SLIDE_DISTANCE_PX: f32 = 250.0;

    // -------------------------------------------------------------------------
    // Video tiles — placeholder "box cover" art
    // -------------------------------------------------------------------------
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
    const TILE_CORNER_RADIUS_DARK: f32 = 20.0;

    const TILE_COLORS: [Vec4; 3] = [
        Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Big Buck Bunny
        Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
        Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
    ];

    /// Real box-cover photos are routinely far larger than the tiles'
    /// on-screen footprint (200×300) — cap decoded images to this before
    /// packing them into `main_atlas` (2048×2048, shared with baked text),
    /// which they'd otherwise not fit in at all or would starve of remaining
    /// space. Also shared by the full-window background crossfade layers
    /// (`advance_background`) for the same reason. Was 600 (1.5x the tile
    /// height) — dropped to 400 once the light/dark theme toggle added two
    /// more background layers plus a full icon set competing for the same
    /// atlas: at 600, the two backgrounds plus the three tile images alone
    /// didn't fit, and `bake_pending_images` has no failure backoff, so an
    /// entity that can't fit retries its full decode+resize+register
    /// attempt every single frame forever (a `main_atlas full` warning
    /// spamming every frame is the tell). The atlas itself can't be grown to
    /// compensate — it must stay within what `downlevel_webgl2_defaults()`
    /// guarantees for the real WebGL2 target this shell runs on.
    const MAX_TILE_IMAGE_SIDE: u32 = 400;

    /// Title shown in the hover overlay (M10) — each tile's `tile_labels[idx]`
    /// `Text` child.
    const TILE_TITLES: [&str; 3] = ["Big Buck Bunny", "Sintel", "Jellyfish"];

    /// Hover-overlay label size.
    const TILE_LABEL_SIZE_PX: f32 = 16.0;
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

    // -------------------------------------------------------------------------
    // Photo gallery — nature-themed images fetched from loremflickr.com, shown as a 4x3 grid
    // -------------------------------------------------------------------------

    const GALLERY_COLS: usize = 4;
    const GALLERY_ROWS: usize = 3;
    const GALLERY_MARGIN_LEFT: f32 = 40.0;
    const GALLERY_MARGIN_RIGHT: f32 = 40.0;
    const GALLERY_MARGIN_TOP: f32 = 40.0;
    const GALLERY_MARGIN_BOTTOM: f32 = 100.0;
    /// Fixed, not "at least" — the spec's "at least 20px" is satisfied as a
    /// floor by always using exactly this value; cell size is derived from
    /// whatever space remains after margins + these fixed gaps.
    const GALLERY_GAP_PX: f32 = 20.0;
    /// Applied on top of the margin/gap-derived cell size so the grid reads
    /// a touch less cramped against the window edges.
    const GALLERY_CELL_SCALE: f32 = 0.9;
    /// The grid block is bottom-anchored (see `gallery_cell_quad`): its
    /// bottom edge sits exactly this many px above the canvas's bottom edge,
    /// regardless of canvas size.
    const GALLERY_GRID_BOTTOM_MARGIN_PX: f32 = 40.0;
    const GALLERY_CORNER_RADIUS: f32 = 20.0;
    const GALLERY_CORNER_RADIUS_DARK: f32 = 20.0;
    /// A touch slower than `BUTTON_TILES_MORPH_DURATION` — a bigger, more
    /// deliberate fan-out (1↔12 vs 1↔3).
    const GALLERY_GRID_MORPH_DURATION: f32 = 0.6;
    /// How long to wait for all 12 images to finish fetching/baking before
    /// giving up and showing `LOADING_LOGO_ERROR_TEXT` instead.
    const GALLERY_FETCH_TIMEOUT: f32 = 10.0;
    const GALLERY_FETCH_BUTTON_LABEL: &str = "Fetch New Images";
    /// Top-anchored, same as the grid is bottom-anchored: this many px
    /// between the canvas's top edge and the button's top edge.
    const GALLERY_FETCH_BUTTON_TOP_MARGIN_PX: f32 = 65.0;
    /// Used only until `gallery_fetch_button_label`'s `BakedText` is ready —
    /// same fallback pattern as `NAV_BUTTON_FALLBACK_SIZE`.
    const GALLERY_FETCH_BUTTON_FALLBACK_SIZE: Vec2 = Vec2::new(220.0, 46.0);
    const LOADING_LOGO_ERROR_TEXT: &str = "Couldn't load images";

    // -------------------------------------------------------------------------
    // Enlarged gallery image — click a tile to morph it into a centered,
    // aspect-fit view; see `start_gallery_to_image`.
    // -------------------------------------------------------------------------

    /// Cap for the enlarged image's hires fetch — distinct from
    /// `MAX_TILE_IMAGE_SIDE` (the 12 simultaneous grid tiles' shared cap):
    /// only one hires image is ever resident at a time, so it can afford to
    /// be bigger without threatening the atlas budget the M11.2 regression
    /// test proves for the grid's own working set. See
    /// `bake_gallery_hires_image`.
    const GALLERY_LARGE_IMAGE_MAX_SIDE: u32 = 900;
    /// Crossfade duration for swapping the enlarged image from its low-res
    /// (grid-tile) stand-in to the fetched hires version, once it arrives.
    const GALLERY_HIRES_CROSSFADE_DURATION: f32 = 0.25;

    // -------------------------------------------------------------------------
    // Nav buttons — splash morphs into these; clicking "Video Demo" morphs into tiles
    // -------------------------------------------------------------------------

    const NAV_BUTTON_TITLES: [&str; 3] = ["Video Demo", "Photo Gallery", "Examples & Tests"];
    const NAV_BUTTON_LABEL_SIZE_PX: f32 = 24.0;
    const NAV_BUTTON_LETTER_SPACING_PX: f32 = NAV_BUTTON_LABEL_SIZE_PX * 0.02;
    const NAV_BUTTON_PADDING_PX: f32 = 15.0;
    const NAV_BUTTON_GAP_PX: f32 = 50.0;
    const NAV_BUTTON_CORNER_RADIUS: f32 = 20.0;
    /// Design System — Color-dark treatment corner radius. See `advance_theme`.
    const NAV_BUTTON_CORNER_RADIUS_DARK: f32 = 20.0;
    const NAV_BUTTON_FALLBACK_SIZE: Vec2 = Vec2::new(150.0, 46.0);

    // -------------------------------------------------------------------------
    // Examples & Tests — Home's 3rd button splits into a 3×2 grid of these
    // -------------------------------------------------------------------------

    /// Column-major (`i = col*2 + row`) — see `example_buttons`'s doc for
    /// the spatial mapping to `nav_buttons`.
    const EXAMPLE_CATEGORY_TITLES: [&str; 6] = [
        "Effects",
        "Text",
        "Transforms & Animation",
        "Stress Tests",
        "Layout",
        "3D",
    ];
    const EXAMPLE_BUTTON_COL_GAP_PX: f32 = 40.0;
    const EXAMPLE_BUTTON_ROW_GAP_PX: f32 = 30.0;
    /// Shared padding: the detail panel's own top/left inset (`example_panel_quad`
    /// sizes the panel from content height using this same value, so the gap
    /// from the panel's top edge to its first row matches the gap from its
    /// left edge to a row's label) and the gap between a category's heading
    /// and the panel below it.
    const EXAMPLE_PANEL_PADDING_PX: f32 = 24.0;
    const EXAMPLE_HEADING_PANEL_GAP_PX: f32 = 20.0;
    /// Local Z for content `ChildOf` `example_detail_panel` — composes
    /// additively over the panel's own Z (0.5), landing at 0.51, so
    /// content renders above the panel's opaque fill instead of behind it.
    /// Shared by every category's content (Effects, Text, ...) that
    /// follows this pattern.
    const EXAMPLE_CONTENT_LOCAL_Z: f32 = 0.01;
    /// Effects screen (`ExampleDetail(0)`) content's own vertical extent —
    /// see `layout_effects_content`'s row-spacing constants, which this
    /// must stay consistent with: 3 row-to-row gaps + one row's own box
    /// height + the opacity row's caption gap/height.
    const EFFECTS_CONTENT_HEIGHT_PX: f32 = 3.0 * 90.0 + 64.0 + 18.0 + 18.0;
    /// Text screen (`ExampleDetail(1)`) content's own vertical extent — see
    /// `layout_text_content`'s row-spacing constants, which this must stay
    /// consistent with: 2 row-to-row gaps + one row's own item height.
    const TEXT_CONTENT_HEIGHT_PX: f32 = 2.0 * 90.0 + 56.0;
    /// Transforms & Animation screen (`ExampleDetail(2)`) content's own
    /// vertical extent — see `layout_transforms_content`'s row-spacing
    /// constants, which this must stay consistent with: 2 row-to-row gaps
    /// (120 each — wider than every other screen's, this content
    /// rotates/scales) + one row's own box height + extra top/bottom
    /// clearance (20px each) so rotated/scaled boxes don't clip the panel
    /// edge. Unlike Effects' Opacity row, the row with captions here
    /// (Rotation/Scale) isn't the *last* row, so no extra caption
    /// allowance is needed: the last row (Continuous Animation) has no
    /// caption of its own.
    const TRANSFORMS_CONTENT_HEIGHT_PX: f32 = 2.0 * 120.0 + 64.0 + 2.0 * 20.0;
    /// Both Stress Tests buttons' fixed height — same as
    /// `gallery_fetch_button`'s. Width is *not* fixed (unlike
    /// `gallery_fetch_button`): the two labels ("Run Burst Spawn"/"Run
    /// Texture Churn") are different lengths, and a single shared width
    /// gave the longer one uneven, cramped-looking horizontal padding —
    /// `layout_stress_content` instead sizes each button to its own
    /// baked label width plus `STRESS_BUTTON_HORIZONTAL_PADDING_PX`.
    const STRESS_BUTTON_HEIGHT_PX: f32 = 46.0;
    /// Horizontal padding (both sides combined) each Stress Tests button
    /// adds around its own label — see `STRESS_BUTTON_HEIGHT_PX`'s doc.
    const STRESS_BUTTON_HORIZONTAL_PADDING_PX: f32 = 40.0;
    /// Height of the strip reserved at the bottom of the Stress Tests
    /// panel for `stress_result_text` — `layout_stress_content` centers
    /// the result line within it, and `random_burst_target` excludes it
    /// from Burst Spawn's usable area so quads never render over the
    /// result text.
    const RESULT_TEXT_RESERVED_HEIGHT_PX: f32 = 40.0;
    /// Wall-clock duration each stress test runs before finalizing and
    /// reporting its average FPS.
    const STRESS_TEST_DURATION: f32 = 3.0;
    /// A result at or above this reads as "bumped into the vsync cap"
    /// rather than "this is the actual load ceiling" — see
    /// `finalize_stress_test`'s vsync note. Set a little under 60 to
    /// absorb ordinary frame-timing jitter in the measured average.
    const VSYNC_FPS_CAP_THRESHOLD: f32 = 55.0;
    /// Number of small quad entities the Burst Spawn test keeps under
    /// sustained, continuously-retriggered transition load for the whole
    /// `STRESS_TEST_DURATION` window.
    const BURST_SPAWN_COUNT: usize = 1000;
    /// How long each Burst Spawn entity's own transition to a fresh
    /// random target takes — short enough that entities retrigger several
    /// times over `STRESS_TEST_DURATION`, keeping the load sustained
    /// rather than a single initial burst.
    const BURST_SPAWN_ITEM_DURATION: f32 = 0.4;
    /// Burst Spawn entities' fixed base size (before their own random
    /// scale is applied).
    const BURST_ITEM_SIZE: f32 = 16.0;
    /// Number of fixed scratch entities the Texture Churn test repeatedly
    /// re-textures for the whole `STRESS_TEST_DURATION` window — small
    /// and fixed (unlike Burst Spawn's entity count) since the load this
    /// test isolates is the register/upload/evict path itself, not
    /// entity count.
    const TEXTURE_CHURN_SLOTS: usize = 8;
    /// Each Texture Churn slot's fixed on-screen quad size — independent
    /// of the synthetic texture's own (varying) pixel size, since
    /// `BakedImage` UVs map onto whatever quad size is set regardless of
    /// the source image's dimensions.
    const TEXTURE_CHURN_SLOT_SIZE: f32 = 80.0;
    /// Grid layout for `TEXTURE_CHURN_SLOTS`.
    const TEXTURE_CHURN_COLS: usize = 4;
    /// Synthetic texture side length range (min, spread) fed to
    /// `TextureRegistry::register_static` each churn — "~100-400px" per
    /// the plan, varying the atlas allocation size each cycle.
    const TEXTURE_CHURN_SIZE_MIN: f32 = 100.0;
    const TEXTURE_CHURN_SIZE_SPREAD: f32 = 300.0;

    // -------------------------------------------------------------------------
    // Home/back nav icons — top-left, visible whenever tiles/screen are showing
    // -------------------------------------------------------------------------

    const NAV_ICON_SIZE_PX: f32 = 56.0;
    const NAV_ICON_CORNER_RADIUS: f32 = NAV_ICON_SIZE_PX / 2.0;
    const NAV_ICON_MARGIN_PX: f32 = 20.0;
    const NAV_ICON_GAP_PX: f32 = 10.0;
    const NAV_ICON_FADE_DURATION: f32 = 0.3;
    /// Gap between the icon row and the video screen — the screen must
    /// resize to respect this, see `video_screen_quad`.
    const SCREEN_NAV_CLEARANCE_PX: f32 = 20.0;
    /// Vertical space the icon row + its clearance reserve at the top of the
    /// canvas — used to cap the video screen's height so it never overlaps
    /// them.
    const ICON_ROW_RESERVED_PX: f32 =
        NAV_ICON_MARGIN_PX + NAV_ICON_SIZE_PX + SCREEN_NAV_CLEARANCE_PX;

    // -------------------------------------------------------------------------
    // Persistent brand lockup — mark + "PROTEUS" wordmark, top-left
    // -------------------------------------------------------------------------

    /// `lockup.png`'s native size (420×92) — its aspect ratio derives the
    /// on-screen width from `LOGO_HEIGHT_PX`, since `Image` doesn't carry
    /// intrinsic size into `QuadState`.
    const LOGO_NATIVE_WIDTH_PX: f32 = 420.0;
    const LOGO_NATIVE_HEIGHT_PX: f32 = 92.0;
    /// The rightmost column (measured) of the "S" in "PROTEUS" within
    /// `lockup.png` — well short of `LOGO_NATIVE_WIDTH_PX` since the image
    /// carries trailing whitespace. The icon row's gap is measured from
    /// here, not from the image's own bounding box, so it reads as 30px from
    /// the visible text rather than 30px plus however much blank margin the
    /// PNG happens to have baked in.
    const LOGO_NATIVE_TEXT_RIGHT_PX: f32 = 299.0;
    /// Matches the nav icons' height so the two rows align.
    const LOGO_HEIGHT_PX: f32 = NAV_ICON_SIZE_PX;
    const LOGO_WIDTH_PX: f32 = LOGO_HEIGHT_PX * (LOGO_NATIVE_WIDTH_PX / LOGO_NATIVE_HEIGHT_PX);
    const LOGO_TEXT_RIGHT_PX: f32 =
        LOGO_HEIGHT_PX * (LOGO_NATIVE_TEXT_RIGHT_PX / LOGO_NATIVE_HEIGHT_PX);
    /// Gap between the visible edge of the "S" in "PROTEUS" and the home
    /// icon's left edge.
    const LOGO_ICONS_GAP_PX: f32 = 45.0;

    // -------------------------------------------------------------------------
    // Light/dark theme toggle — sun/moon icons, top-right
    // -------------------------------------------------------------------------

    /// Sun/moon reuse the home/back icons' exact size/margin/gap — same row,
    /// mirrored to the right edge (see `advance_theme`'s positioning step).
    const THEME_ICON_SIZE_PX: f32 = NAV_ICON_SIZE_PX;
    const THEME_ICON_MARGIN_PX: f32 = NAV_ICON_MARGIN_PX;
    const THEME_ICON_GAP_PX: f32 = NAV_ICON_GAP_PX;
    /// A touch slower than `BUTTON_TILES_MORPH_DURATION` — this is a bigger,
    /// more deliberate showcase moment (the whole app re-themes at once), not
    /// just one shape morphing into another.
    const THEME_MORPH_DURATION: f32 = 0.6;

    // -------------------------------------------------------------------------
    // Demo scene geometry  (identical to proteus-shell-native)
    // -------------------------------------------------------------------------

    /// The animated logo mark, standing in for the old "START" button —
    /// alpha 0 (fades in via `advance_intro_and_hover`), rendered at
    /// `COMPOSITE_SCALE`. `position.x` starts at 0 but is corrected every
    /// frame once the wordmark's baked width is known, to center the
    /// mark+wordmark composite as a whole (see `advance_intro_and_hover`) —
    /// not just the mark. No added fill/border/corner-radius: the Brand Spec
    /// says not to add outlines around the mark, and the mark's own 13:18
    /// rectangle already fills its whole quad (two triangles meeting at the
    /// diagonal — see the Brand Spec's "Geometry" section), so an untinted,
    /// unrounded quad renders it as-is.
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
    /// transition bakes its target's appearance once, at setup time, and
    /// that bake is never revisited, so a hardcoded light-theme radius here
    /// would freeze a stale corner radius into the bake; the real tile
    /// (corrected every frame by `advance_theme`) then shows the correct
    /// dark radius the instant the transition completes, producing a
    /// visible "snap." Passing the live value keeps the bake theme-correct
    /// from the start.
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
    /// canvas and the fixed margins/gaps — the smaller of the two
    /// axis-derived sizes, so neither axis's margin/gap spec is ever
    /// violated by the tighter axis (same "derive from whichever axis is
    /// more constraining" approach as `video_screen_quad`).
    fn gallery_cell_size(canvas_width: f32, canvas_height: f32) -> f32 {
        let usable_w = (canvas_width
            - GALLERY_MARGIN_LEFT
            - GALLERY_MARGIN_RIGHT
            - (GALLERY_COLS - 1) as f32 * GALLERY_GAP_PX)
            .max(0.0);
        let usable_h = (canvas_height
            - GALLERY_MARGIN_TOP
            - GALLERY_MARGIN_BOTTOM
            - (GALLERY_ROWS - 1) as f32 * GALLERY_GAP_PX)
            .max(0.0);
        (usable_w / GALLERY_COLS as f32).min(usable_h / GALLERY_ROWS as f32) * GALLERY_CELL_SCALE
    }

    /// One of the 12 photo gallery tiles. `idx` 0..12, row-major (row =
    /// idx/4, col = idx%4; row 0 = top). Horizontally centered on the canvas
    /// (same centering convention as `tile_quad`/`layout_nav_buttons`), but
    /// vertically bottom-anchored — the grid's bottom edge always sits
    /// `GALLERY_GRID_BOTTOM_MARGIN_PX` above the canvas's bottom edge,
    /// growing upward from there, so it holds a fixed distance from the
    /// bottom regardless of canvas size or cell size.
    fn gallery_cell_quad(
        idx: usize,
        canvas_width: f32,
        canvas_height: f32,
        theme_progress: f32,
    ) -> QuadState {
        let cell = gallery_cell_size(canvas_width, canvas_height);
        let (row, col) = (idx / GALLERY_COLS, idx % GALLERY_COLS);
        let (grid_w, _) = gallery_grid_content_size(canvas_width, canvas_height);
        let x = -grid_w / 2.0 + cell / 2.0 + col as f32 * (cell + GALLERY_GAP_PX);
        let rows_from_bottom = (GALLERY_ROWS - 1 - row) as f32;
        let grid_bottom_y = -canvas_height / 2.0 + GALLERY_GRID_BOTTOM_MARGIN_PX;
        let y = grid_bottom_y + cell / 2.0 + rows_from_bottom * (cell + GALLERY_GAP_PX);
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

    /// Full width/height of the gallery grid's content block (all 12 cells
    /// + gaps, no margins) — the bounding box `gallery_large_image_quad`
    /// fits an enlarged image within.
    fn gallery_grid_content_size(canvas_width: f32, canvas_height: f32) -> (f32, f32) {
        let cell = gallery_cell_size(canvas_width, canvas_height);
        let w = GALLERY_COLS as f32 * cell + (GALLERY_COLS - 1) as f32 * GALLERY_GAP_PX;
        let h = GALLERY_ROWS as f32 * cell + (GALLERY_ROWS - 1) as f32 * GALLERY_GAP_PX;
        (w, h)
    }

    /// The enlarged gallery image's target geometry — dead center of the
    /// canvas, `aspect` (width, height ratio) fit within the grid's own
    /// content bounding box without distorting it. A portrait image (taller
    /// than the box's own aspect ratio) ends up height-constrained; a
    /// square one is height-constrained the same way; a landscape image
    /// (wider than the box's aspect ratio) ends up width-constrained —
    /// all three are just the one `min()` "contain" fit below, not special
    /// cases.
    fn gallery_large_image_quad(
        aspect: (f32, f32),
        canvas_width: f32,
        canvas_height: f32,
        theme_progress: f32,
    ) -> QuadState {
        let (box_w, box_h) = gallery_grid_content_size(canvas_width, canvas_height);
        let (aspect_w, aspect_h) = aspect;
        let scale = (box_w / aspect_w).min(box_h / aspect_h);
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(aspect_w * scale, aspect_h * scale),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: GALLERY_CORNER_RADIUS
                + (GALLERY_CORNER_RADIUS_DARK - GALLERY_CORNER_RADIUS) * theme_progress,
        }
    }

    /// Returns `baked` with its UV cropped to a centered square, based on
    /// `baked.pixel_size`'s aspect ratio — portrait crops the height,
    /// landscape crops the width, square is returned unchanged. Used when
    /// baking a gallery tile's own (usually non-square, since real photos
    /// rarely are — see `NATURE_PHOTOS` in index.html) fetched image, so it
    /// fills the grid's square cell instead of stretching.
    fn center_crop_to_square(baked: BakedImage) -> BakedImage {
        let (pw, ph) = (baked.pixel_size[0], baked.pixel_size[1]);
        let mut uv_offset = baked.uv_offset;
        let mut uv_scale = baked.uv_scale;
        if pw > ph {
            let frac = ph / pw;
            uv_offset[0] += uv_scale[0] * (1.0 - frac) / 2.0;
            uv_scale[0] *= frac;
        } else if ph > pw {
            let frac = pw / ph;
            uv_offset[1] += uv_scale[1] * (1.0 - frac) / 2.0;
            uv_scale[1] *= frac;
        }
        BakedImage {
            uv_offset,
            uv_scale,
            page: baked.page,
            pixel_size: baked.pixel_size,
        }
    }

    /// Color-light inner border for each tile. Full alpha immediately —
    /// tiles appear via the button-spread morph, not a separate fade.
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

    // -------------------------------------------------------------------------
    // Video screen — MP4 playback surface (M9.5)
    // -------------------------------------------------------------------------
    //
    // Sized proportionally to 720p (16:9) rather than rendered at that
    // resolution — the actual decode resolution comes from the browser's own
    // `<video>` element (see `ProteusApp::start_video`, called from JS once
    // `loadedmetadata` fires) and is whatever `QuadPipeline::init_video` was
    // called with.
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
    /// Deliberately smaller than `TILE_CORNER_RADIUS` — reusing the tile's
    /// value read as "nicely rounded" on a ~200px-wide tile but nearly
    /// square at the screen's ~900px width, the same absolute pixel radius
    /// being a much smaller fraction of the much larger shape. See
    /// `advance_theme`'s corner radius step for how this now actually eases
    /// into place (rather than popping) as a tile grows into the screen.
    const SCREEN_CORNER_RADIUS: f32 = 12.0;
    /// Design System — Color-dark treatment corner radius, same 1.5× ratio
    /// as `TILE_CORNER_RADIUS_DARK`/`TILE_CORNER_RADIUS`.
    const SCREEN_CORNER_RADIUS_DARK: f32 = 18.0;

    /// The video screen shape, sized to `SCREEN_WIDTH_FRACTION` of the
    /// current canvas width at a 720p aspect ratio. Recomputed (not cached)
    /// each time a tiles→screen transition starts, so a resize between visits
    /// isn't stale.
    ///
    /// `color` is white (untinted) rather than black: once `VideoPlayer` is
    /// attached, `QuadState.color` multiplies the sampled video texture (see
    /// `proteus_ui::video`), so a black target would render real video frames
    /// as solid black. Before the first pushed frame arrives the video
    /// texture is zero-initialized (transparent), so the screen is briefly
    /// see-through rather than a black card — an acceptable startup blip.
    fn video_screen_quad(canvas_width: f32, canvas_height: f32) -> QuadState {
        let uncapped_height = canvas_width * SCREEN_WIDTH_FRACTION * SCREEN_ASPECT;
        // Never let the screen (vertically centered) overlap the top-left
        // icon row + its clearance — cap height, then re-derive width to
        // keep the 720p aspect ratio rather than stretching.
        let max_height = (canvas_height - 2.0 * ICON_ROW_RESERVED_PX).max(0.0);
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

    /// The "Examples & Tests" detail panel's target geometry — a big,
    /// centered card any of the 6 category buttons morphs into when
    /// clicked, mirroring `video_screen_quad`'s "big centered shape" idea
    /// but a plain content card (subtle translucent fill, not untinted like
    /// the video screen) sized to comfortably hold rows of example content
    /// instead of a fixed 16:9 frame.
    /// `content_height`: the category's own content's vertical extent —
    /// `EFFECTS_CONTENT_HEIGHT_PX` for Effects, a placeholder-sized value
    /// for categories not yet built. Panel height is `content_height +
    /// 2*EXAMPLE_PANEL_PADDING_PX` (capped to the window), so the padding
    /// above/below the content matches the padding used to the left of it
    /// — the panel *shrinks to fit* rather than always filling a fixed
    /// window fraction. `position.y` is always `0.0` here — a category
    /// with its own heading rendered above the panel (see
    /// `layout_effects_content`) shifts it down afterward so heading+panel
    /// together land centered on screen.
    fn example_panel_quad(
        canvas_width: f32,
        canvas_height: f32,
        theme_progress: f32,
        content_height: f32,
    ) -> QuadState {
        let width = (canvas_width * 0.85).min(1000.0);
        let max_height = (canvas_height - 2.0 * ICON_ROW_RESERVED_PX).max(0.0);
        let height = (content_height + 2.0 * EXAMPLE_PANEL_PADDING_PX).min(max_height);
        // Neutral grey (not violet-tinted like everything else in this
        // demo) — deliberately opaque and desaturated, so the
        // effect/text/transform examples sitting on top of it (many close
        // to white) read clearly against it instead of blending into the
        // app's own light background. Unlike every other themed color in
        // this file, this one does *not* lerp to a darker variant in dark
        // mode — a darkened panel read as near-black, at odds with the
        // rest of the demo's dark-mode surfaces; this backdrop stays the
        // same light grey in both themes.
        let light_grey = Vec4::new(0.82, 0.82, 0.83, 1.0);
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(width, height),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: light_grey,
            corner_radius: SCREEN_CORNER_RADIUS
                + (SCREEN_CORNER_RADIUS_DARK - SCREEN_CORNER_RADIUS) * theme_progress,
        }
    }

    // -------------------------------------------------------------------------
    // App state machine
    // -------------------------------------------------------------------------

    /// Every resting state the demo can land in. `Splash` is the only one
    /// nothing ever transitions *to* — it's the initial state and its own
    /// timer carries it forward, see `advance_demo`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum AppState {
        /// Mark + wordmark visible, not yet interactive.
        Splash,
        /// Three nav buttons visible — click "Video Demo" (index 0) to
        /// converge into the video tiles; the other two do nothing yet.
        Home,
        /// Three tiles visible — click any tile to converge into the video
        /// screen, or home to converge back into the nav buttons.
        VideoTiles,
        /// Video screen visible (as `tiles[screen_idx]`) — no longer
        /// interactive itself; the home/back icons drive navigation from here.
        VideoScreen(usize),
        /// Fetching 12 nature-themed loremflickr.com images in the background — the animated
        /// logo (light/dark theme-crossfaded) loops on `loading_logo` while
        /// waiting. Auto-advances to `Gallery` once every gallery tile has a
        /// `BakedImage`, or shows an inline error after
        /// `GALLERY_FETCH_TIMEOUT` if they don't all arrive in time — see
        /// `advance_gallery_fetch`.
        Loading,
        /// 4×3 grid of fetched images visible — click home to converge back
        /// to the nav buttons (three column-grouped `NToOneRequest`s at
        /// once), click any tile to enlarge it (`GalleryImage`).
        Gallery,
        /// `gallery_tiles[image_idx]` enlarged to a centered, aspect-correct
        /// size — the other 11 tiles and the fetch button are faded out.
        /// Click the image to return to `Gallery`, or home to converge
        /// straight to the nav buttons (skipping back through the grid).
        GalleryImage(usize),
        /// 6 category buttons visible, 3 cols × 2 rows — click one to
        /// converge into its own detail panel (`ExampleDetail`), or home to
        /// converge back into the 3 nav buttons.
        ExamplesHome,
        /// `example_buttons[category_idx]` enlarged into a centered detail
        /// panel — no longer interactive itself; the home/back icons drive
        /// navigation from here, mirroring `VideoScreen`.
        ExampleDetail(usize),
    }

    /// An in-flight move between two resting `AppState`s, keyed by the
    /// `(from, to)` pair. `begin_transition` creates one and kicks off
    /// whatever entity setup that pair needs; `drive_transition` ticks it
    /// once per frame and reports completion; `settle` then unconditionally
    /// forces every affected entity into `to`'s correct resting
    /// configuration — regardless of which `from` it arrived from — before
    /// `self.state` actually becomes `to`. This is the piece that keeps a
    /// demo replayed many times from ever leaving stale geometry/visibility
    /// behind: no match arm needs to remember to clean up after itself on
    /// the way out, `settle` always cleans up on the way in.
    struct Transition {
        from: AppState,
        to: AppState,
        elapsed: f32,
    }

    /// Which Stress Tests screen (`ExampleDetail(3)`) button started the
    /// current run — `StressTestRun::kind` dispatches
    /// `advance_stress_test`'s per-frame work and `finalize_stress_test`'s
    /// result-message formatting.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum StressTestKind {
        BurstSpawn,
        TextureChurn,
    }

    /// An in-flight Stress Tests run — mirrors `Transition`'s "one struct
    /// tracks the in-flight thing" shape. `advance_stress_test` ticks
    /// `elapsed`/`frame_count` every frame until `elapsed >=
    /// STRESS_TEST_DURATION`, then hands off to `finalize_stress_test`,
    /// which computes `frame_count as f32 / elapsed` for the result
    /// message, despawns `entities`, and clears `self.stress_test_run`
    /// back to `None` — at which point a fresh click can start another
    /// run.
    struct StressTestRun {
        kind: StressTestKind,
        elapsed: f32,
        frame_count: u32,
        /// The test's own scratch entities — Burst Spawn's quads, or
        /// Texture Churn's `TEXTURE_CHURN_SLOTS` fixed slots.
        entities: Vec<Entity>,
        /// Texture Churn's register/evict cycle count — stays 0 for
        /// Burst Spawn, which reports an entity count instead (see
        /// `finalize_stress_test`).
        churn_iterations: u32,
    }

    // -------------------------------------------------------------------------
    // Staged pointer — accumulates JS events between frames
    // -------------------------------------------------------------------------

    /// Pointer state accumulated from JS events. Flushed to the ECS
    /// `PointerInput` resource at the start of each `tick()` call.
    #[derive(Default)]
    struct StagedPointer {
        position: Option<Vec2>,
        just_pressed: bool,
        just_released: bool,
        is_pressed: bool,
    }

    /// A pending hires fetch for the enlarged gallery image — returned by
    /// `take_gallery_hires_fetch_request()`. `width`/`height` are the actual
    /// on-screen fitted pixel dimensions (see `gallery_large_image_quad`),
    /// already capped to `GALLERY_LARGE_IMAGE_MAX_SIDE`; `photo_id` requests
    /// the same picsum.photos photo the tile's low-res image already shows,
    /// just bigger.
    #[wasm_bindgen]
    #[derive(Clone, Copy)]
    pub struct GalleryHiresRequest {
        pub tile_idx: u32,
        pub width: u32,
        pub height: u32,
        pub photo_id: u32,
    }

    // -------------------------------------------------------------------------
    // ProteusApp
    // -------------------------------------------------------------------------

    /// Proteus web application.  Create via `ProteusApp.init(canvasId)`.
    #[wasm_bindgen]
    pub struct ProteusApp {
        surface: wgpu::Surface<'static>,
        surface_config: wgpu::SurfaceConfiguration,
        device: wgpu::Device,
        queue: wgpu::Queue,
        // `QuadPipeline`/`GpuContext`/`FontAtlas` live inside `ui_world.world`
        // as ECS resources, not as fields here — see proteus-shell-native's
        // RenderState doc comment for why (lets transition-setup systems bake
        // Slice transitions automatically, and (M10.5) lets bake::bake_system
        // reach FontAtlas to bake `Baked` composites).
        ui_world: ProteusWorld,

        /// Full-window backdrop image, visible in every state (see
        /// `advance_background`).
        background: Entity,
        /// Color-dark counterpart, a child of `background` — cross-faded
        /// over it via `theme_progress` (see `advance_theme`).
        background_dark: Entity,

        button: Entity,
        /// The "PROTEUS" wordmark — a `Text` child of `button` (M10
        /// composition), positioned to the right of the mark once baked.
        /// See its spawn site.
        wordmark: Entity,
        /// The three nav buttons ("Video Demo", "Image Gallery Demo", "Video
        /// Tests") the splash morphs into.
        nav_buttons: [Entity; 3],
        /// Per-button label — a `Text` child of `nav_buttons[i]`.
        nav_labels: [Entity; 3],
        /// The 6 "Examples & Tests" category buttons, arranged 3 cols × 2
        /// rows, column-major indexed (`i = col*2 + row`) to match
        /// `nav_buttons[i]`'s own column — see `layout_example_buttons` and
        /// `start_home_to_examples`'s doc for the exact spatial mapping.
        /// Home's 3 nav buttons split into these and merge back from them,
        /// 3 simultaneous per-column group requests each way, mirroring
        /// `start_gallery_to_home`'s M→N composition. Always grid-button
        /// shaped — unlike an earlier version of this feature, these never
        /// double as the detail panel themselves (see
        /// `example_detail_panel`).
        example_buttons: [Entity; 6],
        /// Per-button label — a `Text` child of `example_buttons[i]`.
        example_labels: [Entity; 6],
        /// Dedicated coordinator/display entity for whichever category is
        /// enlarged — mirrors `gallery_enlarged_base` exactly, including
        /// why it has to be a separate entity rather than reusing one of
        /// the 6 `example_buttons`: a category's return-to-grid split has
        /// that same button as *both* one of its own 6 targets *and* (if
        /// it doubled as the panel) the coordinator baking the whole
        /// detail screen — two roles needing different content from one
        /// shared component at once. `start_examples_to_detail`/
        /// `start_detail_to_examples` merge all 6 `example_buttons` into
        /// this and split it back, `MergeLayout::Grid`/
        /// `SplitStrategy::GridSlice{cols:3,rows:2}` matching the real
        /// grid.
        example_detail_panel: Entity,
        /// Each category's own detail-screen content. `ChildOf`
        /// `example_detail_panel` (not any of the 6 `example_buttons`) so
        /// it bakes as part of the panel's own shared source/destination
        /// bake during the 6↔1 group transition — this is what makes the
        /// content visually read as *part of* the sliced/morphing element
        /// instead of popping in after. This is safe from the flash bug an
        /// earlier version hit (content briefly baking onto the wrong grid
        /// button): that bug came from content being `ChildOf` one of the
        /// 6 *targets themselves* (double duty as both a target and the
        /// thing carrying the content); `example_detail_panel` is never
        /// one of the 6 targets, so its bake only ever runs for an actual
        /// `ExamplesHome`↔`ExampleDetail` transition. NOTE for future
        /// categories (Step 3+): today only category 0 (Effects) has
        /// content, so nothing else is a sibling `ChildOf` this same
        /// panel; once more categories get content, a transition into/out
        /// of *any* category would bake *all* categories' content
        /// together (since baking ignores `Visibility`) unless this gets
        /// revisited (e.g. attach/detach `ChildOf` per active category).
        /// Local position is relative to the panel's center — see
        /// `layout_*_content` for the per-category math — with a small
        /// positive local Z (composes additively over the panel's own Z)
        /// so content renders above the panel's opaque fill rather than
        /// behind it. Shown only while `state == ExampleDetail(i)`.
        /// Populated incrementally as each category's content lands;
        /// empty for a category not yet built.
        example_content: [Vec<Entity>; 6],
        /// Effects screen (`ExampleDetail(0)`) layout handles — kept
        /// separate from `example_content[0]` (which only needs a flat
        /// list for visibility toggling) because `layout_effects_content`
        /// has to reposition each of these individually, left-aligned to
        /// the panel's current (window-dependent) size.
        /// Unlike the rest of `example_content[0]`, deliberately **not**
        /// `ChildOf` `example_detail_panel` — it renders *above/outside*
        /// the panel's own bounds and fades in on its own after settle
        /// (`advance_example_title_fade`), independent of the panel's
        /// bake.
        effects_title: Entity,
        /// "Drop Shadow" / "Glow" / "Border" / "Opacity", row order.
        effects_row_labels: [Entity; 4],
        /// Each row's own boxes, row order matching `effects_row_labels`.
        effects_row_boxes: [Vec<Entity>; 4],
        /// The 5 small captions under the Opacity row's items (4 swatches
        /// + the nested cascade pair).
        effects_opacity_item_labels: [Entity; 5],
        /// Text screen (`ExampleDetail(1)`) layout handles — same
        /// shape/role as `effects_title` (independent, fades in above the
        /// panel).
        text_title: Entity,
        /// "Font Size" / "Color" / "Letter Spacing", row order.
        text_row_labels: [Entity; 3],
        /// Each row's own sample-text items, row order matching
        /// `text_row_labels`. Unlike `effects_row_boxes`'s uniform 64px
        /// boxes, these vary in width (different font sizes/letter
        /// spacing all widen the baked glyph run) — `layout_text_content`
        /// flows each one past the previous using its own baked width
        /// rather than a fixed grid column stride.
        text_row_items: [Vec<Entity>; 3],
        /// Transforms & Animation screen (`ExampleDetail(2)`) layout
        /// handles — same shape/role as `effects_title` (independent,
        /// fades in above the panel).
        transforms_title: Entity,
        /// "Rotation" / "Scale" / "Continuous Animation", row order.
        transforms_row_labels: [Entity; 3],
        /// Each row's own boxes, row order matching
        /// `transforms_row_labels` — 3 static boxes for Rotation, 3 for
        /// Scale, 1 for Continuous Animation
        /// (`transforms_continuous_box`, tracked separately below too
        /// since `advance_example_animation` needs direct access to it
        /// every frame).
        transforms_row_boxes: [Vec<Entity>; 3],
        /// Captions underneath the Rotation/Scale rows' boxes (row order
        /// matching `transforms_row_boxes`) — e.g. "0°"/"20°"/"-35°",
        /// "0.6×"/"1.0×"/"1.4×". Empty for the Continuous Animation row:
        /// one box, already described by its own row label.
        transforms_item_labels: [Vec<Entity>; 3],
        /// The Continuous Animation row's one box — same entity as
        /// `transforms_row_boxes[2][0]`, just named directly since
        /// `advance_example_animation` writes to it every frame this
        /// screen is active.
        transforms_continuous_box: Entity,
        /// Elapsed time (seconds) the Continuous Animation box has spent
        /// animating — only advances while `state == ExampleDetail(2)`
        /// and idle (see `advance_example_animation`), paused otherwise.
        /// Drives its rotation/scale/hue directly (not a 0..1 progress
        /// value like every other `_progress`/`_fade` field in this file
        /// — the animation has no end to progress toward, it just runs).
        transforms_anim_elapsed: f32,
        /// Entities `set_example_category_active` must never
        /// attach/detach `ChildOf` `example_detail_panel` for, despite
        /// appearing in the relevant `example_content[idx]` (for
        /// visibility toggling) like every other row entity does. Two
        /// distinct reasons an entity ends up here:
        /// - It's `ChildOf` something *other* than the panel already —
        ///   e.g. the Opacity row's nested cascade child (`ChildOf` its
        ///   own sibling box) or a Stress Tests button's own label
        ///   (`ChildOf` its button) — and inherits panel-relativity
        ///   transitively through that instead.
        /// - It's independent/absolute-positioned, same idea as a
        ///   category's own heading (which has its own separate
        ///   exemption via `example_heading_entity`) — e.g. the Stress
        ///   Tests buttons themselves, which sit *outside* the panel's
        ///   bounds (see `stress_buttons`'s doc).
        ///
        /// Reparenting either kind directly onto the panel would snap
        /// its position onto the panel's own coordinate frame — a real
        /// regression this list exists to prevent.
        example_reparent_exempt: Vec<Entity>,
        /// Stress Tests screen (`ExampleDetail(3)`) layout handles — same
        /// shape/role as `effects_title` (independent, fades in above
        /// the panel).
        stress_title: Entity,
        /// The two action buttons — independent/absolute (not `ChildOf`
        /// `example_detail_panel`; see `example_reparent_exempt`'s doc),
        /// positioned between the heading and the panel. Index 0 = "Run
        /// Burst Spawn", index 1 = "Run Texture Churn".
        stress_buttons: [Entity; 2],
        /// Per-button label — a `Text` child of `stress_buttons[i]`, same
        /// idiom as `gallery_fetch_button_label`.
        stress_button_labels: [Entity; 2],
        /// Hover glow/scale progress (0..1) per button — same pattern as
        /// `gallery_fetch_button_hover_progress`.
        stress_button_hover_progress: [f32; 2],
        stress_button_is_hovering: [bool; 2],
        /// Fade-in (once settled and idle on this screen) / fade-out (in
        /// sync with the transition out) progress (0..1) shared by both
        /// buttons — same rationale as `example_title_fade`: since the
        /// buttons are independent/absolute (not `ChildOf` the panel,
        /// so never baked into any transition — see `stress_buttons`'s
        /// doc), an outbound transition would otherwise leave them
        /// sitting there at full opacity for the whole morph and
        /// popping off abruptly only once settle hides them.
        stress_button_fade: f32,
        /// Result line below the buttons — `ChildOf` the panel, empty (a
        /// single space, so it still bakes once and never needs a
        /// "pending forever" empty-string retry) until a test finishes
        /// and `set_stress_result_text` gives it real content.
        stress_result_text: Entity,
        /// Photosensitivity warning, centered in the panel —
        /// independent and absolute-positioned, unlike
        /// `stress_result_text` (see its own spawn-site doc for why: a
        /// `ChildOf` entity still rides along in an outbound bake
        /// regardless of its live `Visibility`, so hiding it that way
        /// wouldn't actually keep it out of the transition). Fixed
        /// content (never changes at runtime, unlike the result line).
        /// Visible only while settled and idle on this screen with no
        /// test running (see `advance_stress_warning_visibility`);
        /// hidden the instant a test *or* a transition starts.
        stress_warning_text: Entity,
        /// The currently in-flight stress test, if any — see
        /// `StressTestRun`'s doc. `None` means a button click starts a
        /// fresh run; `Some` means clicks are ignored until it finishes.
        stress_test_run: Option<StressTestRun>,
        /// Seed/state for the tiny xorshift32 PRNG `random_unit_f32`
        /// advances — Burst Spawn's only source of randomness (this file
        /// has no `rand` crate dependency, and doesn't need
        /// cryptographic-quality randomness for jittering stress-test
        /// target positions/colors).
        stress_rng_state: u32,
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
        /// Color-dark counterparts of `nav_icons`/`home_icon_selected`, each
        /// a child of its light counterpart — cross-faded via
        /// `theme_progress` (`home_icon_dark`/`back_icon_dark`) or the
        /// split-alpha step function tied to `home_selected_fade`/
        /// `dark_target` (`home_icon_selected_dark`) — see `advance_theme`.
        home_icon_dark: Entity,
        back_icon_dark: Entity,
        home_icon_selected_dark: Entity,
        /// Persistent brand mark + "PROTEUS" wordmark, top-left of `nav_icons`.
        logo_lockup: Entity,
        /// Color-dark counterpart, a child of `logo_lockup` — cross-faded
        /// over it via `theme_progress` (see `advance_theme`).
        logo_lockup_dark: Entity,
        /// Light/dark theme toggle icons, top-right — `sun_icon` is the
        /// "inner" (closer to center) icon, `moon_icon` the "outer" one
        /// (closest to the right edge). Only whichever one does *not* match
        /// the current theme carries `Interactable` at any given moment
        /// (see `advance_theme`).
        sun_icon: Entity,
        moon_icon: Entity,
        /// Each icon's permanent dark-theme look, cross-faded in via
        /// `theme_progress` — see `advance_theme`'s doc for why sun/moon
        /// only ever need a 2-layer crossfade (their "selected" state is
        /// fully determined by which theme is current, not an independent
        /// axis, unlike `home_icon_selected`).
        sun_icon_dark: Entity,
        moon_icon_dark: Entity,

        // ── Photo gallery (M12) ──────────────────────────────────────────────
        /// 4×3 grid, row-major (idx = row*4 + col; row 0 = top row) — see
        /// `start_gallery_to_home`'s column groups for how these map onto
        /// `nav_buttons[0..3]`.
        gallery_tiles: [Entity; 12],
        /// Triggers a `Gallery -> Loading` refetch on click — see
        /// `advance_gallery_button_fade` (fade in/out) and
        /// `advance_gallery_fetch_button_hover` (hover glow).
        gallery_fetch_button: Entity,
        gallery_fetch_button_label: Entity,
        /// Shown only if the fetch times out — see `advance_gallery_fetch`.
        gallery_error_text: Entity,
        /// Separate from Splash's `button`/`logo_frames` — this one
        /// theme-crossfades (see `loading_logo_dark`), which Splash's never
        /// had to.
        loading_logo: Entity,
        /// Color-dark counterpart, a child of `loading_logo` — cross-faded
        /// via `theme_progress` (added to `advance_theme`'s dark-overlay
        /// array).
        loading_logo_dark: Entity,
        /// Mirrors `logo_frames`'s own async-arrival shape (populated via a
        /// new `add_logo_frame_dark` export, `None` for any frame not yet
        /// fetched).
        loading_logo_frames_dark: Vec<Option<(TextureId, BakedImage)>>,
        loading_logo_frame_index: usize,
        loading_logo_frame_elapsed: f32,
        /// Seconds since the fetch started, while resting in `Loading`. Past
        /// `GALLERY_FETCH_TIMEOUT`, `advance_gallery_fetch` gives up and
        /// shows `gallery_error_text` instead of proceeding to `Gallery`.
        gallery_fetch_elapsed: f32,
        /// `true` once the timeout has fired this Loading visit — latched so
        /// the error stays visible rather than re-triggering every frame.
        gallery_error_shown: bool,
        /// `loading_logo`/`loading_logo_dark` alpha multiplier — fades to 0
        /// once `gallery_error_shown` fires, back to 1 immediately on the
        /// next fetch (`start_home_to_loading`/`start_gallery_to_loading`).
        gallery_logo_error_fade: f32,
        /// Bumped once per fetch kicked off (`start_home_to_loading`/
        /// `start_gallery_to_loading`). Paired with
        /// `gallery_tile_fetch_generation` so a re-fetch's "all tiles
        /// loaded" check can't be satisfied by the *previous* fetch's
        /// images still sitting on the tiles — see `set_gallery_image`.
        gallery_fetch_generation: u32,
        /// Which `gallery_fetch_generation` each tile's current `Image` was
        /// stamped with when it arrived — `u32::MAX` (never matches a real
        /// generation) until the tile's first ever image lands.
        gallery_tile_fetch_generation: [u32; 12],
        /// The picsum.photos id each tile's low-res image was fetched with —
        /// reused to request the *same* photo, just bigger, when it's
        /// enlarged (`start_gallery_to_image`).
        gallery_tile_photo_id: [u32; 12],
        /// The (width, height) ratio each tile's assigned photo actually is
        /// — JS pairs each curated picsum id with its own real dimensions
        /// (see `NATURE_PHOTOS` in index.html), so every fetch of it
        /// (low-res and hires alike) requests exactly that ratio: picsum
        /// then only ever resizes, never crops, which is what makes the
        /// hires crossfade never reframe. Drives the enlarged view's
        /// contain-fit sizing (`gallery_large_image_quad`) and the hires
        /// fetch's requested dimensions.
        gallery_tile_aspect: [(f32, f32); 12],
        /// Each tile's *full* (uncropped) bake, stashed by
        /// `bake_pending_images` before it center-crops the tile's own live
        /// `BakedImage` down to a square for grid display. Copied onto
        /// `gallery_enlarged_base` whenever that tile is enlarged
        /// (`start_gallery_to_image`/`settle_gallery_enlarged_base`) — the
        /// low-res stand-in shown until hires arrives must show the same
        /// full frame hires will, or swapping to hires would itself read as
        /// a reframe/zoom. The tile's own `BakedImage` is never touched —
        /// it stays cropped the whole time, see `gallery_enlarged_base`'s
        /// doc for why.
        gallery_tile_full_baked: [Option<BakedImage>; 12],
        /// Dedicated coordinator/display entity for the enlarged image —
        /// distinct from all 12 `gallery_tiles`. `start_gallery_to_image`/
        /// `start_image_to_gallery`/`start_image_to_home` use this as the
        /// `NToOneRequest` destination / `OneToNRequest` source instead of
        /// reusing whichever `gallery_tiles[idx]` was clicked, because that
        /// tile is *also* one of the 12 targets a return-to-grid split
        /// reveals into: a single entity can't simultaneously bake as "the
        /// enlarged photo" (for the coordinator's own crossfade) and "the
        /// grid's cropped square" (for its own target slot) — `bake_one`
        /// reads whatever `BakedImage` currently sits on the entity, once,
        /// so those two roles need two different components at the same
        /// instant. Keeping the coordinator on its own entity means the 12
        /// real tiles are never touched/re-cropped at all — they just stay
        /// hidden and cropped for the entire `GalleryImage` visit, exactly
        /// like the other 11 always were.
        gallery_enlarged_base: Entity,
        /// Glow-only hover progress for `gallery_enlarged_base` — see
        /// `advance_gallery_enlarged_hover`.
        gallery_enlarged_hover_progress: f32,
        gallery_enlarged_is_hovering: bool,
        /// Overlay quad for the enlarged image's hires crossfade — glued to
        /// `gallery_enlarged_base` (`advance_gallery_hires_overlay`
        /// copies its position/size/scale/corner_radius every frame,
        /// scale included so a hover scale-boost on the base doesn't leave
        /// the (unscaled) overlay poking out around it), alpha ramping 0→1
        /// once the hires fetch has baked
        /// (`bake_gallery_hires_image`). Hidden/cleared whenever nothing is
        /// enlarged.
        gallery_hires_overlay: Entity,
        /// Crossfade progress (0..1) for `gallery_hires_overlay`'s alpha.
        gallery_hires_fade: f32,
        /// Which tile's hires fetch is currently authoritative — `None`
        /// once cancelled (backed out of `GalleryImage` before it arrived)
        /// so a late `set_gallery_hires_image` call for the old tile is
        /// ignored instead of applying to the wrong (or no longer enlarged)
        /// image.
        gallery_hires_for_tile: Option<usize>,
        /// Taken by `take_gallery_hires_fetch_request()`, polled once per
        /// `tick()` from index.html — same "Rust signals, JS polls and
        /// fulfills" shape as `pending_gallery_fetch`.
        pending_gallery_hires_fetch: Option<GalleryHiresRequest>,
        /// Set whenever `cancel_gallery_hires_fetch` runs; taken by
        /// `take_gallery_hires_cancel()` so JS can abort the in-flight
        /// `fetch()` via its `AbortController`.
        pending_gallery_hires_cancel: bool,
        gallery_tile_hover_progress: [f32; 12],
        gallery_tile_is_hovering: [bool; 12],
        /// Fade-in (after the grid morph settles) / fade-out (in sync with
        /// the Gallery→Home morph) progress for `gallery_fetch_button`.
        gallery_button_fade: f32,
        /// Hover glow/scale progress (0..1) for `gallery_fetch_button` —
        /// same pattern as `nav_hover_progress`/`gallery_tile_hover_progress`.
        gallery_fetch_button_hover_progress: f32,
        gallery_fetch_button_is_hovering: bool,
        /// `Some(side_px)` exactly once per Home→Loading entry — taken by
        /// `take_gallery_fetch_request()`, polled once per `tick()` from
        /// index.html (same "Rust signals, JS polls and fulfills" shape as
        /// `pending_video_start`).
        pending_gallery_fetch: Option<u32>,

        state: AppState,
        /// `Some` while a transition between two `AppState`s is in flight.
        transition: Option<Transition>,

        staged_pointer: StagedPointer,

        // ── demo animation state ───────────────────────────────────────────
        intro_delay_remaining: f32,
        intro_elapsed: f32,
        splash_hold_remaining: f32,
        nav_hover_progress: [f32; 3],
        nav_is_hovering: [bool; 3],
        example_hover_progress: [f32; 6],
        example_is_hovering: [bool; 6],
        /// Fade-in progress for whichever category's own heading is
        /// currently active (see `example_heading_entity`) — 0 the instant
        /// the 6→1 merge into `example_detail_panel` completes, ramping to
        /// 1 only once fully settled and idle in `ExampleDetail`, so the
        /// heading visibly fades in *after* the morph lands, not alongside
        /// it.
        example_title_fade: f32,
        tile_hover_progress: [f32; 3],
        tile_is_hovering: [bool; 3],
        /// Fade-in/out progress (0..1) for `nav_icons`.
        nav_icon_fade: [f32; 2],
        nav_icon_hover_progress: [f32; 2],
        nav_icon_is_hovering: [bool; 2],
        /// Cross-fade progress (0..1) between `home_icon_selected` (1.0) and
        /// the idle home icon art beneath it (0.0).
        home_selected_fade: f32,
        /// Fade-in progress (0..1) for `logo_lockup` — shares `home_target`'s
        /// timing (see `advance_nav_icons`) but is tracked separately.
        logo_fade: f32,

        // ── Light/dark theme toggle ─────────────────────────────────────────
        /// `true` once the user has clicked toward dark — flips instantly on
        /// click (the other icon becomes clickable right away; the morph
        /// itself is purely cosmetic catch-up, see `theme_progress`).
        dark_target: bool,
        /// 0.0 = fully light, 1.0 = fully dark. Ramped toward `dark_target`'s
        /// value every frame by `advance_theme`, which derives every themed
        /// entity's corner radius/color/image-crossfade alpha from this one
        /// scalar — the whole point of the feature.
        theme_progress: f32,
        /// Fade-in progress (0..1) for `[sun_icon, moon_icon]` — shares
        /// `home_target`'s timing, tracked separately, same as `logo_fade`.
        theme_icon_fade: [f32; 2],
        /// Hover state for whichever of `sun_icon`/`moon_icon` currently
        /// carries `Interactable` — a scalar, not a per-icon array, since
        /// only one is ever interactive at a time.
        theme_icon_hover_progress: f32,
        theme_icon_is_hovering: bool,

        // ── Animated logo (replaces the old "START" text label) ────────────
        /// `(TextureId, BakedImage)` per frame, pre-baked into `main_atlas` as
        /// `add_logo_frame` is called from JS, indexed by frame number − 1.
        /// `None` for any frame not yet fetched/decoded/registered — the
        /// animation just skips over gaps and keeps showing whatever frame
        /// was last successfully set.
        logo_frames: Vec<Option<(TextureId, BakedImage)>>,
        /// Index into `logo_frames` currently shown on `button`.
        logo_frame_index: usize,
        /// Seconds accumulated since the last frame advance.
        logo_frame_elapsed: f32,

        // ── MP4 playback (M9.5) ─────────────────────────────────────────────
        // There's no background decode thread on wasm32 — the browser's own
        // `<video>` element is the decoder (see index.html). Rust's job is
        // just to (a) signal *when* to start/stop, via the `pending_video_*`
        // fields JS polls once per `tick()`, and (b) accept pushed frames via
        // `push_video_frame` and forward them straight to
        // `QuadPipeline::upload_video_frame` — no channel, no thread.
        playing_video: Option<PlayingVideo>,
        pending_video_start: Option<u32>,
        pending_video_stop: bool,
    }

    /// Tracks the one video currently playing (at most one — the video
    /// screen is always a single tile at a time).
    struct PlayingVideo {
        tile_idx: usize,
        texture_id: TextureId,
    }

    #[wasm_bindgen]
    impl ProteusApp {
        /// Initialise Proteus on the `<canvas>` element with the given `id`.
        ///
        /// Returns a JS `Promise<ProteusApp>`.  Call `tick(dt_ms)` inside
        /// `requestAnimationFrame` to drive the render loop.
        #[wasm_bindgen]
        pub async fn init(canvas_id: String) -> Result<ProteusApp, JsValue> {
            console_error_panic_hook::set_once();
            wasm_logger::init(wasm_logger::Config::default());

            log::info!("ProteusApp::init — canvas #{canvas_id}");

            // Locate the canvas in the DOM.
            let canvas = web_sys::window()
                .ok_or_else(|| JsValue::from_str("no window"))?
                .document()
                .ok_or_else(|| JsValue::from_str("no document"))?
                .get_element_by_id(&canvas_id)
                .ok_or_else(|| JsValue::from_str("canvas element not found"))?
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .map_err(|_| JsValue::from_str("element is not a canvas"))?;

            let width = canvas.width().max(1);
            let height = canvas.height().max(1);

            // Browser instance — prefers WebGPU, falls back to WebGL2.
            // Do NOT restrict to `Backends::GL` here: that path hangs at
            // `request_adapter` in Chrome builds that also have WebGPU present
            // because the GL future stalls on internal wgpu book-keeping that
            // assumes the WebGPU path ran first.  `all()` lets wgpu pick the
            // best backend available in the current browser.
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            });

            // Surface bound to the canvas.  On wasm32 the canvas reference has
            // JS-managed lifetime so the Surface is effectively 'static.
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|e| JsValue::from_str(&format!("create_surface: {e}")))?;

            // Adapter — no high-performance preference; compatible with our canvas surface.
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .map_err(|_| JsValue::from_str("no suitable WebGPU or WebGL2 adapter"))?;

            let info = adapter.get_info();
            log::info!("Adapter: {} (backend: {:?})", info.name, info.backend);

            // Device & queue — conservative WebGL2-compatible limits (safe under WebGPU too).
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("proteus-web"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: Default::default(),
                    ..Default::default()
                })
                .await
                .map_err(|e| JsValue::from_str(&format!("request_device: {e}")))?;

            // Surface configuration.
            let surface_caps = surface.get_capabilities(&adapter);
            // Explicitly avoid an sRGB-tagged surface format — see the
            // matching comment in proteus-shell-native/src/main.rs. This
            // build's WebGL2 surface doesn't offer an sRGB-capable format
            // today anyway (falls back to plain `Bgra8Unorm`), but picking
            // it explicitly rather than by accident guards against a future
            // WebGPU backend reintroducing the native/web color mismatch
            // this was the actual cause of.
            let surface_format = surface_caps
                .formats
                .iter()
                .find(|f| !f.is_srgb())
                .copied()
                .unwrap_or(surface_caps.formats[0]);

            let surface_config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: surface_format,
                width,
                height,
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: surface_caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            surface.configure(&device, &surface_config);

            // Render pipeline.
            let atlas_config = AtlasConfig::default();
            validate_atlas_config(&device, &atlas_config)
                .map_err(|e| JsValue::from_str(&format!("AtlasConfig: {e}")))?;
            let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096, atlas_config);
            pipeline.set_view_projection(&queue, QuadPipeline::ortho(width as f32, height as f32));

            log::info!(
                "GPU ready — {}×{} px, format {:?}",
                width,
                height,
                surface_format,
            );

            // Font atlas.
            let font_atlas = FontAtlas::with_embedded_font();

            // ECS world + demo entities.
            let mut ui_world = ProteusWorld::new();
            ui_world.world.insert_resource(GpuContext {
                device: device.clone(),
                queue: queue.clone(),
            });
            ui_world.world.insert_resource(pipeline);
            ui_world.world.insert_resource(font_atlas);

            // Full-window backdrop (see `advance_background`) — z=0.0 so
            // every other entity draws over it. Sized properly on the very
            // first `advance_background` call; the size here just avoids a
            // zero-size flash before that runs. Bytes arrive later via
            // `set_background_image`.
            let background = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.0),
                        size: Vec2::new(width as f32, height as f32),
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
            // Color-dark counterpart — a child of `background` (zero
            // relative offset, so position/size compose for free from the
            // parent), cross-faded in via `theme_progress` alone (see
            // `advance_theme`). Bytes arrive later via
            // `set_background_dark_image`.
            let background_dark = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::ZERO,
                        size: Vec2::new(width as f32, height as f32),
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

            let button = ui_world
                .world
                .spawn((start_button_quad(), Lifecycle::Idle, Visibility::VISIBLE))
                .id();
            // Logo frames arrive later via `add_logo_frame` (JS fetches them
            // after `init()` resolves) — the button starts as a blank
            // transparent quad until frame 1 lands.
            let logo_frames: Vec<Option<(TextureId, BakedImage)>> =
                (0..LOGO_FRAME_COUNT).map(|_| None).collect();
            // Color-dark counterpart, arriving later via a new
            // `add_logo_frame_dark` export — used only by the Loading
            // screen's `loading_logo_dark` (Splash's own `button`/
            // `logo_frames` never theme-crossfades).
            let loading_logo_frames_dark: Vec<Option<(TextureId, BakedImage)>> =
                (0..LOGO_FRAME_COUNT).map(|_| None).collect();

            // The "PROTEUS" wordmark (M10 composition, same pattern as the
            // old "START" label) — a `Text` child of the button. Local X
            // starts at 0; `advance_intro_and_hover` moves it to sit just
            // right of the mark once `bake_pending_text` has measured the
            // glyph run's actual width (needed for pixel-accurate placement
            // — see WORDMARK_GAP_PX's doc).
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
            // theme-crossfade) since this one does. Just the animated mark,
            // no wordmark — centered on the window; `advance_loading_logo_animation`
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
            // Color-dark counterpart — a child of `loading_logo` (zero
            // relative offset), cross-faded in via `theme_progress` alone,
            // same shape as `logo_lockup`/`logo_lockup_dark` (see
            // `advance_theme`).
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
            // transition. Placeholder geometry; `start_splash_to_nav`
            // overwrites position/size once each label's baked width is known.
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

            // Examples & Tests category buttons — same spawn recipe as
            // nav_buttons (border/glow/interactable), start hidden.
            // Placeholder geometry; `layout_example_buttons` overwrites
            // position/size once each label's baked width is known.
            let mut example_buttons = [Entity::PLACEHOLDER; 6];
            let mut example_labels = [Entity::PLACEHOLDER; 6];
            for i in 0..6 {
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
                example_buttons[i] = btn;
                example_labels[i] = ui_world
                    .world
                    .spawn((
                        QuadState {
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::VISIBLE,
                        Text::new(EXAMPLE_CATEGORY_TITLES[i], NAV_BUTTON_LABEL_SIZE_PX)
                            .with_color(violet())
                            .with_letter_spacing(NAV_BUTTON_LETTER_SPACING_PX),
                        ChildOf(btn),
                    ))
                    .id();
            }

            // Dedicated enlarged-category entity — see
            // `example_detail_panel`'s doc for why this can't just be
            // whichever `example_buttons[idx]` was clicked.
            let example_detail_panel = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.5),
                        size: Vec2::ONE,
                        rotation: 0.0,
                        scale: 1.0,
                        anchor: Vec2::new(0.5, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        corner_radius: 0.0,
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    nav_button_border(),
                ))
                .id();

            let mut example_content: [Vec<Entity>; 6] = std::array::from_fn(|_| Vec::new());
            // See `example_reparent_exempt`'s doc — populated below wherever
            // a category spawns a nested (grandchild) hierarchy.
            let mut example_reparent_exempt: Vec<Entity> = Vec::new();

            // Effects (ExampleDetail(0)) content: a title (rendered
            // *outside* the panel, above it, independent of the panel's
            // bake — fades in on its own after settle) plus 4 labelled
            // rows (Drop Shadow / Glow / Border / Opacity), each a row
            // label followed by a horizontal strip of boxes. The rows are
            // `ChildOf` `example_detail_panel` — see `example_content`'s
            // doc for why — so their positions below are *local*,
            // relative to the panel's own center, with a small positive
            // local Z (`EXAMPLE_CONTENT_LOCAL_Z`) so they render above the panel's
            // opaque fill once composed. Spawned at placeholder positions
            // here; `layout_effects_content` (called from
            // `settle_example_detail_panel`) does the real positioning
            // once `example_detail_panel`'s actual (window-dependent)
            // geometry is known.
            let (effects_title, effects_row_labels, effects_row_boxes, effects_opacity_item_labels) = {
                // A non-brand accent color, for the "different color" example
                // in the Glow/Border rows — everything else in this demo is
                // violet.
                let accent = Vec4::new(0.95, 0.55, 0.25, 1.0);

                let title = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new("Effects", 28.0).with_color(violet()),
                    ))
                    .id();

                let label = |ui_world: &mut ProteusWorld, text: &str, size_px: f32| {
                    ui_world
                        .world
                        .spawn((
                            QuadState {
                                position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                                ..Default::default()
                            },
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            Text::new(text, size_px).with_color(violet()),
                            ChildOf(example_detail_panel),
                        ))
                        .id()
                };
                let effect_box = || QuadState {
                    position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                    size: Vec2::new(64.0, 64.0),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: white(),
                    corner_radius: 10.0,
                };

                // Drop Shadow — soft small offset, hard large offset, large
                // spread (a wider, less-blurred silhouette, not a halo — same
                // black as the other two, just a bigger spread/lower
                // softness, so it doesn't read as a Glow effect instead).
                let drop_shadow_label = label(&mut ui_world, "Drop Shadow", 16.0);
                let drop_shadow_boxes = vec![
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            DropShadow {
                                offset: Vec2::new(2.0, -2.0),
                                color: Vec4::new(0.0, 0.0, 0.0, 0.5),
                                softness: 4.0,
                                spread: 0.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            DropShadow {
                                offset: Vec2::new(8.0, -8.0),
                                color: Vec4::new(0.0, 0.0, 0.0, 0.8),
                                softness: 2.0,
                                spread: 2.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            QuadState {
                                size: Vec2::new(61.0, 61.0),
                                ..effect_box()
                            },
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            DropShadow {
                                offset: Vec2::new(6.0, -6.0),
                                color: Vec4::new(0.0, 0.0, 0.0, 0.6),
                                softness: 6.0,
                                spread: 10.0,
                            },
                        ))
                        .id(),
                ];

                // Glow — small radius, large radius, non-brand accent color.
                let glow_label = label(&mut ui_world, "Glow", 16.0);
                let glow_boxes = vec![
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Glow {
                                radius: 6.0,
                                color: violet(),
                                intensity: 1.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Glow {
                                radius: 24.0,
                                color: violet(),
                                intensity: 1.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Glow {
                                radius: 16.0,
                                color: accent,
                                intensity: 1.0,
                            },
                        ))
                        .id(),
                ];

                // Border — thin, thick, different color. Inner offset only —
                // `Border::offset` center/outer are a known-broken limitation
                // (see the type's own doc), not demonstrated here.
                let border_label = label(&mut ui_world, "Border", 16.0);
                let border_boxes = vec![
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Border {
                                width: 2.0,
                                color: violet(),
                                offset: -1.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Border {
                                width: 8.0,
                                color: violet(),
                                offset: -1.0,
                            },
                        ))
                        .id(),
                    ui_world
                        .world
                        .spawn((
                            effect_box(),
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            ChildOf(example_detail_panel),
                            Border {
                                width: 4.0,
                                color: accent,
                                offset: -1.0,
                            },
                        ))
                        .id(),
                ];

                // Opacity — 4 flat `color.w` swatches, plus one nested
                // parent+child pair using the real `Opacity`/`EffectiveOpacity`
                // cascade (already wired into the schedule and already read by
                // `collect_instances` — no new plumbing needed): parent
                // `Opacity(0.6)` with a child also `Opacity(0.6)` multiplies
                // down to an effective 0.36, distinguishing "flat alpha" from
                // "cascaded opacity."
                let opacity_label = label(&mut ui_world, "Opacity", 16.0);
                let mut opacity_boxes = Vec::with_capacity(5);
                let mut opacity_item_labels = Vec::with_capacity(5);
                for (alpha, caption) in [(0.25, "25%"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%")]
                {
                    opacity_boxes.push(
                        ui_world
                            .world
                            .spawn((
                                QuadState {
                                    position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                                    size: Vec2::new(64.0, 64.0),
                                    rotation: 0.0,
                                    scale: 1.0,
                                    anchor: Vec2::new(0.5, 0.5),
                                    color: Vec4::new(1.0, 1.0, 1.0, alpha),
                                    corner_radius: 10.0,
                                },
                                Lifecycle::Idle,
                                Visibility::HIDDEN,
                                ChildOf(example_detail_panel),
                            ))
                            .id(),
                    );
                    opacity_item_labels.push(label(&mut ui_world, caption, 12.0));
                }
                let nested_parent = ui_world
                    .world
                    .spawn((
                        effect_box(),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Opacity(0.6),
                        ChildOf(example_detail_panel),
                    ))
                    .id();
                let nested_child = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::ZERO,
                            size: Vec2::new(36.0, 36.0),
                            rotation: 0.0,
                            scale: 1.0,
                            anchor: Vec2::new(0.5, 0.5),
                            color: accent,
                            corner_radius: 6.0,
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Opacity(0.6),
                        ChildOf(nested_parent),
                    ))
                    .id();
                // `ChildOf(nested_parent)`, not the panel — see
                // `example_reparent_exempt`'s doc for why this must be
                // tracked so `set_example_category_active` never
                // reparents it.
                example_reparent_exempt.push(nested_child);
                opacity_boxes.push(nested_parent);
                opacity_item_labels.push(label(&mut ui_world, "0.6 × 0.6", 12.0));

                let opacity_item_labels: [Entity; 5] = opacity_item_labels.try_into().unwrap();

                // `nested_child` is never repositioned by
                // `layout_effects_content` (it's placed purely by
                // `ChildOf(nested_parent)` composition), but it still
                // needs its own `Visibility` toggled at settle time like
                // every other piece of content — without this it was
                // never added to `example_content[0]`, so it stayed
                // permanently hidden in the steady state, only ever
                // appearing mid-transition since baking ignores
                // `Visibility`.
                example_content[0].push(nested_child);
                example_content[0].push(title);
                for &e in [drop_shadow_label, glow_label, border_label, opacity_label]
                    .iter()
                    .chain(drop_shadow_boxes.iter())
                    .chain(glow_boxes.iter())
                    .chain(border_boxes.iter())
                    .chain(opacity_boxes.iter())
                    .chain(opacity_item_labels.iter())
                {
                    example_content[0].push(e);
                }

                (
                    title,
                    [drop_shadow_label, glow_label, border_label, opacity_label],
                    [drop_shadow_boxes, glow_boxes, border_boxes, opacity_boxes],
                    opacity_item_labels,
                )
            };

            // Text (ExampleDetail(1)) content: mirrors Effects' structure —
            // an independent title above the panel (fades in on its own
            // after settle) plus 3 labelled rows `ChildOf`
            // `example_detail_panel` (baked as part of the panel's own
            // crossfade, same rationale as `example_content`'s doc). Each
            // row demonstrates one `Text` parameter at a time, holding the
            // sample string constant.
            let (text_title, text_row_labels, text_row_items) = {
                const SAMPLE: &str = "Sample Text";
                let accent = Vec4::new(0.95, 0.55, 0.25, 1.0);
                // The sample text itself reads as very dark grey (not
                // violet, unlike the row labels) — closer to how body text
                // actually looks, and it keeps the Color row's
                // violet/white/accent items the only samples using
                // brand/accent color on purpose.
                let text_grey = Vec4::new(0.16, 0.16, 0.17, 1.0);

                let title = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new("Text", 28.0).with_color(violet()),
                    ))
                    .id();

                let label = |ui_world: &mut ProteusWorld, text: &str, size_px: f32| {
                    ui_world
                        .world
                        .spawn((
                            QuadState {
                                position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                                ..Default::default()
                            },
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            Text::new(text, size_px).with_color(violet()),
                            ChildOf(example_detail_panel),
                        ))
                        .id()
                };
                let sample = |ui_world: &mut ProteusWorld, text: Text| {
                    ui_world
                        .world
                        .spawn((
                            QuadState {
                                position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                                ..Default::default()
                            },
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            text,
                            ChildOf(example_detail_panel),
                        ))
                        .id()
                };

                // Font Size — same sample string at 14/20/28/40px.
                let font_size_label = label(&mut ui_world, "Font Size", 16.0);
                let font_size_items: Vec<Entity> = [14.0, 20.0, 28.0, 40.0]
                    .into_iter()
                    .map(|size| {
                        sample(&mut ui_world, Text::new(SAMPLE, size).with_color(text_grey))
                    })
                    .collect();

                // Color — same string in dark grey, white, one extra
                // accent color (the row's whole point, so these two stay
                // put).
                let color_label = label(&mut ui_world, "Color", 16.0);
                let color_items: Vec<Entity> = [text_grey, white(), accent]
                    .into_iter()
                    .map(|color| sample(&mut ui_world, Text::new(SAMPLE, 20.0).with_color(color)))
                    .collect();

                // Letter Spacing — same string at 0/4/10px spacing.
                let letter_spacing_label = label(&mut ui_world, "Letter Spacing", 16.0);
                let letter_spacing_items: Vec<Entity> = [0.0, 4.0, 10.0]
                    .into_iter()
                    .map(|spacing| {
                        sample(
                            &mut ui_world,
                            Text::new(SAMPLE, 20.0)
                                .with_color(text_grey)
                                .with_letter_spacing(spacing),
                        )
                    })
                    .collect();

                example_content[1].push(title);
                for &e in [font_size_label, color_label, letter_spacing_label]
                    .iter()
                    .chain(font_size_items.iter())
                    .chain(color_items.iter())
                    .chain(letter_spacing_items.iter())
                {
                    example_content[1].push(e);
                }

                (
                    title,
                    [font_size_label, color_label, letter_spacing_label],
                    [font_size_items, color_items, letter_spacing_items],
                )
            };

            // Transforms & Animation (ExampleDetail(2)) content: mirrors
            // Effects' structure — an independent title above the panel
            // (fades in on its own after settle) plus 3 labelled rows
            // `ChildOf` `example_detail_panel` (baked as part of the
            // panel's own crossfade, same rationale as
            // `example_content`'s doc). Rotation/Scale are static, each
            // box labelled with its own value underneath; Continuous
            // Animation is one box kept spinning/breathing/color-cycling
            // by `advance_example_animation` for as long as this screen
            // is active.
            let (
                transforms_title,
                transforms_row_labels,
                transforms_row_boxes,
                transforms_item_labels,
                transforms_continuous_box,
            ) = {
                let title = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new("Transforms & Animation", 28.0).with_color(violet()),
                    ))
                    .id();

                let label = |ui_world: &mut ProteusWorld, text: &str, size_px: f32| {
                    ui_world
                        .world
                        .spawn((
                            QuadState {
                                position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                                ..Default::default()
                            },
                            Lifecycle::Idle,
                            Visibility::HIDDEN,
                            Text::new(text, size_px).with_color(violet()),
                            ChildOf(example_detail_panel),
                        ))
                        .id()
                };
                let transform_box = |rotation: f32, scale: f32| QuadState {
                    position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                    size: Vec2::new(64.0, 64.0),
                    rotation,
                    scale,
                    anchor: Vec2::new(0.5, 0.5),
                    color: white(),
                    corner_radius: 10.0,
                };

                // Rotation — 0°/20°/-35°, each labelled underneath with
                // its own value.
                let rotation_label = label(&mut ui_world, "Rotation", 16.0);
                let rotation_degrees = [0.0_f32, 20.0, -35.0];
                let rotation_boxes: Vec<Entity> = rotation_degrees
                    .iter()
                    .map(|deg| {
                        ui_world
                            .world
                            .spawn((
                                transform_box(deg.to_radians(), 1.0),
                                Lifecycle::Idle,
                                Visibility::HIDDEN,
                                ChildOf(example_detail_panel),
                            ))
                            .id()
                    })
                    .collect();
                let rotation_item_labels: Vec<Entity> = rotation_degrees
                    .iter()
                    .map(|deg| label(&mut ui_world, &format!("{deg:.0}°"), 12.0))
                    .collect();

                // Scale — 0.6×/1.0×/1.4×, each labelled underneath with
                // its own value.
                let scale_label = label(&mut ui_world, "Scale", 16.0);
                let scale_factors = [0.6_f32, 1.0, 1.4];
                let scale_boxes: Vec<Entity> = scale_factors
                    .iter()
                    .map(|scale| {
                        ui_world
                            .world
                            .spawn((
                                transform_box(0.0, *scale),
                                Lifecycle::Idle,
                                Visibility::HIDDEN,
                                ChildOf(example_detail_panel),
                            ))
                            .id()
                    })
                    .collect();
                let scale_item_labels: Vec<Entity> = scale_factors
                    .iter()
                    .map(|scale| label(&mut ui_world, &format!("{scale:.1}×"), 12.0))
                    .collect();

                // Continuous Animation — one box, kept
                // spinning/breathing/color-cycling by
                // `advance_example_animation` for as long as this screen
                // is active.
                let continuous_label = label(&mut ui_world, "Continuous Animation", 16.0);
                let continuous_box = ui_world
                    .world
                    .spawn((
                        transform_box(0.0, 1.0),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        ChildOf(example_detail_panel),
                    ))
                    .id();

                example_content[2].push(title);
                for &e in [rotation_label, scale_label, continuous_label]
                    .iter()
                    .chain(rotation_boxes.iter())
                    .chain(scale_boxes.iter())
                    .chain(std::iter::once(&continuous_box))
                    .chain(rotation_item_labels.iter())
                    .chain(scale_item_labels.iter())
                {
                    example_content[2].push(e);
                }

                (
                    title,
                    [rotation_label, scale_label, continuous_label],
                    [rotation_boxes, scale_boxes, vec![continuous_box]],
                    [rotation_item_labels, scale_item_labels, Vec::new()],
                    continuous_box,
                )
            };

            // Stress Tests (ExampleDetail(3)) content: an independent
            // title above the panel (fades in on its own after settle),
            // two action buttons *below* the panel (also
            // independent/absolute, side by side, left-aligned as a pair
            // to the panel's left edge — this screen's panel fills
            // available vertical space rather than being sized from a
            // fixed content height, so unlike every other category's
            // rows these buttons live outside it, not `ChildOf` it), and
            // a result line that *does* stay `ChildOf` the panel, near
            // its bottom.
            let (
                stress_title,
                stress_buttons,
                stress_button_labels,
                stress_result_text,
                stress_warning_text,
            ) = {
                let title = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new("Stress Tests", 28.0).with_color(violet()),
                    ))
                    .id();

                // Same shape as
                // `gallery_fetch_button`/`gallery_fetch_button_label`
                // (border + hover glow + Interactable, label as a
                // `ChildOf` child) — independent/absolute like `title`,
                // not `ChildOf` the panel, since these sit outside its
                // bounds.
                let button = |ui_world: &mut ProteusWorld, label_text: &str| -> (Entity, Entity) {
                    let button = ui_world
                        .world
                        .spawn((
                            QuadState {
                                position: Vec3::new(0.0, 0.0, 0.51),
                                // Placeholder width —
                                // `layout_stress_content` overwrites it
                                // with this button's own label width
                                // before it's ever shown.
                                size: Vec2::new(200.0, STRESS_BUTTON_HEIGHT_PX),
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
                    let label = ui_world
                        .world
                        .spawn((
                            QuadState {
                                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                                ..Default::default()
                            },
                            Lifecycle::Idle,
                            Visibility::VISIBLE,
                            Text::new(label_text, NAV_BUTTON_LABEL_SIZE_PX)
                                .with_color(violet())
                                .with_letter_spacing(NAV_BUTTON_LETTER_SPACING_PX),
                            ChildOf(button),
                        ))
                        .id();
                    (button, label)
                };

                let (burst_button, burst_label) = button(&mut ui_world, "Run Burst Spawn");
                let (churn_button, churn_label) = button(&mut ui_world, "Run Texture Churn");

                let result_text = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, EXAMPLE_CONTENT_LOCAL_Z),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        // A single space, not an empty string:
                        // `bake_pending_text` only ever bakes entities
                        // without `BakedText`, and an empty string fails
                        // to rasterize at all (see
                        // `FontAtlas::rasterize_text_tracked`), which
                        // would leave this entity permanently "pending"
                        // — retried, and logging a warning, every single
                        // frame.
                        Text::new(" ", 16.0).with_color(violet()),
                        ChildOf(example_detail_panel),
                    ))
                    .id();

                // Photosensitivity warning — centered in the panel
                // (`layout_stress_content` positions it in *absolute*
                // world coordinates, matching `title`/the buttons),
                // independent — deliberately NOT `ChildOf` the panel,
                // unlike `result_text`. `gather_bake_instances` bakes a
                // subtree's children unconditionally, ignoring
                // `Visibility` (the same root cause `example_content`'s
                // doc already documents for Effects/Text/Transforms
                // content): if this were `ChildOf` the panel,
                // `advance_stress_warning_visibility` hiding it the
                // instant a transition starts wouldn't stop the outbound
                // bake — taken *before* that hide lands — from capturing
                // it anyway, so it would still visibly ride along
                // through the whole transition. Independent content is
                // never gathered into any bake at all, so this concern
                // doesn't apply.
                let warning_text = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new(
                            "Warning: the Texture Churn test may be harmful to photosensitive users.",
                            24.0,
                        )
                        .with_color(violet()),
                    ))
                    .id();

                // Buttons and the warning text are independent/absolute
                // (see their own docs), and the button labels are
                // `ChildOf` their own button, not the panel — see
                // `example_reparent_exempt`'s doc for why all 5 must be
                // tracked there.
                example_reparent_exempt.push(burst_button);
                example_reparent_exempt.push(churn_button);
                example_reparent_exempt.push(burst_label);
                example_reparent_exempt.push(churn_label);
                example_reparent_exempt.push(warning_text);

                example_content[3].push(title);
                for &e in [burst_button, churn_button, result_text, warning_text]
                    .iter()
                    .chain(std::iter::once(&burst_label))
                    .chain(std::iter::once(&churn_label))
                {
                    example_content[3].push(e);
                }

                (
                    title,
                    [burst_button, churn_button],
                    [burst_label, churn_label],
                    result_text,
                    warning_text,
                )
            };

            // Layout (4) and 3D (5) are placeholders for future milestones
            // — just a centered "planned future feature" label, hidden
            // until their own `ExampleDetail` settles. Not `ChildOf`
            // anything — see `example_content`'s doc — but unlike Effects'
            // content this never needs repositioning: their panel has no
            // heading pushing it off center, so it always settles back to
            // world (0,0).
            for idx in [4usize, 5] {
                let placeholder = ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(0.0, 0.0, 0.51),
                            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                            ..Default::default()
                        },
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new("Planned future feature", NAV_BUTTON_LABEL_SIZE_PX)
                            .with_color(violet()),
                    ))
                    .id();
                example_content[idx].push(placeholder);
            }

            // Tiles start hidden; box-cover art (fetched separately, see
            // set_tile_image) makes a text label redundant, so tiles carry
            // no Text component.
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

            // Photo gallery grid — "tile w/o label" recipe (border + hover
            // glow, no overlay-tint/title-label children, unlike the video
            // tiles above). Placeholder geometry; `layout_gallery_tiles`
            // overwrites it once the canvas size is known/on each
            // Home→Loading entry. Images are attached later, as each
            // loremflickr fetch completes (`set_gallery_image`, called from
            // JS).
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

            // "Fetch New Images" — same visual shape as a nav button; a
            // visual placeholder only this pass, never wired into
            // advance_demo's click handling (see advance_gallery_button_fade
            // for its fade in/out).
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
            // advance_gallery_fetch) — dead center of the canvas (the
            // looping logo fades out to make room for it; see
            // advance_gallery_error_fade).
            let gallery_error_text = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        ..Default::default()
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(LOADING_LOGO_ERROR_TEXT, 18.0).with_color(white()),
                ))
                .id();

            // Dedicated enlarged-image entity — see `gallery_enlarged_base`'s
            // doc for why this can't just be whichever `gallery_tiles[idx]`
            // was clicked. Same component recipe as a gallery tile (it *is*
            // one, visually) minus being part of the `gallery_tiles` array.
            let gallery_enlarged_base = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.5),
                        size: Vec2::ONE,
                        rotation: 0.0,
                        scale: 1.0,
                        anchor: Vec2::new(0.5, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        corner_radius: 0.0,
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Interactable,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id();

            // Enlarged gallery image's hires crossfade overlay — position/
            // size/corner_radius kept glued to `gallery_enlarged_base` by
            // `advance_gallery_hires_overlay`; a fixed z slightly ahead of
            // that entity's own 0.5 so it renders on top once its alpha
            // ramps up (same "child z = parent z + offset" stacking
            // convention as `loading_logo_dark`/`background_dark`, just
            // without an actual `ChildOf` — this entity's position isn't a
            // fixed offset from a parent, it has to track wherever the
            // enlarged image currently is, including mid-morph).
            let gallery_hires_overlay = ui_world
                .world
                .spawn((
                    QuadState {
                        position: Vec3::new(0.0, 0.0, 0.6),
                        size: Vec2::ONE,
                        rotation: 0.0,
                        scale: 1.0,
                        anchor: Vec2::new(0.5, 0.5),
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        corner_radius: 0.0,
                    },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    tile_border(),
                    tile_hover_glow(),
                ))
                .id();

            // Home/back nav icons — top-left, hidden until tiles/screen
            // show. Border+glyph are baked into the PNGs themselves
            // (Color-light treatment); bytes arrive later via
            // `set_nav_icon_image`, JS-fetched same as box-cover art.
            let nav_icons = [0, 1].map(|_| {
                ui_world
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
                    .id()
            });

            // Color-dark counterparts of nav_icons — children layered on
            // top, cross-faded in via `theme_progress` alone (see
            // `advance_theme`). Bytes arrive later via
            // `set_home_icon_dark_image`/`set_back_icon_dark_image`.
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

            // "Selected" home icon art — a child of nav_icons[0], layered on
            // top and cross-faded in via its own alpha (see
            // `advance_nav_icons`) rather than swapping the base entity's
            // texture: both PNGs already bake in their own opaque fill, so a
            // plain alpha blend between the two stacked quads reads as a
            // real crossfade. Bytes arrive later via
            // `set_home_icon_selected_image`.
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
            // Color-dark counterpart of `home_icon_selected` — same shape,
            // but driven by a split-alpha step function (not a continuous
            // crossfade) tied to `dark_target`, since it shares the
            // `home_selected_fade` envelope with its light sibling rather
            // than fading independently. See `advance_theme`'s doc for why
            // this is a deliberate simplification rather than true bilinear
            // (page-selected × theme) compositing. Bytes arrive later via
            // `set_home_icon_selected_dark_image`.
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

            // Light/dark theme toggle — sun/moon, top-right. `sun_icon`
            // holds sun's *dark*-theme ring art and `sun_icon_dark` holds
            // its *light*-theme solid-disc art — an inversion of what the
            // names suggest, see `set_sun_icon_image`'s doc for why. Moon's
            // roles match its name as usual. Only moon starts
            // `Interactable`; `advance_theme` moves it to whichever icon
            // doesn't match the current theme. Bytes arrive later via
            // `set_sun_icon_image`/`set_moon_icon_image`.
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

            // Persistent brand lockup (mark + "PROTEUS" wordmark), top-left —
            // hidden until the splash finishes morphing into Home, then
            // fades in and stays up through every other state (see
            // `advance_nav_icons`). Bytes arrive later via
            // `set_logo_lockup_image`.
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
            // Color-dark counterpart — a child of `logo_lockup` (zero
            // relative offset), cross-faded in via `theme_progress` alone.
            // Bytes arrive later via `set_logo_lockup_dark_image`.
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

            log::info!(
                "Demo entities — button {:?}, wordmark {:?}, nav_buttons {:?}, tiles {:?}, tile_overlays {:?}, tile_labels {:?}, nav_icons {:?}, logo_lockup {:?}",
                button,
                wordmark,
                nav_buttons,
                tiles,
                tile_overlays,
                tile_labels,
                nav_icons,
                logo_lockup
            );

            Ok(ProteusApp {
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
                example_buttons,
                example_labels,
                example_detail_panel,
                example_content,
                effects_title,
                effects_row_labels,
                effects_row_boxes,
                effects_opacity_item_labels,
                text_title,
                text_row_labels,
                text_row_items,
                transforms_title,
                transforms_row_labels,
                transforms_row_boxes,
                transforms_item_labels,
                transforms_continuous_box,
                transforms_anim_elapsed: 0.0,
                example_reparent_exempt,
                stress_title,
                stress_buttons,
                stress_button_labels,
                stress_button_hover_progress: [0.0; 2],
                stress_button_is_hovering: [false; 2],
                stress_button_fade: 0.0,
                stress_result_text,
                stress_warning_text,
                stress_test_run: None,
                stress_rng_state: 0x9E3779B9,
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
                gallery_enlarged_base,
                gallery_enlarged_hover_progress: 0.0,
                gallery_enlarged_is_hovering: false,
                gallery_hires_overlay,
                loading_logo,
                loading_logo_dark,
                loading_logo_frames_dark,
                loading_logo_frame_index: 0,
                loading_logo_frame_elapsed: 0.0,
                gallery_fetch_elapsed: 0.0,
                gallery_error_shown: false,
                gallery_logo_error_fade: 1.0,
                gallery_fetch_generation: 0,
                gallery_tile_fetch_generation: [u32::MAX; 12],
                gallery_tile_photo_id: [0; 12],
                gallery_tile_aspect: [(1.0, 1.0); 12],
                gallery_tile_full_baked: std::array::from_fn(|_| None),
                gallery_hires_fade: 0.0,
                gallery_hires_for_tile: None,
                pending_gallery_hires_fetch: None,
                pending_gallery_hires_cancel: false,
                gallery_tile_hover_progress: [0.0; 12],
                gallery_tile_is_hovering: [false; 12],
                gallery_button_fade: 0.0,
                gallery_fetch_button_hover_progress: 0.0,
                gallery_fetch_button_is_hovering: false,
                pending_gallery_fetch: None,
                state: AppState::Splash,
                transition: None,
                staged_pointer: StagedPointer::default(),
                intro_delay_remaining: INTRO_DELAY,
                intro_elapsed: 0.0,
                splash_hold_remaining: SPLASH_HOLD_DURATION,
                nav_hover_progress: [0.0; 3],
                nav_is_hovering: [false; 3],
                example_hover_progress: [0.0; 6],
                example_is_hovering: [false; 6],
                example_title_fade: 0.0,
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
                pending_video_start: None,
                pending_video_stop: false,
            })
        }

        /// Advance one frame.  `dt_ms` is the elapsed time in milliseconds
        /// (pass `performance.now()` delta from the rAF callback).
        #[wasm_bindgen]
        pub fn tick(&mut self, dt_ms: f32) {
            let dt = (dt_ms / 1000.0).min(0.05); // cap at 50 ms

            // Flush staged JS pointer events → PointerInput ECS resource.
            {
                let mut pi = self.ui_world.world.resource_mut::<PointerInput>();
                pi.position = self.staged_pointer.position;
                pi.just_pressed = self.staged_pointer.just_pressed;
                pi.just_released = self.staged_pointer.just_released;
                pi.is_pressed = self.staged_pointer.is_pressed;
            }
            // Clear one-shot flags — they're true for exactly one frame.
            self.staged_pointer.just_pressed = false;
            self.staged_pointer.just_released = false;

            self.ui_world.update(dt);
            self.bake_pending_text();
            self.bake_gallery_hires_image();
            self.bake_pending_images();
            self.advance_gallery_fetch(dt);
            self.advance_background();
            self.advance_intro_and_hover(dt);
            self.advance_logo_animation(dt);
            self.advance_loading_logo_animation(dt);
            self.advance_nav_hover(dt);
            self.advance_example_button_hover(dt);
            self.advance_tile_hover(dt);
            self.advance_gallery_tile_hover(dt);
            self.advance_gallery_enlarged_hover(dt);
            self.advance_gallery_button_fade(dt);
            self.advance_gallery_fetch_button_hover(dt);
            self.advance_example_title_fade(dt);
            self.advance_example_animation(dt);
            self.advance_stress_button_hover(dt);
            self.advance_stress_button_fade(dt);
            self.advance_stress_warning_visibility();
            self.advance_stress_test(dt);
            self.advance_demo(dt);
            self.advance_nav_icons(dt);
            self.advance_theme(dt);
            self.advance_gallery_error_fade(dt);
            self.advance_gallery_hires_overlay(dt);

            // The functions above mutate `Visibility` directly (e.g. `settle`
            // hiding/revealing tiles and nav buttons) — refresh the cascaded
            // `EffectiveVisibility`/`EffectiveOpacity` collect_instances
            // actually reads, or those changes render one frame late (see
            // `ProteusWorld::refresh_cascades`'s doc — this was the cause of
            // hidden tiles flashing visible for a frame right after a
            // transition landed).
            self.ui_world.refresh_cascades();

            // Collect visible instances.
            // See `proteus_ui::collect` for the two-instance-per-text-entity model.
            let instances = collect_instances(&mut self.ui_world.world);

            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                    self.surface.configure(&self.device, &self.surface_config);
                    return;
                }
                e => {
                    log::error!("Surface error: {e:?}");
                    return;
                }
            };

            let view = frame.texture.create_view(&Default::default());

            let mut pipeline = self.ui_world.world.resource_mut::<QuadPipeline>();

            if !instances.is_empty() {
                pipeline.upload_instances(&self.queue, &instances);
            }

            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frame"),
                });

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("main"),
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
        }

        /// Notify Proteus that the canvas has been resized to `width` × `height`
        /// CSS pixels.  Call this from a ResizeObserver callback.
        #[wasm_bindgen]
        pub fn resize(&mut self, width: u32, height: u32) {
            if width == 0 || height == 0 {
                return;
            }
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
            self.ui_world
                .world
                .resource::<QuadPipeline>()
                .set_view_projection(
                    &self.queue,
                    QuadPipeline::ortho(width as f32, height as f32),
                );
        }

        // ── Pointer event entry points (called from JS) ────────────────────────

        /// Report a pointer move.  `x` and `y` are CSS pixels (origin top-left).
        /// Converts to world-space (origin centre, Y up) before storing.
        /// Call from `canvas.addEventListener('mousemove', ...)`.
        #[wasm_bindgen]
        pub fn on_mouse_move(&mut self, x: f32, y: f32) {
            let w = self.surface_config.width as f32;
            let h = self.surface_config.height as f32;
            self.staged_pointer.position = Some(Vec2::new(x - w / 2.0, h / 2.0 - y));
        }

        /// Report that the pointer has left the canvas.
        /// Call from `canvas.addEventListener('mouseleave', ...)`.
        #[wasm_bindgen]
        pub fn on_mouse_leave(&mut self) {
            self.staged_pointer.position = None;
        }

        /// Report a primary-button press.
        /// Call from `canvas.addEventListener('mousedown', ...)`.
        #[wasm_bindgen]
        pub fn on_mouse_down(&mut self) {
            self.staged_pointer.is_pressed = true;
            self.staged_pointer.just_pressed = true;
        }

        /// Report a primary-button release.
        /// Call from `canvas.addEventListener('mouseup', ...)`.
        #[wasm_bindgen]
        pub fn on_mouse_up(&mut self) {
            self.staged_pointer.is_pressed = false;
            self.staged_pointer.just_released = true;
        }

        // ── MP4 playback (M9.5) ─────────────────────────────────────────────
        //
        // There's no background decode thread on wasm32 — the browser's own
        // `<video>` element is the decoder. JS polls `take_video_start_tile`/
        // `take_video_stop` once per `tick()` to learn when a tile was
        // clicked (Rust owns hit-testing; there's no per-tile DOM element for
        // JS to attach its own click listener to), drives a hidden `<video>`
        // element accordingly, and pushes decoded frames via
        // `push_video_frame` — see index.html for the JS side.

        /// Returns the tile index playback should start for, once, or
        /// `undefined` if nothing changed since the last call. Call once per
        /// `tick()`; on `Some`, load/play that tile's video file and — once
        /// `loadedmetadata` fires — call [`start_video`](Self::start_video).
        #[wasm_bindgen]
        pub fn take_video_start_tile(&mut self) -> Option<u32> {
            self.pending_video_start.take()
        }

        /// Returns `true` once, the first `tick()` after the screen was
        /// clicked to stop playback — the corresponding `<video>` element
        /// should be paused. Rust-side texture/component cleanup has already
        /// happened by the time this flips true.
        #[wasm_bindgen]
        pub fn take_video_stop(&mut self) -> bool {
            std::mem::replace(&mut self.pending_video_stop, false)
        }

        /// Sizes the pipeline's video texture and attaches `VideoPlayer` to
        /// `tiles[tile_idx]`. Call once `<video>`'s `loadedmetadata` event has
        /// fired, passing its `videoWidth`/`videoHeight`.
        #[wasm_bindgen]
        pub fn start_video(&mut self, tile_idx: u32, width: u32, height: u32) {
            let (texture_id, _sender) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .init_video(&self.device, width, height);
            // `_sender` (the BYOV channel's sending half) goes unused on
            // wasm32 — `push_video_frame` uploads directly instead of routing
            // through the channel, since blocking on a full bounded channel
            // would deadlock with no second thread free to drain it.
            self.ui_world
                .world
                .entity_mut(self.tiles[tile_idx as usize])
                .insert((VideoPlayer, VideoCrossfade { video_t: 0.0 }));
            self.playing_video = Some(PlayingVideo {
                tile_idx: tile_idx as usize,
                texture_id,
            });
        }

        /// Uploads one decoded RGBA frame (`width×height×4` bytes, matching
        /// whatever `start_video` was called with) straight to the video
        /// texture. Call once per `<video>` `requestVideoFrameCallback`.
        #[wasm_bindgen]
        pub fn push_video_frame(&mut self, rgba: &[u8]) {
            if self.playing_video.is_some() {
                self.ui_world
                    .world
                    .resource::<QuadPipeline>()
                    .upload_video_frame(&self.queue, rgba);
            }
        }

        /// Attaches box-cover art to `tiles[tile_idx]` (M9.7). Call once,
        /// after JS has fetched the image file's bytes — there's no fetch on
        /// the Rust side; `bake_pending_images` (run every `tick()`) decodes
        /// and uploads it to `main_atlas` on the next frame.
        #[wasm_bindgen]
        pub fn set_tile_image(&mut self, tile_idx: u32, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.tiles[tile_idx as usize])
                .insert(Image::new(bytes));
            // Untinted — the real box art replaces the placeholder
            // TILE_COLORS fill, so it shouldn't be tinted by it.
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.tiles[tile_idx as usize])
            {
                qs.color = white();
            }
        }

        // ── Photo gallery fetch (M12) ────────────────────────────────────────
        //
        // Rust signals *when* to fetch (on entering `Loading`, from either
        // Home or a "Fetch New Images" click) via
        // `take_gallery_fetch_request`, polled once per `tick()` — same
        // "polled take" shape as `take_video_start_tile`. JS does the actual
        // loremflickr.com `fetch()` calls (12 nature-themed image fetches)
        // and hands bytes back via `set_gallery_image`.

        /// Returns `Some(side_px)` exactly once per Loading entry — the
        /// square pixel size JS should request each of the 12 gallery images
        /// at. `None` if nothing changed since the last call.
        #[wasm_bindgen]
        pub fn take_gallery_fetch_request(&mut self) -> Option<u32> {
            self.pending_gallery_fetch.take()
        }

        /// Attaches a fetched gallery image (same round-trip as
        /// `set_tile_image`). Call once per tile, after JS has fetched its
        /// bytes.
        ///
        /// Clears the tile's *own* previous `BakedImage`/`TextureRef` right
        /// here, before inserting the new `Image` — `bake_pending_images`
        /// only bakes entities that don't already have a `BakedImage`, so on
        /// a re-fetch (the tile already showing last batch's photo) this is
        /// required, not just tidy: without it, the new bytes would sit on
        /// the entity forever, silently never baked. Stamping
        /// `gallery_tile_fetch_generation` here (rather than clearing every
        /// tile up front when the fetch starts) is what lets each tile
        /// update independently, whenever its own fetch actually resolves,
        /// without racing the collapse-into-`loading_logo` animation that
        /// needs the *outgoing* tiles' bake intact when it starts — see
        /// `start_gallery_to_loading`.
        ///
        /// `photo_id`/`aspect_w`/`aspect_h` are stashed
        /// (`gallery_tile_photo_id`/`gallery_tile_aspect`) for later reuse
        /// if this tile gets enlarged — the same `photo_id` requests the
        /// same picsum.photos photo at a bigger size
        /// (`start_gallery_to_image`), and `aspect` drives the enlarged
        /// view's contain-fit sizing.
        #[wasm_bindgen]
        pub fn set_gallery_image(
            &mut self,
            tile_idx: u32,
            bytes: &[u8],
            photo_id: u32,
            aspect_w: f32,
            aspect_h: f32,
        ) {
            let tile = self.gallery_tiles[tile_idx as usize];
            self.ui_world
                .world
                .entity_mut(tile)
                .remove::<(BakedImage, TextureRef, Image)>()
                .insert(Image::new(bytes));
            self.gallery_tile_fetch_generation[tile_idx as usize] = self.gallery_fetch_generation;
            self.gallery_tile_photo_id[tile_idx as usize] = photo_id;
            self.gallery_tile_aspect[tile_idx as usize] = (aspect_w, aspect_h);
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
                qs.color = white();
            }
        }

        /// Returns the pending hires fetch for the enlarged gallery image,
        /// exactly once per `start_gallery_to_image` call — polled once per
        /// `tick()` from index.html, same "polled take" shape as
        /// `take_gallery_fetch_request`.
        #[wasm_bindgen]
        pub fn take_gallery_hires_fetch_request(&mut self) -> Option<GalleryHiresRequest> {
            self.pending_gallery_hires_fetch.take()
        }

        /// `true` exactly once whenever `cancel_gallery_hires_fetch` just
        /// ran (backing out of `GalleryImage` before the hires image
        /// arrived) — JS should `.abort()` its in-flight fetch's
        /// `AbortController`, if any, on seeing this.
        #[wasm_bindgen]
        pub fn take_gallery_hires_cancel(&mut self) -> bool {
            std::mem::take(&mut self.pending_gallery_hires_cancel)
        }

        /// Attaches the fetched hires bytes to `gallery_hires_overlay` — a
        /// no-op if `tile_idx` isn't (or is no longer) the tile currently
        /// enlarged (`gallery_hires_for_tile`), which is exactly what makes
        /// a response that arrives *after* the user already backed out (or
        /// enlarged a different tile) harmless to apply, even though JS's
        /// own `AbortController` should normally have already cut the
        /// request off before this point.
        #[wasm_bindgen]
        pub fn set_gallery_hires_image(&mut self, tile_idx: u32, bytes: &[u8]) {
            if self.gallery_hires_for_tile != Some(tile_idx as usize) {
                return;
            }
            self.ui_world
                .world
                .entity_mut(self.gallery_hires_overlay)
                .remove::<(BakedImage, TextureRef, Image)>()
                .insert(Image::new(bytes));
        }

        /// Attaches a home/back nav icon image (`icon_idx` 0 = home, 1 =
        /// back). Same round-trip as `set_tile_image` — `bake_pending_images`
        /// (run every `tick()`) decodes and uploads it on the next frame.
        #[wasm_bindgen]
        pub fn set_nav_icon_image(&mut self, icon_idx: u32, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.nav_icons[icon_idx as usize])
                .insert(Image::new(bytes));
        }

        /// Attaches the "selected" home icon art (`home_icon_selected`, a
        /// child of `nav_icons[0]`). Same round-trip as `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_home_icon_selected_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.home_icon_selected)
                .insert(Image::new(bytes));
        }

        /// Attaches the persistent brand lockup (`logo_lockup`, mark +
        /// "PROTEUS" wordmark, top-left). Same round-trip as
        /// `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_logo_lockup_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.logo_lockup)
                .insert(Image::new(bytes));
        }

        /// Attaches the full-window backdrop image (`background`). Same
        /// round-trip as `set_tile_image` — `bake_pending_images` (run every
        /// `tick()`) decodes, downsamples, and uploads it on the next frame.
        #[wasm_bindgen]
        pub fn set_background_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.background)
                .insert(Image::new(bytes));
        }

        /// Attaches the Color-dark backdrop counterpart (`background_dark`,
        /// cross-faded over `background` via `theme_progress` — see
        /// `advance_theme`). Same round-trip as `set_background_image`.
        #[wasm_bindgen]
        pub fn set_background_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.background_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches the Color-dark home icon counterpart (`home_icon_dark`,
        /// a child of `nav_icons[0]`). Same round-trip as
        /// `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_home_icon_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.home_icon_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches the Color-dark back icon counterpart (`back_icon_dark`,
        /// a child of `nav_icons[1]`). Same round-trip as
        /// `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_back_icon_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.back_icon_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches the Color-dark "selected" home icon counterpart
        /// (`home_icon_selected_dark`). Same round-trip as
        /// `set_home_icon_selected_image`.
        #[wasm_bindgen]
        pub fn set_home_icon_selected_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.home_icon_selected_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches the Color-dark brand lockup counterpart
        /// (`logo_lockup_dark`). Same round-trip as `set_logo_lockup_image`.
        #[wasm_bindgen]
        pub fn set_logo_lockup_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.logo_lockup_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches sun's permanent *dark*-theme art (a thin, mostly-
        /// transparent ring) onto the always-visible `sun_icon` base —
        /// an inversion of what the method/field name suggests. Alpha-over
        /// compositing can only occlude what's beneath it where the top
        /// layer's own pixels are opaque, so the solid-fill "selected" art
        /// (sun's *light*-theme look) has to be the one sitting in the
        /// overlay slot instead — see `set_sun_icon_dark_image`. Same
        /// round-trip as `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_sun_icon_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.sun_icon)
                .insert(Image::new(bytes));
        }

        /// Attaches sun's permanent *light*-theme art (the solid-fill
        /// "selected" disc) onto the `sun_icon_dark` overlay, cross-faded
        /// over `sun_icon` via `1.0 - theme_progress` (inverted from every
        /// other themed overlay in `advance_theme` — fully opaque while
        /// light, correctly hiding the thin ring beneath it, fading away as
        /// the theme goes dark) — see `set_sun_icon_image`'s doc for why.
        /// Moon doesn't need this inversion: its solid "selected" art is
        /// already the *dark*-theme look, so it already belongs in the
        /// overlay slot with the ordinary `theme_progress`-driven alpha.
        #[wasm_bindgen]
        pub fn set_sun_icon_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.sun_icon_dark)
                .insert(Image::new(bytes));
        }

        /// Attaches the moon icon's permanent light-theme art (`moon_icon`
        /// — idle-and-hoverable, since light theme makes moon the active
        /// icon). Same round-trip as `set_nav_icon_image`.
        #[wasm_bindgen]
        pub fn set_moon_icon_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.moon_icon)
                .insert(Image::new(bytes));
        }

        /// Attaches the moon icon's permanent dark-theme art
        /// (`moon_icon_dark` — already "selected", since moon is always
        /// selected while dark), cross-faded over `moon_icon` via
        /// `theme_progress`.
        #[wasm_bindgen]
        pub fn set_moon_icon_dark_image(&mut self, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.moon_icon_dark)
                .insert(Image::new(bytes));
        }

        /// Registers one animated-logo frame (`frame_idx` 0..`LOGO_FRAME_COUNT`,
        /// matching `frame-{frame_idx+1:02}.png`). Call once per frame, after
        /// JS has fetched its bytes — there's no fetch on the Rust side, and
        /// unlike `set_tile_image`/`Image`/`bake_pending_images`, this bakes
        /// immediately rather than waiting for next `tick()`'s bake pass,
        /// since `advance_logo_animation` reads `logo_frames` directly rather
        /// than going through `BakedImage` on a per-entity basis. Eternal —
        /// see the eviction-safety note on `TextureRegistry::register_static`,
        /// this content must never be evicted mid-loop. If `frame_idx == 0`
        /// and the button has no image yet, shows it immediately instead of
        /// waiting for the first `advance_logo_animation` tick.
        #[wasm_bindgen]
        pub fn add_logo_frame(&mut self, frame_idx: u32, bytes: &[u8]) {
            let frame_idx = frame_idx as usize;
            let decoded = match proteus_render::decode_image(bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("logo frame {frame_idx}: could not decode: {e}");
                    return;
                }
            };
            let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);
            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "logo frame {frame_idx}: main_atlas full — could not register {}×{}",
                    decoded.width,
                    decoded.height,
                );
                return;
            };
            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, placement, &decoded.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            let baked = BakedImage {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [decoded.width as f32, decoded.height as f32],
            };

            if frame_idx == 0 && self.ui_world.world.get::<BakedImage>(self.button).is_none() {
                self.ui_world
                    .world
                    .entity_mut(self.button)
                    .insert((baked.clone(), TextureRef(texture_id)));
            }
            if let Some(slot) = self.logo_frames.get_mut(frame_idx) {
                *slot = Some((texture_id, baked));
            }
        }

        /// Color-dark counterpart of `add_logo_frame` — writes into
        /// `loading_logo_frames_dark` instead, and never touches `button`
        /// (only `loading_logo_dark`, driven by
        /// `advance_loading_logo_animation`, ever shows these frames).
        #[wasm_bindgen]
        pub fn add_logo_frame_dark(&mut self, frame_idx: u32, bytes: &[u8]) {
            let frame_idx = frame_idx as usize;
            let decoded = match proteus_render::decode_image(bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("logo frame dark {frame_idx}: could not decode: {e}");
                    return;
                }
            };
            let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);
            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "logo frame dark {frame_idx}: main_atlas full — could not register {}×{}",
                    decoded.width,
                    decoded.height,
                );
                return;
            };
            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, placement, &decoded.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            let baked = BakedImage {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [decoded.width as f32, decoded.height as f32],
            };
            if let Some(slot) = self.loading_logo_frames_dark.get_mut(frame_idx) {
                *slot = Some((texture_id, baked));
            }
        }
    }

    // ── private helpers ────────────────────────────────────────────────────────

    impl ProteusApp {
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
                // resource_scope — bevy's pattern for needing a specific
                // resource and general World access (for the entity_mut
                // insert below) without a borrow conflict.
                let glyphs =
                    self.ui_world
                        .world
                        .resource_scope::<FontAtlas, _>(|_world, mut font_atlas| {
                            font_atlas.rasterize_text_tracked(&content, size_px, letter_spacing_px)
                        });
                let Some(glyphs) = glyphs else {
                    log::warn!("FontAtlas: could not rasterize '{content}'");
                    continue;
                };

                // M11: allocation moved from FontAtlas's old shelf packer to
                // the real TextureRegistry — register the region, then upload.
                let Some(texture_id) = self
                    .ui_world
                    .world
                    .resource_mut::<QuadPipeline>()
                    .texture_registry
                    .register_static(glyphs.width, glyphs.height, false)
                else {
                    log::warn!(
                        "bake_pending_text: main_atlas full — could not register {}x{}",
                        glyphs.width,
                        glyphs.height,
                    );
                    continue;
                };

                let pipeline = self.ui_world.world.resource::<QuadPipeline>();
                let placement = pipeline
                    .texture_registry
                    .main_atlas_region(texture_id)
                    .expect("just registered");
                pipeline.write_to_main_atlas(&self.queue, placement, &glyphs.rgba_pixels);
                let uv = pipeline
                    .texture_registry
                    .main_atlas_uv(texture_id)
                    .expect("just registered");

                self.ui_world.world.entity_mut(entity).insert((
                    BakedText {
                        uv_offset: uv.uv_offset,
                        uv_scale: uv.uv_scale,
                        page: uv.page,
                        pixel_size: [glyphs.width as f32, glyphs.height as f32],
                    },
                    TextureRef(texture_id),
                ));
            }
        }

        /// For every entity with `Image` but no `BakedImage`: decode → upload
        /// to main_atlas (via the same shelf packer `bake_pending_text` uses
        /// — see `FontAtlas::bake_image`) → insert `BakedImage`. Mirrors
        /// `bake_pending_text` exactly. There's no fetch here — `Image` is
        /// only ever inserted once JS has already fetched the bytes and
        /// handed them over via `set_tile_image`.
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
                        log::warn!("bake_pending_images: {e}");
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
                        "bake_pending_images: main_atlas full — could not register {}×{} image",
                        decoded.width,
                        decoded.height,
                    );
                    continue;
                };

                let pipeline = self.ui_world.world.resource::<QuadPipeline>();
                let placement = pipeline
                    .texture_registry
                    .main_atlas_region(texture_id)
                    .expect("just registered");
                pipeline.write_to_main_atlas(&self.queue, placement, &decoded.rgba_pixels);
                let uv = pipeline
                    .texture_registry
                    .main_atlas_uv(texture_id)
                    .expect("just registered");

                let full_baked_image = BakedImage {
                    uv_offset: uv.uv_offset,
                    uv_scale: uv.uv_scale,
                    page: uv.page,
                    pixel_size: [decoded.width as f32, decoded.height as f32],
                };
                // Gallery tiles display in square grid cells, but a fetched
                // photo's own real aspect ratio (see NATURE_PHOTOS in
                // index.html) is essentially never square — center-crop the
                // UV here, once, at bake time, so it fills the cell instead
                // of stretching/distorting. The *full* (uncropped) bake is
                // separately stashed in `gallery_tile_full_baked`:
                // `start_gallery_to_image` swaps it back in while enlarged
                // (the low-res stand-in shown until hires arrives must show
                // the same full frame hires will, or swapping to hires
                // would itself look like a reframe/zoom — the very bug this
                // fixes) and `settle_gallery_tile` re-crops from it once
                // back in the grid.
                let baked_image =
                    if let Some(idx) = self.gallery_tiles.iter().position(|&e| e == entity) {
                        let cropped = center_crop_to_square(full_baked_image.clone());
                        self.gallery_tile_full_baked[idx] = Some(full_baked_image);
                        cropped
                    } else {
                        full_baked_image
                    };
                self.ui_world
                    .world
                    .entity_mut(entity)
                    .insert((baked_image, TextureRef(texture_id)));
            }
        }

        /// Dedicated bake step for `gallery_hires_overlay`, mirroring
        /// `bake_pending_images` but resizing to `GALLERY_LARGE_IMAGE_MAX_SIDE`
        /// instead of `MAX_TILE_IMAGE_SIDE` — the shared 400px cap is sized
        /// for 12 simultaneous tiles; only one hires image is ever resident
        /// at a time, so it can be bigger. Must run before
        /// `bake_pending_images` each tick: once this bakes the overlay's
        /// `Image`, that generic pass's own "no `BakedImage` yet" filter
        /// skips it, instead of re-baking it at the wrong (smaller) cap.
        fn bake_gallery_hires_image(&mut self) {
            let entity = self.gallery_hires_overlay;
            if self.ui_world.world.get::<BakedImage>(entity).is_some() {
                return;
            }
            let Some(bytes) = self
                .ui_world
                .world
                .get::<Image>(entity)
                .map(|img| img.bytes.clone())
            else {
                return;
            };
            let decoded = match proteus_render::decode_image(&bytes) {
                Ok(decoded) => decoded,
                Err(e) => {
                    log::warn!("bake_gallery_hires_image: {e}");
                    return;
                }
            };
            let decoded = proteus_render::resize_to_fit(decoded, GALLERY_LARGE_IMAGE_MAX_SIDE);

            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(decoded.width, decoded.height, false)
            else {
                log::warn!(
                    "bake_gallery_hires_image: main_atlas full — could not register {}×{} image",
                    decoded.width,
                    decoded.height,
                );
                return;
            };

            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, placement, &decoded.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");

            self.ui_world.world.entity_mut(entity).insert((
                BakedImage {
                    uv_offset: uv.uv_offset,
                    uv_scale: uv.uv_scale,
                    page: uv.page,
                    pixel_size: [decoded.width as f32, decoded.height as f32],
                },
                TextureRef(texture_id),
            ));
        }

        /// Keeps the full-window backdrop sized to the current canvas every
        /// frame, so a resize doesn't leave it under/oversized — no
        /// animation, state, or hover involved, just a permanent,
        /// unconditional fill.
        fn advance_background(&mut self) {
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.background) {
                qs.size = Vec2::new(canvas_width, canvas_height);
            }
            // `background_dark` is a child with zero relative offset, but
            // size isn't inherited from the parent automatically — keep it
            // in sync too, same as the parent above.
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.background_dark)
            {
                qs.size = Vec2::new(canvas_width, canvas_height);
            }
        }

        /// Advances the one-shot entry fade and the hover glow sweep, then
        /// writes the results directly onto the button's
        /// `QuadState`/`Glow` components.
        ///
        /// Neither animation goes through `TransitionRequest` — they aren't
        /// morphs between two declared forms, just continuous alpha/radius
        /// sweeps driven by elapsed time and hover state, so it's simpler to
        /// drive them directly here than to route them through the
        /// transition system.
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
            // Mark + wordmark read as one composite, not "a mark with a
            // label" — centering just the mark (as before) would leave the
            // wordmark trailing off-center to the right. Once the wordmark's
            // actual baked width is known (see the doc on WORDMARK_GAP_PX's
            // neighbor consts), shift the mark left by half of the width the
            // wordmark *adds*, so the whole assembly is centered — then
            // slide the whole thing in from the left in lockstep with the
            // same eased fade curve `alpha` already drives, so the two
            // effects read as one motion rather than a fade and a slide
            // happening to overlap.
            //
            // This offset is computed in the same "unscaled" units as
            // WORDMARK_GAP_PX/LOGO_MARK_WIDTH —
            // `hierarchy::compose_with_parent` scales a child's local
            // position by the parent's own composed scale, so `button`'s own
            // COMPOSITE_SCALE factor is applied once, here, rather than
            // needing to be baked into this formula too.
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

        /// Advances the logo's frame-sweep animation while the button is
        /// idle (waiting for a click) — swaps which pre-baked frame's
        /// `BakedImage`/`TextureRef` sits on `button`, wrapping through
        /// `logo_frames` every `LOGO_FRAME_DURATION` seconds. Skips over any
        /// frame index JS hasn't fetched/registered yet via `add_logo_frame`
        /// rather than stalling the sweep on it. Stops advancing once
        /// `ButtonToTiles` begins: the button is either mid-morph (its
        /// current frame gets baked into the Slice transition's snapshot,
        /// same as any other `BakedImage` content) or already hidden, so
        /// there's nothing left to animate.
        fn advance_logo_animation(&mut self, dt: f32) {
            if self.state != AppState::Splash {
                return;
            }
            self.logo_frame_elapsed += dt;
            while self.logo_frame_elapsed >= LOGO_FRAME_DURATION {
                self.logo_frame_elapsed -= LOGO_FRAME_DURATION;
                self.logo_frame_index = (self.logo_frame_index + 1) % self.logo_frames.len();
                if let Some((texture_id, baked)) = self.logo_frames[self.logo_frame_index].clone() {
                    self.ui_world
                        .world
                        .entity_mut(self.button)
                        .insert((baked, TextureRef(texture_id)));
                }
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
            // `from`, or (once settled) `self.state` itself. Used below both
            // to suppress hover on the screen tile and to size its title
            // label.
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
                // The video screen is static while playing — no hover
                // glow/scale reaction, even though `Interactable` stays on
                // the entity (harmless: `advance_demo`'s `VideoScreen` arm
                // never checks for clicks on `tiles[screen_idx]` itself, only
                // the nav icons).
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
                        overlay_qs.size =
                            (tile_size - Vec2::splat(2.0 * BORDER_WIDTH)).max(Vec2::ZERO);
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
                // the in-flight morph, so there's nothing to interpolate — it
                // only needs the right value once settled into `VideoTiles` or
                // `VideoScreen`.
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

        /// Design-System hover: 15px glow + 5% scale, no fill/text-color
        /// change. Labels stay fully opaque throughout — unlike tile labels,
        /// they're the button's primary content, not a hover reveal.
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

        /// Same glow+scale hover reaction as `advance_nav_hover`, for
        /// `example_buttons` — suppressed whenever a transition is in
        /// flight or we're not resting in `ExamplesHome` (mirrors
        /// `advance_gallery_tile_hover`'s same reasoning: a button
        /// mid-morph, or one that's currently the detail panel, isn't
        /// meaningfully "hoverable").
        fn advance_example_button_hover(&mut self, dt: f32) {
            let suppressed = self.transition.is_some() || self.state != AppState::ExamplesHome;
            for i in 0..6 {
                let entity = self.example_buttons[i];
                {
                    let events = self.ui_world.world.resource::<InteractionEvents>();
                    if events.hover_entered.contains(&entity) {
                        self.example_is_hovering[i] = true;
                    } else if events.hover_exited.contains(&entity) {
                        self.example_is_hovering[i] = false;
                    }
                }
                let target = if suppressed {
                    0.0
                } else if self.example_is_hovering[i] {
                    1.0
                } else {
                    0.0
                };
                let step = dt / GLOW_DURATION;
                if self.example_hover_progress[i] < target {
                    self.example_hover_progress[i] =
                        (self.example_hover_progress[i] + step).min(target);
                } else if self.example_hover_progress[i] > target {
                    self.example_hover_progress[i] =
                        (self.example_hover_progress[i] - step).max(target);
                }
                if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                    glow.radius = self.example_hover_progress[i] * GLOW_MAX_RADIUS;
                }
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                    qs.scale = 1.0 + self.example_hover_progress[i] * HOVER_SCALE_BOOST;
                }
            }
        }

        /// Fades home/back icons and the logo lockup in/out based on phase,
        /// positions the logo lockup and both icons top-left (recomputed
        /// every frame so canvas resizes track correctly), drives the
        /// icons' Design-System hover (a 15px glow plus a 5% scale bump),
        /// and cross-fades the home icon's "selected" art in/out of its
        /// idle art depending on whether `Home` is the current or target
        /// state.
        fn advance_nav_icons(&mut self, dt: f32) {
            // The home icon is now up on every non-splash state (not just
            // once away from Home, like "back") — hidden only during
            // `Splash` itself and the splash→home morph, so it's already in
            // place the instant Home first appears rather than popping in a
            // beat later.
            let home_target: f32 = match (&self.transition, self.state) {
                (Some(t), _) if t.from == AppState::Splash => 0.0,
                (None, AppState::Splash) => 0.0,
                _ => 1.0,
            };
            // "back" only ever fades in once idle on the video screen or an
            // example detail screen — clicking home from either skips
            // straight to a `(_, Home)` transition
            // (`start_screen_to_nav`/`start_detail_to_home`), which falls
            // into the `_` arm below without needing a `then_home` flag.
            let back_target: f32 = match (&self.transition, self.state) {
                (None, AppState::VideoScreen(_)) => 1.0,
                (None, AppState::ExampleDetail(_)) => 1.0,
                _ => 0.0,
            };
            let targets = [home_target, back_target];
            // The home icon shows no hover reaction while already resting on
            // `Home` — clicking it there would be a no-op, so there's
            // nothing to invite hover feedback for (mirrors
            // `advance_tile_hover`'s `is_idle_screen` suppression for the
            // same reason).
            let home_icon_hover_suppressed =
                self.transition.is_none() && matches!(self.state, AppState::Home);

            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let logo_left_edge = -canvas_width / 2.0 + NAV_ICON_MARGIN_PX;
            let base_x =
                logo_left_edge + LOGO_TEXT_RIGHT_PX + LOGO_ICONS_GAP_PX + NAV_ICON_SIZE_PX / 2.0;
            let y = canvas_height / 2.0 - NAV_ICON_MARGIN_PX - NAV_ICON_SIZE_PX / 2.0;
            let xs = [base_x, base_x + NAV_ICON_SIZE_PX + NAV_ICON_GAP_PX];

            // The logo fades in alongside the home icon (same `home_target`
            // signal, same duration) the first time either appears, right
            // after the splash morph — and then, since that target never
            // returns to 0, stays up through every other state.
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

            // Home icon "selected" while Home is the current resting state
            // *or* the transition's destination — flipping the instant a
            // transition begins (not once it lands) is what makes this a
            // real crossfade rather than an instant swap at the very end of
            // the button/tile morph, which runs on its own much longer
            // timeline.
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
                let hover_target =
                    if target > 0.0 && self.nav_icon_is_hovering[i] && !suppress_hover {
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
        /// corner-radius/color/image-crossfade values onto every themed
        /// entity — unconditionally, every frame, regardless of `AppState`
        /// or visibility. That unconditional-every-frame property is what
        /// makes "a component already reflects the current theme by the
        /// time it becomes visible" true for free, with no visibility
        /// branching needed here at all.
        ///
        /// Runs after `advance_demo`/`advance_nav_icons` so it always has
        /// the final say for the frame — `settle` (inside `advance_demo`)
        /// resets tiles/buttons to hardcoded light-treatment values on every
        /// scene arrival, and this overwrites that with the current theme's
        /// actual values immediately after. One consequence: during the
        /// `VideoTiles ↔ VideoScreen` `TransitionRequest`, the framework's
        /// own transition-tick system eases `corner_radius` toward
        /// `video_screen_quad`'s hardcoded value, but this function
        /// overwrites it again right after — so corner radius never
        /// actually rides that transition's own eased curve, it's pinned
        /// flat to the current theme's value throughout. Harmless today
        /// since `TILE_CORNER_RADIUS` and `SCREEN_CORNER_RADIUS` are the
        /// same 20.0 (both theme-driven identically) — don't "fix" this
        /// later by adding corner_radius back into a `TransitionRequest`
        /// target expecting it to interpolate.
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
            // Tiles/screen are trickier: `tiles[i]` reuses the same entity
            // for both shapes, and while it's actively mid the VideoTiles ↔
            // VideoScreen `TransitionRequest`, its corner_radius is already
            // being eased by the framework's own transition-tick system
            // (from whichever shape it's leaving to whichever it's entering
            // — see `start_tiles_to_screen`/`start_screen_to_tiles`).
            // Reasserting a flat value here every frame would stomp that
            // mid-flight, so the corner radius would never actually
            // round up/down as the shape grows/shrinks — it'd just pop once
            // `settle` lands. So this only reasserts a theme-blended radius
            // for tiles at rest (either grid-shaped or fully the screen);
            // whichever one entity is actively morphing right now is left
            // alone until it settles.
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

            // Gallery tiles and the fetch button — same continuous
            // theme-driven corner radius as nav_buttons above (no "actively
            // morphing" concern like the video tiles' dual-shape reuse: a
            // gallery tile is always a single square-cell shape). Real
            // tiles stay hidden/static during a Loading<->Gallery GridSlice
            // transition (only the virtuals animate), so it's always safe
            // to reassert this.
            for &tile in &self.gallery_tiles {
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
                    qs.corner_radius = GALLERY_CORNER_RADIUS
                        + (GALLERY_CORNER_RADIUS_DARK - GALLERY_CORNER_RADIUS) * p;
                }
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_enlarged_base)
            {
                qs.corner_radius = GALLERY_CORNER_RADIUS
                    + (GALLERY_CORNER_RADIUS_DARK - GALLERY_CORNER_RADIUS) * p;
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_fetch_button)
            {
                qs.corner_radius = NAV_BUTTON_CORNER_RADIUS
                    + (NAV_BUTTON_CORNER_RADIUS_DARK - NAV_BUTTON_CORNER_RADIUS) * p;
            }

            // Example buttons — same continuous theme-driven corner radius
            // as nav_buttons above (no "actively morphing" concern like
            // the video tiles' dual-shape reuse: an example button is
            // always a single grid-cell shape now that
            // `example_detail_panel` is a dedicated entity, never one of
            // the 6 buttons themselves).
            for &btn in &self.example_buttons {
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(btn) {
                    qs.corner_radius = NAV_BUTTON_CORNER_RADIUS
                        + (NAV_BUTTON_CORNER_RADIUS_DARK - NAV_BUTTON_CORNER_RADIUS) * p;
                }
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.example_detail_panel)
            {
                qs.corner_radius =
                    SCREEN_CORNER_RADIUS + (SCREEN_CORNER_RADIUS_DARK - SCREEN_CORNER_RADIUS) * p;
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
            for &tile in &self.gallery_tiles {
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
            if let Some(mut b) = self
                .ui_world
                .world
                .get_mut::<Border>(self.gallery_enlarged_base)
            {
                b.color.x = primary.x;
                b.color.y = primary.y;
                b.color.z = primary.z;
            }
            if let Some(mut g) = self
                .ui_world
                .world
                .get_mut::<Glow>(self.gallery_enlarged_base)
            {
                g.color.x = primary.x;
                g.color.y = primary.y;
                g.color.z = primary.z;
            }
            if let Some(mut b) = self
                .ui_world
                .world
                .get_mut::<Border>(self.gallery_fetch_button)
            {
                b.color.x = primary.x;
                b.color.y = primary.y;
                b.color.z = primary.z;
            }
            if let Some(mut g) = self
                .ui_world
                .world
                .get_mut::<Glow>(self.gallery_fetch_button)
            {
                g.color.x = primary.x;
                g.color.y = primary.y;
                g.color.z = primary.z;
            }
            if let Some(mut t) = self
                .ui_world
                .world
                .get_mut::<Text>(self.gallery_fetch_button_label)
            {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            for i in 0..6 {
                let btn = self.example_buttons[i];
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
                let label = self.example_labels[i];
                if let Some(mut t) = self.ui_world.world.get_mut::<Text>(label) {
                    t.color.x = primary.x;
                    t.color.y = primary.y;
                    t.color.z = primary.z;
                }
            }
            if let Some(mut b) = self
                .ui_world
                .world
                .get_mut::<Border>(self.example_detail_panel)
            {
                b.color.x = primary.x;
                b.color.y = primary.y;
                b.color.z = primary.z;
            }
            if let Some(mut t) = self.ui_world.world.get_mut::<Text>(self.effects_title) {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            if let Some(mut t) = self.ui_world.world.get_mut::<Text>(self.text_title) {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            if let Some(mut t) = self.ui_world.world.get_mut::<Text>(self.transforms_title) {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            if let Some(mut t) = self.ui_world.world.get_mut::<Text>(self.stress_title) {
                t.color.x = primary.x;
                t.color.y = primary.y;
                t.color.z = primary.z;
            }
            for i in 0..2 {
                if let Some(mut b) = self
                    .ui_world
                    .world
                    .get_mut::<Border>(self.stress_buttons[i])
                {
                    b.color.x = primary.x;
                    b.color.y = primary.y;
                    b.color.z = primary.z;
                }
                if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(self.stress_buttons[i]) {
                    g.color.x = primary.x;
                    g.color.y = primary.y;
                    g.color.z = primary.z;
                }
                if let Some(mut t) = self
                    .ui_world
                    .world
                    .get_mut::<Text>(self.stress_button_labels[i])
                {
                    t.color.x = primary.x;
                    t.color.y = primary.y;
                    t.color.z = primary.z;
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
            // `set_sun_icon_image`'s doc) — it must be the fully-opaque one
            // while light (correctly occluding the thin ring underneath) and
            // fade *away* going dark, so its alpha runs inverted from every
            // other overlay above.
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.sun_icon_dark) {
                qs.color.w = 1.0 - p;
            }

            // 8. home_icon_selected/_dark share the home_selected_fade
            // envelope, split by a hard dark_target gate rather than true
            // bilinear (page-selected × theme) compositing — see this fn's
            // doc.
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
            // after" signal advance_nav_icons computes for the logo/home
            // icon, recomputed locally here (each advance_* function in
            // this file independently recomputes its own derived state
            // rather than sharing a field, matching the existing style).
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
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let right_edge = canvas_width / 2.0 - THEME_ICON_MARGIN_PX;
            let moon_x = right_edge - THEME_ICON_SIZE_PX / 2.0;
            let sun_x = moon_x - THEME_ICON_SIZE_PX - THEME_ICON_GAP_PX;
            let y = canvas_height / 2.0 - THEME_ICON_MARGIN_PX - THEME_ICON_SIZE_PX / 2.0;
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

            // 11. Hover glow/scale for whichever icon is currently active;
            // force the inactive one back to rest, in case it was
            // mid-hover-animation the instant it flipped inactive.
            let active_icon = if self.dark_target {
                self.sun_icon
            } else {
                self.moon_icon
            };
            // Recomputed from `HoveredEntity` (ground truth for "what's under
            // the pointer right now") rather than this frame's enter/exit
            // events — `active_icon`'s *identity* can flip the instant a
            // click lands (step 3, above), and the newly-active icon won't
            // necessarily get its own hover_entered/hover_exited event on
            // that same frame (the pointer hasn't moved, it's still sitting
            // over the icon that was just clicked, which is now the
            // *inactive* one). Trusting only this frame's event vecs left
            // `theme_icon_is_hovering` stuck at whichever value the old
            // active icon last had — showing a glow on the new active icon
            // until the pointer physically entered and left it once.
            // Comparing directly against the live cursor target is correct
            // regardless of which icon just became active.
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
        /// everything else is click-driven. `dt` drives the splash hold
        /// countdown and the manual tile↔screen fade timers.
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
                // (`intro_elapsed >= INTRO_DURATION`), so "2 seconds to
                // register it" is measured from when the composite is
                // actually done animating in, not from when the whole demo
                // started.
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
                    } else if clicked.contains(&self.nav_buttons[2]) {
                        self.begin_transition(AppState::Home, AppState::ExamplesHome);
                    }
                }

                // Click a tile to morph into the video screen, or click home
                // to converge back into the nav buttons.
                AppState::VideoTiles => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::VideoTiles, AppState::Home);
                    } else if let Some(clicked_idx) =
                        self.tiles.iter().position(|e| clicked.contains(e))
                    {
                        self.begin_transition(
                            AppState::VideoTiles,
                            AppState::VideoScreen(clicked_idx),
                        );
                    }
                }

                // No longer interactive itself — click back to morph into
                // the tile grid, or home to morph straight to the nav
                // buttons; both are 1→N Slice splits (see
                // `start_screen_to_tiles`/`start_screen_to_nav`), so the flat
                // single-tile shape is never shown on the way to either.
                // Both stop playback immediately (in `begin_transition`)
                // rather than carrying it through the morph — a Slice
                // transition bakes a frozen frame of whatever the source
                // looks like at setup time, so there's no "live" video to
                // crossfade through in the first place.
                AppState::VideoScreen(screen_idx) => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::VideoScreen(screen_idx), AppState::Home);
                    } else if clicked.contains(&self.nav_icons[1]) {
                        self.begin_transition(
                            AppState::VideoScreen(screen_idx),
                            AppState::VideoTiles,
                        );
                    }
                }

                // Home icon is the escape hatch if the fetch is
                // slow/erroring; otherwise auto-advance to Gallery once
                // every tile has baked *this* fetch's image (checking
                // generation as well as `BakedImage` presence, since a tile
                // whose own fetch hasn't resolved yet still carries the
                // previous fetch's `BakedImage` — see `set_gallery_image`)
                // *and* the loading animation has played through every
                // frame at least once — a fast fetch (typical, well under
                // one loop) would otherwise cut the spinner off mid-cycle,
                // reading as a glitch rather than a deliberate loading
                // beat. `logo_frames.len()` (not a hardcoded frame count)
                // so this stays correct if the frame set ever changes size.
                AppState::Loading => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::Loading, AppState::Home);
                    } else if self.gallery_fetch_elapsed
                        >= self.logo_frames.len() as f32 * LOGO_FRAME_DURATION
                        && self.gallery_tiles.iter().enumerate().all(|(i, &e)| {
                            self.gallery_tile_fetch_generation[i] == self.gallery_fetch_generation
                                && self.ui_world.world.get::<BakedImage>(e).is_some()
                        })
                    {
                        self.begin_transition(AppState::Loading, AppState::Gallery);
                    }
                }

                AppState::Gallery => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::Gallery, AppState::Home);
                    } else if clicked.contains(&self.gallery_fetch_button) {
                        self.begin_transition(AppState::Gallery, AppState::Loading);
                    } else if let Some(idx) =
                        (0..12).find(|&i| clicked.contains(&self.gallery_tiles[i]))
                    {
                        self.begin_transition(AppState::Gallery, AppState::GalleryImage(idx));
                    }
                }

                AppState::GalleryImage(idx) => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::GalleryImage(idx), AppState::Home);
                    } else if clicked.contains(&self.gallery_enlarged_base) {
                        self.begin_transition(AppState::GalleryImage(idx), AppState::Gallery);
                    }
                }

                AppState::ExamplesHome => {
                    if clicked.contains(&self.nav_icons[0]) {
                        self.begin_transition(AppState::ExamplesHome, AppState::Home);
                    } else if let Some(idx) =
                        (0..6).find(|&i| clicked.contains(&self.example_buttons[i]))
                    {
                        self.begin_transition(AppState::ExamplesHome, AppState::ExampleDetail(idx));
                    }
                }

                AppState::ExampleDetail(idx) => {
                    if clicked.contains(&self.nav_icons[0]) {
                        if idx == 3 {
                            self.cancel_stress_test();
                        }
                        self.begin_transition(AppState::ExampleDetail(idx), AppState::Home);
                    } else if clicked.contains(&self.nav_icons[1]) {
                        if idx == 3 {
                            self.cancel_stress_test();
                        }
                        self.begin_transition(AppState::ExampleDetail(idx), AppState::ExamplesHome);
                    } else if idx == 3 && clicked.contains(&self.stress_buttons[0]) {
                        self.run_burst_spawn();
                    } else if idx == 3 && clicked.contains(&self.stress_buttons[1]) {
                        self.run_texture_churn();
                    }
                }
            }
        }

        /// Starts whatever entity setup a given `(from, to)` pair needs, then
        /// records it as the in-flight `Transition`. Every arm here dispatches
        /// to the matching `start_*` function's `TransitionRequest`/
        /// `OneToNRequest` setup — adding a new reachable state only means
        /// adding one arm here (and its mirror in `drive_transition`/
        /// `settle`).
        fn begin_transition(&mut self, from: AppState, to: AppState) {
            match (from, to) {
                (AppState::Splash, AppState::Home) => self.start_splash_to_nav(),
                (AppState::Home, AppState::VideoTiles) => self.start_nav_to_tiles(),
                (AppState::VideoTiles, AppState::Home) => self.start_tiles_to_nav(),
                (AppState::VideoTiles, AppState::VideoScreen(idx)) => {
                    self.start_tiles_to_screen(idx);
                    // Playback starts the instant the tile is clicked, not
                    // when the morph finishes — it plays underneath the
                    // morph. JS polls `take_video_start_tile` once per tick
                    // and, once the browser's <video> element has loaded
                    // metadata, calls back into `start_video`.
                    self.pending_video_start = Some(idx as u32);
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
                (AppState::Gallery, AppState::Loading) => self.start_gallery_to_loading(),
                (AppState::Gallery, AppState::GalleryImage(idx)) => {
                    self.start_gallery_to_image(idx)
                }
                (AppState::GalleryImage(_), AppState::Gallery) => self.start_image_to_gallery(),
                (AppState::GalleryImage(_), AppState::Home) => self.start_image_to_home(),
                (AppState::Home, AppState::ExamplesHome) => self.start_home_to_examples(),
                (AppState::ExamplesHome, AppState::Home) => self.start_examples_to_home(),
                (AppState::ExamplesHome, AppState::ExampleDetail(idx)) => {
                    self.start_examples_to_detail(idx)
                }
                (AppState::ExampleDetail(_), AppState::ExamplesHome) => {
                    self.start_detail_to_examples()
                }
                (AppState::ExampleDetail(_), AppState::Home) => self.start_detail_to_home(),
                (from, to) => unreachable!("no transition defined for {from:?} -> {to:?}"),
            }
            self.transition = Some(Transition {
                from,
                to,
                elapsed: 0.0,
            });
        }

        /// Ticks a transition's manual fades and reports whether it has
        /// completed. Mirrors `begin_transition`'s `(from, to)` dispatch,
        /// driving the same fade function and completion condition the old
        /// per-phase code used for each pair.
        fn drive_transition(&mut self, t: &Transition) -> bool {
            match (t.from, t.to) {
                (AppState::Splash, AppState::Home) => self.nav_buttons.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::Home, AppState::VideoTiles) => self.tiles.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                // Both `VideoTiles`'s and `VideoScreen`'s "home" click land
                // here — `start_tiles_to_nav` fires one 1→1 crossfade per
                // tile (tile i onto nav_buttons[i]) while `start_screen_to_nav`
                // fires one 1→3 split (the screen tile onto all three
                // buttons), but both are framework-driven `OneToNRequest`s
                // that reveal their own targets on completion — no manual
                // fade needed either way, just wait for every button to come
                // back.
                (AppState::VideoTiles, AppState::Home)
                | (AppState::VideoScreen(_), AppState::Home) => self.nav_buttons.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::VideoTiles, AppState::VideoScreen(idx)) => {
                    self.advance_tiles_to_screen_fade(idx, t.elapsed);
                    let lifecycle = self.ui_world.world.get::<Lifecycle>(self.tiles[idx]);
                    matches!(lifecycle, Some(Lifecycle::Idle))
                        && t.elapsed >= BUTTON_TILES_MORPH_DURATION
                }
                (AppState::VideoScreen(_), AppState::VideoTiles) => self.tiles.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::Home, AppState::Loading) | (AppState::Gallery, AppState::Loading) => {
                    matches!(self.ui_world.world.get::<Visibility>(self.loading_logo), Some(v) if v.visible)
                }
                (AppState::Loading, AppState::Gallery) => self.gallery_tiles.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::Loading, AppState::Home)
                | (AppState::Gallery, AppState::Home)
                | (AppState::GalleryImage(_), AppState::Home)
                | (AppState::ExamplesHome, AppState::Home)
                | (AppState::ExampleDetail(_), AppState::Home) => self.nav_buttons.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::Gallery, AppState::GalleryImage(_)) => {
                    matches!(self.ui_world.world.get::<Visibility>(self.gallery_enlarged_base), Some(v) if v.visible)
                }
                (AppState::GalleryImage(_), AppState::Gallery) => self.gallery_tiles.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                ),
                (AppState::Home, AppState::ExamplesHome)
                | (AppState::ExampleDetail(_), AppState::ExamplesHome) => {
                    self.example_buttons.iter().all(|&e| {
                        matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)
                    })
                }
                (AppState::ExamplesHome, AppState::ExampleDetail(_)) => {
                    matches!(self.ui_world.world.get::<Visibility>(self.example_detail_panel), Some(v) if v.visible)
                }
                (from, to) => unreachable!("no transition defined for {from:?} -> {to:?}"),
            }
        }

        /// Unconditionally forces every entity this demo touches into
        /// `state`'s correct resting configuration, regardless of which state
        /// it just transitioned from. A `(from, to)` match table alone can't
        /// guarantee this: each arm only ever changes what its own transition
        /// touches, so anything an earlier visit left dirty — stale geometry
        /// a group transition never wrote back (see `settle_tile_geometry`'s
        /// doc), a manual fade that zeroed alpha but never hid the entity —
        /// stays dirty until something re-asserts the whole picture on
        /// arrival. `settle` is that something, called once per landing after
        /// the transition's own animation has finished.
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
                    for i in 0..6 {
                        self.settle_example_button_idle(i, false);
                    }
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
                    // No bulk bake-clear here — a stale tile's `BakedImage`/
                    // `TextureRef` is cleared individually, right as its own
                    // new image arrives (`set_gallery_image`), not as a
                    // batch when this state settles. Clearing all 12 up
                    // front here used to race the fetch: a fast response
                    // could land, bake, and get immediately wiped out again
                    // the moment this arm next ran, before the "all 12
                    // loaded" check ever saw it.
                    for i in 0..12 {
                        self.settle_gallery_tile(i, false);
                    }
                    if let Some(mut vis) =
                        self.ui_world.world.get_mut::<Visibility>(self.loading_logo)
                    {
                        vis.visible = true;
                    }
                    self.hide_gallery_error();
                }

                AppState::Gallery => {
                    self.set_nav_buttons_visible(false);
                    if let Some(mut vis) =
                        self.ui_world.world.get_mut::<Visibility>(self.loading_logo)
                    {
                        vis.visible = false;
                    }
                    for i in 0..12 {
                        self.settle_gallery_tile(i, true);
                    }
                }

                AppState::GalleryImage(image_idx) => {
                    self.set_nav_buttons_visible(false);
                    for i in 0..12 {
                        self.settle_gallery_tile(i, false);
                    }
                    self.settle_gallery_enlarged_base(image_idx);
                }

                AppState::ExamplesHome => {
                    self.set_nav_buttons_visible(false);
                    for i in 0..6 {
                        self.settle_example_button_idle(i, true);
                    }
                }

                AppState::ExampleDetail(category_idx) => {
                    self.set_nav_buttons_visible(false);
                    // All 6 stay hidden — `example_detail_panel`, not any
                    // of these buttons, is what's on screen now.
                    for i in 0..6 {
                        self.settle_example_button_idle(i, false);
                    }
                    self.settle_example_detail_panel(category_idx);
                }
            }
        }

        /// Rust-side + JS-side cleanup for whatever video is currently
        /// playing, unconditionally — a no-op (on both sides) if nothing is.
        /// Combines `stop_video` (removes `VideoPlayer`/frees the GPU
        /// texture) with flagging `pending_video_stop` so JS's next
        /// `take_video_stop` poll pauses the `<video>` element too.
        fn stop_video_playback(&mut self) {
            self.stop_video();
            self.pending_video_stop = true;
        }

        /// Forces all three nav buttons' `Visibility` and, when shown, their
        /// full idle appearance (transparent fill per the Design System spec,
        /// opaque border/glow/label) — the resting picture for
        /// `AppState::Home` from any `from`, whether this is the first-ever
        /// `Splash → Home` reveal or a return trip whose crossfade left
        /// alpha/border/glow at some intermediate value.
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
        /// (`AppState::Home`, or any other tile while one is the video
        /// screen) or visible (`AppState::VideoTiles`) — hover reset to idle
        /// either way, so a hover ramp frozen mid-transition never lingers
        /// into the next scene.
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
        /// video screen — recomputed from the current canvas size rather than
        /// trusting wherever the `TransitionRequest` left it lerped to, so a
        /// resize mid-morph can't leave it slightly off.
        fn settle_tile_screen(&mut self, i: usize) {
            let tile = self.tiles[i];
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(tile) {
                vis.visible = true;
            }
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let screen = video_screen_quad(canvas_width, canvas_height);
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

        /// Resets `tiles[i]`'s own `QuadState` back to its grid position/
        /// size/color. Group transitions never write this back onto the real
        /// entity themselves (only the virtual slices get the destination
        /// shape — see `layout_nav_buttons`'s doc), and the tile↔screen
        /// `TransitionRequest` morph leaves it wherever it lerped to — so
        /// without an unconditional reset on every arrival, a tile that ever
        /// became the video screen would carry that geometry into its next
        /// appearance as a grid tile (this was the "full-screen Jellyfish"
        /// bug: `start_screen_to_nav` collapsed a screen-shaped tile straight
        /// into the nav button without ever passing back through this reset).
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
        /// Computes each nav button's final centered-as-a-group layout from
        /// its label's baked width (falls back to `NAV_BUTTON_FALLBACK_SIZE`
        /// if a label somehow hasn't baked yet), writes it onto the button's
        /// own `QuadState` (so it matches once the Slice transition reveals
        /// it — group transitions never write the target's own `QuadState`,
        /// only the virtual slices), and returns the resulting states for use
        /// as `GroupTarget`s.
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

        /// Lays out `example_buttons` in a 3 col × 2 row grid, centered on
        /// both axes (unlike the bottom-anchored gallery grid) — the
        /// resting shape for `AppState::ExamplesHome`. Same "size from each
        /// label's baked width" idiom as `layout_nav_buttons`, just 2D;
        /// column-major indexed (`i = col*2 + row`) to match
        /// `example_buttons`'s doc.
        fn layout_example_buttons(&mut self) -> [QuadState; 6] {
            let sizes: [Vec2; 6] = std::array::from_fn(|i| {
                self.ui_world
                    .world
                    .get::<BakedText>(self.example_labels[i])
                    .map(|b| {
                        Vec2::new(b.pixel_size[0], b.pixel_size[1])
                            + Vec2::splat(2.0 * NAV_BUTTON_PADDING_PX)
                    })
                    .unwrap_or(NAV_BUTTON_FALLBACK_SIZE)
            });
            let col_width = |col: usize| sizes[col * 2].x.max(sizes[col * 2 + 1].x);
            let widths = [col_width(0), col_width(1), col_width(2)];
            let total_width: f32 = widths.iter().sum::<f32>() + 2.0 * EXAMPLE_BUTTON_COL_GAP_PX;
            let row_height = sizes.iter().map(|s| s.y).fold(0.0, f32::max);
            let total_height = 2.0 * row_height + EXAMPLE_BUTTON_ROW_GAP_PX;
            let mut col_centers = [0.0f32; 3];
            let mut x = -total_width / 2.0;
            for c in 0..3 {
                col_centers[c] = x + widths[c] / 2.0;
                x += widths[c] + EXAMPLE_BUTTON_COL_GAP_PX;
            }
            let top_y = total_height / 2.0 - row_height / 2.0;
            let states: [QuadState; 6] = std::array::from_fn(|i| {
                let col = i / 2;
                let y = if i % 2 == 0 { top_y } else { -top_y };
                QuadState {
                    position: Vec3::new(col_centers[col], y, 0.5),
                    // Both buttons in a column share the column's width
                    // (its wider label's own size) rather than each sizing
                    // to its own label — e.g. "3D" matches "Layout"'s width.
                    size: Vec2::new(widths[col], row_height),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    corner_radius: NAV_BUTTON_CORNER_RADIUS,
                }
            });
            for (i, state) in states.iter().enumerate() {
                if let Some(mut qs) = self
                    .ui_world
                    .world
                    .get_mut::<QuadState>(self.example_buttons[i])
                {
                    *qs = state.clone();
                }
            }
            states
        }

        /// Positions the Effects screen's content (`example_content`'s doc
        /// for why the rows are `ChildOf` `example_detail_panel` while the
        /// title isn't), relative to `example_detail_panel`'s current
        /// position/size (window-dependent, so this must run after that
        /// geometry is set — called from `settle_example_detail_panel`):
        /// the title renders *above* the panel in absolute world
        /// coordinates (outside its bounds, `EXAMPLE_HEADING_PANEL_GAP_PX`
        /// above its top edge), left-aligned to the panel's own left edge.
        /// Each row's label/boxes are positioned in *local*, panel-relative
        /// coordinates instead (since they're children of the panel,
        /// `panel_position` doesn't factor in — only `panel_size`),
        /// indented 10px further than the title, boxes flowing right
        /// starting just past that row's own label (read from its baked
        /// width). Text entities render centered on their own position
        /// (anchor 0.5,0.5), so left-aligning means shifting each one
        /// right by half its own baked width.
        fn layout_effects_content(&mut self, panel_position: Vec3, panel_size: Vec2) {
            const ROW_INDENT_EXTRA: f32 = 10.0;
            const ROW_GAP_Y: f32 = 90.0;
            const LABEL_BOX_GAP: f32 = 24.0;
            const BOX_GAP: f32 = 30.0;
            const BOX_SIZE: f32 = 64.0;
            // Gap from a box's bottom edge to its caption underneath — 3px
            // more than the original (too-tight) spacing.
            const OPACITY_LABEL_GAP: f32 = 18.0;

            // Title: absolute world coordinates (independent entity, not a
            // panel child).
            let abs_left_x = panel_position.x - panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let abs_panel_top_y = panel_position.y + panel_size.y / 2.0;

            let title_height = self
                .ui_world
                .world
                .get::<BakedText>(self.effects_title)
                .map(|b| b.pixel_size[1])
                .unwrap_or(0.0);
            let title_width = self
                .ui_world
                .world
                .get::<BakedText>(self.effects_title)
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.effects_title) {
                qs.position.x = abs_left_x + title_width / 2.0;
                qs.position.y = abs_panel_top_y + EXAMPLE_HEADING_PANEL_GAP_PX + title_height / 2.0;
            }

            // Rows: local, panel-relative coordinates (panel center =
            // origin) — these entities are `ChildOf` the panel, so
            // `panel_position` must NOT be added in here.
            let left_x = -panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let row_left_x = left_x + ROW_INDENT_EXTRA;
            let panel_top_y = panel_size.y / 2.0;

            let top_y = panel_top_y - EXAMPLE_PANEL_PADDING_PX - BOX_SIZE / 2.0;
            let row_y = [
                top_y,
                top_y - ROW_GAP_Y,
                top_y - 2.0 * ROW_GAP_Y,
                top_y - 3.0 * ROW_GAP_Y,
            ];

            // All 4 rows' boxes share one column start — the Drop Shadow
            // row's own label (the widest of the four), not each row's own
            // label — so every row's leftmost box lines up on the same
            // left edge instead of drifting with each row's label width.
            let boxes_start_x = {
                let drop_shadow_label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(self.effects_row_labels[0])
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                row_left_x + drop_shadow_label_width + LABEL_BOX_GAP
            };

            for (row, &y) in row_y.iter().enumerate() {
                let label_entity = self.effects_row_labels[row];
                let label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(label_entity)
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(label_entity) {
                    qs.position.x = row_left_x + label_width / 2.0;
                    qs.position.y = y;
                }
                for i in 0..self.effects_row_boxes[row].len() {
                    let box_entity = self.effects_row_boxes[row][i];
                    let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
                    if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(box_entity) {
                        qs.position.x = x;
                        qs.position.y = y;
                    }
                    // Opacity row (3) — also place this item's own caption
                    // just below it.
                    if row == 3 {
                        let caption = self.effects_opacity_item_labels[i];
                        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(caption) {
                            qs.position.x = x;
                            qs.position.y = y - BOX_SIZE / 2.0 - OPACITY_LABEL_GAP;
                        }
                    }
                }
            }
        }

        /// Positions the Text screen's content — same title/row split as
        /// `layout_effects_content` (title absolute, rows local to the
        /// panel), just with items flowed by their own *actual* baked
        /// width instead of a fixed box grid, since font size/letter
        /// spacing directly change how wide each sample renders.
        fn layout_text_content(&mut self, panel_position: Vec3, panel_size: Vec2) {
            const ROW_INDENT_EXTRA: f32 = 10.0;
            const ROW_GAP_Y: f32 = 90.0;
            const LABEL_ITEM_GAP: f32 = 24.0;
            const ITEM_GAP: f32 = 30.0;
            const ROW_HEIGHT: f32 = 56.0;

            // Title: absolute world coordinates (independent entity, not a
            // panel child) — same treatment as `layout_effects_content`.
            let abs_left_x = panel_position.x - panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let abs_panel_top_y = panel_position.y + panel_size.y / 2.0;

            let title_height = self
                .ui_world
                .world
                .get::<BakedText>(self.text_title)
                .map(|b| b.pixel_size[1])
                .unwrap_or(0.0);
            let title_width = self
                .ui_world
                .world
                .get::<BakedText>(self.text_title)
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.text_title) {
                qs.position.x = abs_left_x + title_width / 2.0;
                qs.position.y = abs_panel_top_y + EXAMPLE_HEADING_PANEL_GAP_PX + title_height / 2.0;
            }

            // Rows: local, panel-relative coordinates (panel center =
            // origin).
            let left_x = -panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let row_left_x = left_x + ROW_INDENT_EXTRA;
            let panel_top_y = panel_size.y / 2.0;

            let top_y = panel_top_y - EXAMPLE_PANEL_PADDING_PX - ROW_HEIGHT / 2.0;
            let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

            // All 3 rows' items share one column start — "Letter Spacing"
            // (row 2), the widest of the three labels, not each row's own
            // label — so every row's leftmost item lines up on the same
            // left edge instead of drifting with each row's label width.
            let items_start_x = {
                let letter_spacing_label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(self.text_row_labels[2])
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                row_left_x + letter_spacing_label_width + LABEL_ITEM_GAP
            };

            for (row, &y) in row_y.iter().enumerate() {
                let label_entity = self.text_row_labels[row];
                let label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(label_entity)
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(label_entity) {
                    qs.position.x = row_left_x + label_width / 2.0;
                    qs.position.y = y;
                }
                let mut running_x = items_start_x;
                for i in 0..self.text_row_items[row].len() {
                    let item_entity = self.text_row_items[row][i];
                    let item_width = self
                        .ui_world
                        .world
                        .get::<BakedText>(item_entity)
                        .map(|b| b.pixel_size[0])
                        .unwrap_or(0.0);
                    if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(item_entity) {
                        qs.position.x = running_x + item_width / 2.0;
                        qs.position.y = y;
                    }
                    running_x += item_width + ITEM_GAP;
                }
            }
        }

        /// Positions the Transforms & Animation screen's content — same
        /// title/row split as `layout_effects_content` (title absolute,
        /// rows local to the panel), using the same fixed box-grid layout
        /// as Effects' rows. Rotation/Scale rows also place each box's
        /// own caption underneath it (mirroring Effects' Opacity row);
        /// Continuous Animation has none (one box, already described by
        /// its own row label).
        fn layout_transforms_content(&mut self, panel_position: Vec3, panel_size: Vec2) {
            const ROW_INDENT_EXTRA: f32 = 10.0;
            // Wider than every other screen's row gap (90) — this
            // screen's boxes rotate/scale, so they need more room to
            // breathe between rows than a row of static content does.
            const ROW_GAP_Y: f32 = 120.0;
            const LABEL_BOX_GAP: f32 = 29.0;
            const BOX_GAP: f32 = 40.0;
            const BOX_SIZE: f32 = 64.0;
            const ITEM_LABEL_GAP: f32 = 21.0;
            // Extra clearance beyond `EXAMPLE_PANEL_PADDING_PX`, top and
            // bottom, specific to this screen: a rotated box's diagonal
            // extends past its own bounding box, and the Scale/Continuous
            // rows scale boxes up to 1.4×/1.2× — both eat into the
            // standard padding enough to visibly clip against the panel
            // edge without this.
            const EXTRA_VERTICAL_PADDING: f32 = 20.0;

            // Title: absolute world coordinates (independent entity, not
            // a panel child) — same treatment as `layout_effects_content`.
            let abs_left_x = panel_position.x - panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let abs_panel_top_y = panel_position.y + panel_size.y / 2.0;

            let title_height = self
                .ui_world
                .world
                .get::<BakedText>(self.transforms_title)
                .map(|b| b.pixel_size[1])
                .unwrap_or(0.0);
            let title_width = self
                .ui_world
                .world
                .get::<BakedText>(self.transforms_title)
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.transforms_title)
            {
                qs.position.x = abs_left_x + title_width / 2.0;
                qs.position.y = abs_panel_top_y + EXAMPLE_HEADING_PANEL_GAP_PX + title_height / 2.0;
            }

            // Rows: local, panel-relative coordinates (panel center =
            // origin).
            let left_x = -panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let row_left_x = left_x + ROW_INDENT_EXTRA;
            let panel_top_y = panel_size.y / 2.0;

            let top_y =
                panel_top_y - EXAMPLE_PANEL_PADDING_PX - EXTRA_VERTICAL_PADDING - BOX_SIZE / 2.0;
            let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

            // All 3 rows' boxes share one column start — "Continuous
            // Animation" (row 2), the widest of the three labels, not
            // each row's own label — so every row's leftmost box lines up
            // on the same left edge instead of drifting with each row's
            // label width.
            let boxes_start_x = {
                let continuous_label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(self.transforms_row_labels[2])
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                row_left_x + continuous_label_width + LABEL_BOX_GAP
            };

            for (row, &y) in row_y.iter().enumerate() {
                let label_entity = self.transforms_row_labels[row];
                let label_width = self
                    .ui_world
                    .world
                    .get::<BakedText>(label_entity)
                    .map(|b| b.pixel_size[0])
                    .unwrap_or(0.0);
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(label_entity) {
                    qs.position.x = row_left_x + label_width / 2.0;
                    qs.position.y = y;
                }
                for i in 0..self.transforms_row_boxes[row].len() {
                    let box_entity = self.transforms_row_boxes[row][i];
                    let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
                    if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(box_entity) {
                        qs.position.x = x;
                        qs.position.y = y;
                    }
                    if let Some(&caption) = self.transforms_item_labels[row].get(i) {
                        if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(caption) {
                            qs.position.x = x;
                            qs.position.y = y - BOX_SIZE / 2.0 - ITEM_LABEL_GAP;
                        }
                    }
                }
            }
        }

        /// Positions the Stress Tests screen's content. Stacking order,
        /// top to bottom: title, then the two buttons, then the panel —
        /// mirroring `stress_panel_target`'s own derivation of the
        /// panel's top edge, which stacks the *same* three gaps/heights
        /// in the same order to work out how far down the panel has to
        /// start. Title and buttons are both absolute (independent
        /// entities, not `ChildOf` the panel — see `stress_buttons`'s
        /// doc), left-aligned to the panel's own left edge, using the
        /// same gap (`EXAMPLE_HEADING_PANEL_GAP_PX`) between
        /// title/buttons and buttons/panel. Each button is sized to its
        /// own baked label width (see `STRESS_BUTTON_HEIGHT_PX`'s doc for
        /// why a shared fixed width didn't work) — the second button's x
        /// depends on the first's actual width, so `stress_buttons[0]`
        /// must be sized before `stress_buttons[1]`'s x is computed. The
        /// result line stays `ChildOf` the panel, centered horizontally
        /// (local x=0 — its own anchor already centers it regardless of
        /// baked width) near the panel's bottom edge, within
        /// `RESULT_TEXT_RESERVED_HEIGHT_PX` of it — `random_burst_target`
        /// reserves that same strip so Burst Spawn's quads never render
        /// over it.
        fn layout_stress_content(&mut self, panel_position: Vec3, panel_size: Vec2) {
            const BUTTON_GAP: f32 = 24.0;

            let abs_left_x = panel_position.x - panel_size.x / 2.0 + EXAMPLE_PANEL_PADDING_PX;
            let panel_top_edge = panel_position.y + panel_size.y / 2.0;

            // Buttons: directly above the panel, side by side,
            // left-aligned as a pair, each sized to its own label.
            let burst_label_width = self
                .ui_world
                .world
                .get::<BakedText>(self.stress_button_labels[0])
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            let churn_label_width = self
                .ui_world
                .world
                .get::<BakedText>(self.stress_button_labels[1])
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            let burst_width = burst_label_width + STRESS_BUTTON_HORIZONTAL_PADDING_PX;
            let churn_width = churn_label_width + STRESS_BUTTON_HORIZONTAL_PADDING_PX;

            let buttons_y =
                panel_top_edge + EXAMPLE_HEADING_PANEL_GAP_PX + STRESS_BUTTON_HEIGHT_PX / 2.0;
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.stress_buttons[0])
            {
                qs.size.x = burst_width;
                qs.position.x = abs_left_x + burst_width / 2.0;
                qs.position.y = buttons_y;
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.stress_buttons[1])
            {
                qs.size.x = churn_width;
                qs.position.x = abs_left_x + burst_width + BUTTON_GAP + churn_width / 2.0;
                qs.position.y = buttons_y;
            }

            // Title: above the buttons.
            let title_height = self
                .ui_world
                .world
                .get::<BakedText>(self.stress_title)
                .map(|b| b.pixel_size[1])
                .unwrap_or(0.0);
            let title_width = self
                .ui_world
                .world
                .get::<BakedText>(self.stress_title)
                .map(|b| b.pixel_size[0])
                .unwrap_or(0.0);
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.stress_title) {
                qs.position.x = abs_left_x + title_width / 2.0;
                qs.position.y = buttons_y
                    + STRESS_BUTTON_HEIGHT_PX / 2.0
                    + EXAMPLE_HEADING_PANEL_GAP_PX
                    + title_height / 2.0;
            }

            // Result: local, panel-relative — stays inside the panel,
            // near its bottom edge.
            let result_y = -panel_size.y / 2.0 + RESULT_TEXT_RESERVED_HEIGHT_PX / 2.0;
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.stress_result_text)
            {
                qs.position.x = 0.0;
                qs.position.y = result_y;
            }

            // Warning: dead center of the panel — absolute world
            // coordinates (independent entity, not a panel child; see
            // its own spawn-site doc for why).
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.stress_warning_text)
            {
                qs.position.x = panel_position.x;
                qs.position.y = panel_position.y;
            }
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

        /// 1→1 Slice ×3, reverse of `start_tiles_to_nav`: each nav button
        /// morphs directly onto the tile closest to it. "Closest" is always
        /// same-index here — see `start_tiles_to_nav`'s doc for why — so
        /// button 0 goes to tile 0, button 1 to tile 1, and so on. Each
        /// button gets its own single-target `OneToNRequest` (three
        /// independent requests, not one 3-target group), the same
        /// degenerate single-crossfade shape `start_tiles_to_nav` uses in
        /// reverse.
        fn start_nav_to_tiles(&mut self) {
            for i in 0..3 {
                let mut state = tile_quad(i, self.theme_progress);
                // Same placeholder-tint override as the old splash→tiles
                // morph.
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

        /// 1→1 Slice ×3, reverse of `start_tiles_to_screen`'s sibling: each
        /// tile morphs directly onto the nav button closest to it. "Closest"
        /// is always same-index here — `layout_nav_buttons` and `tile_quad`
        /// both lay their three entities out left-to-right in index order,
        /// so tile 0 is nearest button 0, tile 1 nearest button 1, and so on;
        /// no distance computation needed. Each tile gets its own
        /// single-target `OneToNRequest` (three independent requests, not
        /// one 3-source group) — with exactly one target this degenerates to
        /// a single whole-shape baked crossfade rather than an actual slice,
        /// the same way a single-source group transition does elsewhere (see
        /// `start_screen_to_nav`'s old single-target form, now generalized
        /// to all three). Each button's own `QuadState` still holds the
        /// layout `layout_nav_buttons` wrote the first time it ran — nothing
        /// has touched it since (transitions only ever move virtual
        /// entities, not the real target) — so reading it back here is
        /// already the correct destination shape.
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
        /// (currently screen-shaped) splits into all three nav buttons at
        /// once, the same "one shape fans out into three" motion as the
        /// original button→tiles split, just sourced from the screen tile
        /// instead of the button and run in reverse. The flat tile grid is
        /// never shown.
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
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let mut to = video_screen_quad(canvas_width, canvas_height);
            // `video_screen_quad` always bakes in the *light* screen radius
            // — override to the current theme's, so this transition's own
            // eased tick (which `advance_theme` now steps aside for while
            // it's in flight, see its corner radius doc) heads toward the
            // theme-correct value instead of always light.
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

        /// Stops whatever video is currently playing: removes `VideoPlayer`
        /// from its tile and releases the video texture's GPU memory.
        /// Rust-side cleanup only — `take_video_stop` separately tells JS to
        /// pause the actual `<video>` element. A no-op if nothing is playing.
        fn stop_video(&mut self) {
            let Some(playing) = self.playing_video.take() else {
                return;
            };
            self.ui_world
                .world
                .entity_mut(self.tiles[playing.tile_idx])
                .remove::<(VideoPlayer, VideoCrossfade)>();
            self.ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .suspend_video(&self.device, playing.texture_id);
        }

        /// 1→N Slice: `tiles[screen_idx]` (currently screen-shaped) splits
        /// into all three tiles at once — the same "one shape fans out into
        /// three" motion as `start_screen_to_nav`, just landing back on the
        /// tile grid instead of the nav buttons. `tiles[screen_idx]` is both
        /// the source *and* one of the three targets here (it's returning to
        /// its own grid slot) — that's fine: a group transition never writes
        /// a target's own `QuadState` regardless of whether it's also the
        /// source, so `settle(VideoTiles)` (which unconditionally resets
        /// every tile's geometry on arrival) is exactly what makes this
        /// correct rather than anything special-cased here.
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

        // -------------------------------------------------------------------
        // Examples & Tests — Home ↔ ExamplesHome ↔ ExampleDetail (M11.3)
        // -------------------------------------------------------------------

        /// 3 simultaneous 1→2 `OneToNRequest`s (`GridSlice{cols:1,rows:2}`),
        /// one per nav button — the reverse of `start_examples_to_home`,
        /// and the same "M→N via composed 1↔N primitives" idiom
        /// `start_gallery_to_home` established. Column `i`
        /// (`nav_buttons[i]`) splits into `example_buttons[i*2]` (top) and
        /// `example_buttons[i*2+1]` (bottom) — the home row `[0,1,2]`
        /// becomes grid `[a,b,c]/[d,e,f]` as `0→(a,d)`, `1→(b,e)`,
        /// `2→(c,f)`, each button's column landing directly at/below itself
        /// spatially. All 3 fire in the same frame, so it reads as one
        /// continuous 3→6 fan-out, not 3 separate moments.
        fn start_home_to_examples(&mut self) {
            let states = self.layout_example_buttons();
            for col in 0..3 {
                let targets = (0..2)
                    .map(|row| {
                        let i = col * 2 + row;
                        GroupTarget {
                            entity: self.example_buttons[i],
                            state: states[i].clone(),
                        }
                    })
                    .collect();
                self.ui_world
                    .world
                    .entity_mut(self.nav_buttons[col])
                    .insert(OneToNRequest {
                        targets,
                        default_config: TransitionConfig {
                            duration: BUTTON_TILES_MORPH_DURATION,
                            delay: 0.0,
                            easing: ease_in_out_quad,
                        },
                        child_behavior: None,
                        strategy: SplitStrategy::GridSlice { cols: 1, rows: 2 },
                    });
            }
        }

        /// Reverse of `start_home_to_examples`: 3 simultaneous 2→1
        /// `NToOneRequest`s (`MergeLayout::Grid{cols:1,rows:2}`), each
        /// column's top+bottom pair merging back into its matching
        /// `nav_buttons[i]` — mirrors `start_gallery_to_home`'s per-column
        /// merges exactly.
        fn start_examples_to_home(&mut self) {
            for col in 0..3 {
                let sources = (0..2)
                    .map(|row| {
                        let i = col * 2 + row;
                        let entity = self.example_buttons[i];
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
                    .entity_mut(self.nav_buttons[col])
                    .insert(NToOneRequest {
                        sources,
                        default_config: TransitionConfig {
                            duration: BUTTON_TILES_MORPH_DURATION,
                            delay: 0.0,
                            easing: ease_in_out_quad,
                        },
                        child_behavior: None,
                        layout: MergeLayout::Grid { cols: 1, rows: 2 },
                    });
            }
        }

        /// N→1 GridSlice: all 6 `example_buttons` converge into
        /// `example_detail_panel` — the same starburst mechanic
        /// `start_gallery_to_image` uses for the gallery grid, just
        /// converging onto a dedicated entity instead of
        /// `gallery_enlarged_base`. Sources are remapped to row-major
        /// order (row 0: all 3 cols, then row 1) to match
        /// `MergeLayout::Grid`'s expectation — `example_buttons` itself is
        /// column-major (`i = col*2 + row`).
        fn start_examples_to_detail(&mut self, idx: usize) {
            // Only category `idx`'s content stays `ChildOf` the panel for
            // this merge's bake — see `set_example_category_active`'s doc
            // for why this has to happen before the bake, not after.
            self.set_example_category_active(idx);

            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;

            let sources: Vec<GroupSource> = (0..2)
                .flat_map(|row| (0..3).map(move |col| col * 2 + row))
                .map(|i| {
                    let entity = self.example_buttons[i];
                    let state = self
                        .ui_world
                        .world
                        .get::<QuadState>(entity)
                        .cloned()
                        .unwrap_or_default();
                    GroupSource { entity, state }
                })
                .collect();

            let dest = self.example_detail_panel;
            // The real, final resting geometry for category `idx` —
            // computed the same way `settle_example_detail_panel` will
            // once the transition completes, so the merge animates
            // straight to the correct spot instead of animating to a
            // generic placeholder and then snapping (a visible
            // "pop"/"drop") once settle overrides it.
            let to = self.example_detail_target(idx, canvas_width, canvas_height);
            let target_position = to.position;
            let target_size = to.size;
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(dest) {
                *qs = to;
            }
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(dest) {
                vis.visible = false;
            }
            // Position content *before* the merge's own bake runs (not
            // just at settle) — otherwise on the very first-ever visit to
            // this category, content still sits at its raw spawn-time
            // (panel-center) offset when the bake captures it, flashing
            // there before jumping to its real position once settle
            // catches up.
            match idx {
                0 => self.layout_effects_content(target_position, target_size),
                1 => self.layout_text_content(target_position, target_size),
                2 => self.layout_transforms_content(target_position, target_size),
                3 => self.layout_stress_content(target_position, target_size),
                _ => {}
            }

            self.ui_world.world.entity_mut(dest).insert(NToOneRequest {
                sources,
                default_config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                layout: MergeLayout::Grid { cols: 3, rows: 2 },
            });
        }

        /// 1→N GridSlice: reverses `start_examples_to_detail` —
        /// `example_detail_panel` splits back into all 6 category buttons.
        /// `GridSlice` (not `Slice`) since the 6 targets form a real 3×2
        /// grid, same reasoning as the gallery's own `GridSlice` choice.
        fn start_detail_to_examples(&mut self) {
            let states = self.layout_example_buttons();
            // GridSlice needs targets in row-major order (row 0: all 3
            // cols, then row 1), but `example_buttons` is column-major
            // (`i = col*2 + row`, matching the category table) — remap
            // here.
            let targets = (0..2)
                .flat_map(|row| (0..3).map(move |col| col * 2 + row))
                .map(|i| GroupTarget {
                    entity: self.example_buttons[i],
                    state: states[i].clone(),
                })
                .collect();
            self.ui_world
                .world
                .entity_mut(self.example_detail_panel)
                .insert(OneToNRequest {
                    targets,
                    default_config: TransitionConfig {
                        duration: BUTTON_TILES_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                    strategy: SplitStrategy::GridSlice { cols: 3, rows: 2 },
                });
        }

        /// 1→N Slice, the escape-hatch path: `example_detail_panel` splits
        /// directly into the 3 nav buttons — mirrors `start_screen_to_nav`.
        fn start_detail_to_home(&mut self) {
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
                .entity_mut(self.example_detail_panel)
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

        /// Forces `example_buttons[i]` back to its resting grid shape,
        /// either hidden (`AppState::Home`) or visible
        /// (`AppState::ExamplesHome`) — hover reset to idle either way,
        /// mirroring `settle_tile_idle`.
        fn settle_example_button_idle(&mut self, i: usize, visible: bool) {
            let btn = self.example_buttons[i];
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(btn) {
                vis.visible = visible;
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(btn) {
                border.color.w = 1.0;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(btn) {
                glow.radius = 0.0;
                glow.color.w = 1.0;
            }
            if let Some(mut label) = self.ui_world.world.get_mut::<Text>(self.example_labels[i]) {
                label.color.w = 1.0;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(btn) {
                qs.scale = 1.0;
            }
            self.example_hover_progress[i] = 0.0;
            for &content in &self.example_content[i] {
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(content) {
                    vis.visible = false;
                }
            }
        }

        /// The `example_detail_panel`'s resting geometry for category
        /// `idx` — shared by `start_examples_to_detail` (so the merge's
        /// own destination target already matches the final settled
        /// position, no end-of-morph pop) and `settle_example_detail_panel`
        /// (the actual resting assert). idx 0/1/2
        /// (Effects/Text/Transforms & Animation) use their own measured
        /// content height, and — since they each have their own heading
        /// rendered above the panel — shift the panel down so
        /// heading+panel together land vertically centered on screen
        /// (see `layout_effects_content`'s doc); 4/5 (Layout/3D
        /// placeholders) just need enough room for one line of text. idx
        /// 3 (Stress Tests) is a genuinely different shape — see
        /// `stress_panel_target`'s doc.
        fn example_detail_target(
            &mut self,
            idx: usize,
            canvas_width: f32,
            canvas_height: f32,
        ) -> QuadState {
            if idx == 3 {
                return self.stress_panel_target(canvas_width, canvas_height);
            }
            let content_height = match idx {
                0 => EFFECTS_CONTENT_HEIGHT_PX,
                1 => TEXT_CONTENT_HEIGHT_PX,
                2 => TRANSFORMS_CONTENT_HEIGHT_PX,
                4 | 5 => 100.0,
                _ => canvas_height * 0.78 - 2.0 * EXAMPLE_PANEL_PADDING_PX,
            };
            let mut target = example_panel_quad(
                canvas_width,
                canvas_height,
                self.theme_progress,
                content_height,
            );

            if let Some(heading) = self.example_heading_entity(idx) {
                let heading_height = self
                    .ui_world
                    .world
                    .get::<BakedText>(heading)
                    .map(|b| b.pixel_size[1])
                    .unwrap_or(0.0);
                target.position.y = -(heading_height + EXAMPLE_HEADING_PANEL_GAP_PX) / 2.0;
            }
            target
        }

        /// The Stress Tests panel's resting geometry — unlike every
        /// other category, this one doesn't size from a fixed content
        /// height centered as a block. Instead the whole assembly (top
        /// nav icons → heading → buttons → panel → bottom of screen) is
        /// anchored at both ends: the heading sits
        /// `SCREEN_NAV_CLEARANCE_PX` below the reserved nav-icon row
        /// (`ICON_ROW_RESERVED_PX` already bakes that clearance in), the
        /// panel's bottom edge sits `SCREEN_NAV_CLEARANCE_PX` above the
        /// very bottom of the screen — the *same* clearance value used at
        /// the top, so the assembly reads as symmetric — and the panel's
        /// top edge is wherever it lands after stacking the heading and
        /// buttons (with the usual `EXAMPLE_HEADING_PANEL_GAP_PX` gap
        /// between each) down from the top: growing/shrinking either of
        /// those pushes the panel's top edge down/up, but its bottom
        /// edge never moves. `layout_stress_content` positions the
        /// heading and buttons by stacking the *same* gaps in the same
        /// order starting from this panel's own top edge, so the two
        /// stay in sync.
        fn stress_panel_target(&mut self, canvas_width: f32, canvas_height: f32) -> QuadState {
            let width = (canvas_width * 0.85).min(1000.0);

            let heading_height = self
                .ui_world
                .world
                .get::<BakedText>(self.stress_title)
                .map(|b| b.pixel_size[1])
                .unwrap_or(0.0);

            let heading_top_y = canvas_height / 2.0 - ICON_ROW_RESERVED_PX;
            let heading_bottom_y = heading_top_y - heading_height;
            let buttons_top_y = heading_bottom_y - EXAMPLE_HEADING_PANEL_GAP_PX;
            let buttons_bottom_y = buttons_top_y - STRESS_BUTTON_HEIGHT_PX;
            let panel_top_edge = buttons_bottom_y - EXAMPLE_HEADING_PANEL_GAP_PX;

            let panel_bottom_edge = -canvas_height / 2.0 + SCREEN_NAV_CLEARANCE_PX;

            let height = (panel_top_edge - panel_bottom_edge).max(0.0);
            let position_y = (panel_top_edge + panel_bottom_edge) / 2.0;

            // Same flat, theme-independent light grey and theme-lerped
            // corner radius as `example_panel_quad` — duplicated rather
            // than shared since this screen's height/position math isn't
            // content-height driven the way that helper assumes.
            QuadState {
                position: Vec3::new(0.0, position_y, 0.5),
                size: Vec2::new(width, height),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::new(0.82, 0.82, 0.83, 1.0),
                corner_radius: SCREEN_CORNER_RADIUS
                    + (SCREEN_CORNER_RADIUS_DARK - SCREEN_CORNER_RADIUS) * self.theme_progress,
            }
        }

        /// Unconditionally forces `example_detail_panel` into its resting
        /// geometry for category `idx` and reveals that category's
        /// content — the `ExampleDetail` counterpart of
        /// `settle_tile_screen`.
        fn settle_example_detail_panel(&mut self, idx: usize) {
            let panel = self.example_detail_panel;
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(panel) {
                vis.visible = true;
            }
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;

            let target = self.example_detail_target(idx, canvas_width, canvas_height);

            let panel_position = target.position;
            let panel_size = target.size;
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(panel) {
                *qs = target;
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(panel) {
                border.color.w = 1.0;
            }
            for &content in &self.example_content[idx] {
                if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(content) {
                    vis.visible = true;
                }
            }
            match idx {
                0 => self.layout_effects_content(panel_position, panel_size),
                1 => self.layout_text_content(panel_position, panel_size),
                2 => self.layout_transforms_content(panel_position, panel_size),
                3 => self.layout_stress_content(panel_position, panel_size),
                _ => {}
            }
        }

        // -------------------------------------------------------------------
        // Photo gallery — Home ↔ Loading ↔ Gallery (M12)
        // -------------------------------------------------------------------

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
        /// hidden (`AppState::Home`/`Loading`) or visible
        /// (`AppState::Gallery`) — hover reset to idle either way, mirroring
        /// `settle_tile_idle`. The 12 real tiles are never swapped away from
        /// their cropped `BakedImage` (see `gallery_enlarged_base`'s doc for
        /// why the enlarged view uses a separate entity instead), so unlike
        /// `settle_gallery_enlarged_base` there's no bake to restore here.
        fn settle_gallery_tile(&mut self, i: usize, visible: bool) {
            let tile = self.gallery_tiles[i];
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(tile) {
                vis.visible = visible;
            }
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let mut state = gallery_cell_quad(i, canvas_width, canvas_height, self.theme_progress);
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

        /// Unconditionally forces `gallery_enlarged_base` into its resting
        /// enlarged geometry, showing photo `idx`'s full frame — the
        /// `GalleryImage` counterpart of `settle_tile_screen`. Doesn't touch
        /// `gallery_hires_overlay`; that's `advance_gallery_hires_overlay`'s
        /// job, every frame, for as long as `gallery_hires_for_tile ==
        /// Some(idx)`.
        fn settle_gallery_enlarged_base(&mut self, idx: usize) {
            let entity = self.gallery_enlarged_base;
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(entity) {
                vis.visible = true;
            }
            // The low-res stand-in must show the same full frame hires will
            // — see `gallery_tile_full_baked`'s doc.
            if let Some(full) = self.gallery_tile_full_baked[idx].clone() {
                self.ui_world.world.entity_mut(entity).insert(full);
            }
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let target = gallery_large_image_quad(
                self.gallery_tile_aspect[idx],
                canvas_width,
                canvas_height,
                self.theme_progress,
            );
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                qs.position = target.position;
                qs.size = target.size;
                qs.corner_radius = target.corner_radius;
                qs.scale = 1.0;
                qs.color = white();
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(entity) {
                border.color.w = 1.0;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = 0.0;
                glow.color.w = 1.0;
            }
            self.gallery_enlarged_hover_progress = 0.0;
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
        /// tile's own `QuadState` — group transitions never write a
        /// target's own `QuadState` (see `layout_nav_buttons`'s doc), so
        /// this must happen before `OneToNRequest` is inserted. Returns the
        /// same states for use as `GroupTarget`s.
        fn layout_gallery_tiles(&mut self) -> [QuadState; 12] {
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let states: [QuadState; 12] = std::array::from_fn(|i| {
                let mut qs = gallery_cell_quad(i, canvas_width, canvas_height, self.theme_progress);
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
            let button_size = self
                .ui_world
                .world
                .get::<BakedText>(self.gallery_fetch_button_label)
                .map(|b| {
                    Vec2::new(b.pixel_size[0], b.pixel_size[1])
                        + Vec2::splat(2.0 * NAV_BUTTON_PADDING_PX)
                })
                .unwrap_or(GALLERY_FETCH_BUTTON_FALLBACK_SIZE);
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_fetch_button)
            {
                qs.size = button_size;
                qs.position.x = 0.0;
                qs.position.y =
                    canvas_height / 2.0 - GALLERY_FETCH_BUTTON_TOP_MARGIN_PX - button_size.y / 2.0;
            }
            states
        }

        /// N→1 Slice: the 3 nav buttons merge into `loading_logo` and
        /// signals JS to start a fresh fetch — every Home→Loading entry
        /// re-fetches. Bumps `gallery_fetch_generation` so the "all 12
        /// tiles loaded" check (`advance_demo`'s `Loading` arm) can't be
        /// satisfied by a previous fetch's images still sitting on the
        /// tiles — each tile's own `BakedImage`/`TextureRef` is cleared
        /// individually, right as its *own* new image arrives
        /// (`set_gallery_image`), not here; see that function's doc for why
        /// clearing eagerly for all 12 up front is what caused stale/lost
        /// images before.
        fn start_home_to_loading(&mut self) {
            self.gallery_fetch_elapsed = 0.0;
            self.gallery_error_shown = false;
            self.gallery_logo_error_fade = 1.0;
            self.gallery_fetch_generation = self.gallery_fetch_generation.wrapping_add(1);
            self.hide_gallery_error();
            self.loading_logo_frame_elapsed = 0.0;
            self.loading_logo_frame_index = 0;
            self.apply_loading_logo_frame();

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
                    layout: MergeLayout::Horizontal,
                });

            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let side_px = gallery_cell_size(canvas_width, canvas_height)
                .min(MAX_TILE_IMAGE_SIDE as f32) as u32;
            self.pending_gallery_fetch = Some(side_px);
        }

        /// N→1 GridSlice: the "Fetch New Images" button's click handler —
        /// the 12 gallery tiles collapse into `loading_logo` (mirroring
        /// `start_home_to_loading`, sourced from the grid instead of the nav
        /// buttons) before a fresh fetch replaces them. Deliberately does
        /// *not* clear the tiles' current bake first — the collapse should
        /// show today's actual photos shrinking into the logo, and
        /// `n_to_one_setup_system` bakes each source's live appearance at
        /// setup time, so clearing here would make it collapse from a blank
        /// placeholder instead. Bumping `gallery_fetch_generation` (rather
        /// than clearing the tiles' bake right away) is what keeps the
        /// "all 12 loaded" check from being satisfied by the *outgoing*
        /// batch still sitting on the (now hidden, mid-collapse) tiles —
        /// each tile's own stale `BakedImage`/`TextureRef` is cleared
        /// individually once its *own* new image actually arrives (see
        /// `set_gallery_image`), whenever that happens to land relative to
        /// the collapse animation.
        fn start_gallery_to_loading(&mut self) {
            self.gallery_fetch_elapsed = 0.0;
            self.gallery_error_shown = false;
            self.gallery_logo_error_fade = 1.0;
            self.gallery_fetch_generation = self.gallery_fetch_generation.wrapping_add(1);
            self.hide_gallery_error();
            self.loading_logo_frame_elapsed = 0.0;
            self.loading_logo_frame_index = 0;
            self.apply_loading_logo_frame();

            let sources: Vec<GroupSource> = (0..12)
                .map(|i| GroupSource {
                    entity: self.gallery_tiles[i],
                    state: self
                        .ui_world
                        .world
                        .get::<QuadState>(self.gallery_tiles[i])
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
                        duration: GALLERY_GRID_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                    layout: MergeLayout::Grid {
                        cols: GALLERY_COLS,
                        rows: GALLERY_ROWS,
                    },
                });

            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let side_px = gallery_cell_size(canvas_width, canvas_height)
                .min(MAX_TILE_IMAGE_SIDE as f32) as u32;
            self.pending_gallery_fetch = Some(side_px);
        }

        /// 1→N GridSlice: `loading_logo` splits into all 12 gallery tiles,
        /// grid-paired (not `Slice`'s flat 12-wide strip) so each tile's
        /// virtual radiates outward from its own quadrant of the logo —
        /// see `SplitStrategy::GridSlice`'s doc for why `Slice` looked like a
        /// zigzag here instead of a starburst.
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
                    strategy: SplitStrategy::GridSlice {
                        cols: GALLERY_COLS,
                        rows: GALLERY_ROWS,
                    },
                });
        }

        /// 1→N Slice, the error-escape-hatch path: `loading_logo` splits
        /// back into the 3 nav buttons directly (mirrors
        /// `start_screen_to_nav`'s "read back the already-correct state"
        /// idiom — nothing has touched the buttons' `QuadState` since
        /// `layout_nav_buttons` last ran).
        fn start_loading_to_home(&mut self) {
            self.pending_gallery_fetch = None;
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

        /// Three independent N→1 GridSlice merges, fired in the same frame:
        /// each column *group* of gallery tiles converges into its
        /// corresponding nav button — column 0 → button 0, columns 1 and 2
        /// → button 1, column 3 → button 2 — the way to compose an "M
        /// sources → N destinations" effect from the framework's 1↔N
        /// primitives (confirmed during planning: multiple `NToOneRequest`s
        /// with different destinations coexist correctly in the same
        /// frame, no framework changes needed). Each group uses
        /// `MergeLayout::Grid` (not `Horizontal`) so a group's tiles —
        /// stacked by row, one or two columns wide — converge onto the
        /// matching row/col cell of their button instead of onto `n` flat
        /// horizontal strips of it; see `SplitStrategy::GridSlice`'s doc for
        /// why that avoids the zigzag the old row-based split had.
        fn start_gallery_to_home(&mut self) {
            const COLUMN_GROUPS: [(&[usize], usize); 3] = [(&[0], 0), (&[1, 2], 1), (&[3], 2)];
            for (cols, button_idx) in COLUMN_GROUPS {
                let sources: Vec<GroupSource> = (0..GALLERY_ROWS)
                    .flat_map(|row| cols.iter().map(move |&col| row * GALLERY_COLS + col))
                    .map(|tile_idx| {
                        let entity = self.gallery_tiles[tile_idx];
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
                    .entity_mut(self.nav_buttons[button_idx])
                    .insert(NToOneRequest {
                        sources,
                        default_config: TransitionConfig {
                            duration: GALLERY_GRID_MORPH_DURATION,
                            delay: 0.0,
                            easing: ease_in_out_quad,
                        },
                        child_behavior: None,
                        layout: MergeLayout::Grid {
                            cols: cols.len(),
                            rows: GALLERY_ROWS,
                        },
                    });
            }
        }

        /// N→1 GridSlice: all 12 gallery tiles converge into
        /// `gallery_enlarged_base`, which becomes the enlarged image — the
        /// same starburst mechanic `start_gallery_to_loading` uses for the
        /// grid↔loading-logo morph, just converging onto a dedicated entity
        /// instead of `loading_logo`. Uses `gallery_enlarged_base` rather
        /// than `gallery_tiles[idx]` itself — see that field's doc for why
        /// reusing the clicked tile as the coordinator breaks the *reverse*
        /// trip. Its current `QuadState` doesn't matter (unlike a plain
        /// `OneToNRequest` source, whose *current* state is already exactly
        /// what should be sliced from) because `n_to_one_setup_system`
        /// reads the destination's *current* `QuadState` next tick to
        /// compute slice geometry — so it's overwritten to the enlarged
        /// target before inserting the request.
        ///
        /// Kicks off the hires fetch for this tile's already-assigned
        /// photo_id/aspect (stamped when the low-res batch fetch landed —
        /// see `set_gallery_image`), sized to the actual on-screen fitted
        /// dimensions this morph is heading toward.
        fn start_gallery_to_image(&mut self, idx: usize) {
            self.gallery_hires_for_tile = Some(idx);
            self.gallery_hires_fade = 0.0;
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;

            let sources: Vec<GroupSource> = (0..12)
                .map(|i| {
                    let state = self
                        .ui_world
                        .world
                        .get::<QuadState>(self.gallery_tiles[i])
                        .cloned()
                        .unwrap_or_default();
                    GroupSource {
                        entity: self.gallery_tiles[i],
                        state,
                    }
                })
                .collect();

            let to = gallery_large_image_quad(
                self.gallery_tile_aspect[idx],
                canvas_width,
                canvas_height,
                self.theme_progress,
            );
            // Cap proportionally (the *larger* axis against the cap, both
            // scaled by the same factor) rather than clamping each axis
            // independently — independent clamping only ever changes a
            // square request's aspect ratio by construction (both axes
            // equal), but silently distorts a portrait/landscape one
            // whenever just one axis crosses the cap: loremflickr then
            // crops the underlying photo to a *different* ratio than the
            // low-res fetch used, showing up as a shift/"different crop"
            // the moment hires swaps in.
            let uncapped = to.size.x.max(to.size.y);
            let cap_scale = (GALLERY_LARGE_IMAGE_MAX_SIDE as f32 / uncapped).min(1.0);
            let width = (to.size.x * cap_scale).round() as u32;
            let height = (to.size.y * cap_scale).round() as u32;

            // `gallery_enlarged_base` (not `gallery_tiles[idx]`) is the
            // destination — see its doc for why. Show the full frame, not
            // the grid's center-cropped square — see `gallery_tile_full_baked`'s
            // doc for why the low-res stand-in must already match what
            // hires will eventually show.
            let dest = self.gallery_enlarged_base;
            if let Some(full) = self.gallery_tile_full_baked[idx].clone() {
                self.ui_world.world.entity_mut(dest).insert(full);
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(dest) {
                *qs = to;
            }
            if let Some(mut vis) = self.ui_world.world.get_mut::<Visibility>(dest) {
                vis.visible = false;
            }

            self.ui_world.world.entity_mut(dest).insert(NToOneRequest {
                sources,
                default_config: TransitionConfig {
                    duration: GALLERY_GRID_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                child_behavior: None,
                layout: MergeLayout::Grid {
                    cols: GALLERY_COLS,
                    rows: GALLERY_ROWS,
                },
            });

            self.pending_gallery_hires_fetch = Some(GalleryHiresRequest {
                tile_idx: idx as u32,
                width,
                height,
                photo_id: self.gallery_tile_photo_id[idx],
            });
        }

        /// 1→N GridSlice: reverses `start_gallery_to_image` — `gallery_enlarged_base`
        /// splits into all 12 grid cells. Unlike before this used a
        /// dedicated coordinator entity, the source here is *never* also one
        /// of the 12 targets, so each target's own bake (its live,
        /// permanently-cropped `BakedImage`) is always correct — no more
        /// racing the source's own "show the full frame" state. Doesn't
        /// pre-write anything to the real entities: a `OneToNRequest`
        /// source's own *current* `QuadState` (still the enlarged size —
        /// exactly what should be sliced from) is read directly, and
        /// `settle(Gallery)` already unconditionally fixes up all 12 tiles'
        /// resting geometry once this completes.
        fn start_image_to_gallery(&mut self) {
            self.cancel_gallery_hires_fetch();
            let canvas_width = self.surface_config.width as f32;
            let canvas_height = self.surface_config.height as f32;
            let targets = (0..12)
                .map(|i| {
                    let mut state =
                        gallery_cell_quad(i, canvas_width, canvas_height, self.theme_progress);
                    if self
                        .ui_world
                        .world
                        .get::<BakedImage>(self.gallery_tiles[i])
                        .is_some()
                    {
                        state.color = white();
                    }
                    GroupTarget {
                        entity: self.gallery_tiles[i],
                        state,
                    }
                })
                .collect();
            self.ui_world
                .world
                .entity_mut(self.gallery_enlarged_base)
                .insert(OneToNRequest {
                    targets,
                    default_config: TransitionConfig {
                        duration: GALLERY_GRID_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                    strategy: SplitStrategy::GridSlice {
                        cols: GALLERY_COLS,
                        rows: GALLERY_ROWS,
                    },
                });
        }

        /// 1→N Slice: the enlarged image splits directly into the 3 nav
        /// buttons — mirrors `start_screen_to_nav`, skipping back through
        /// the grid. Cancels any in-flight hires fetch first, same as
        /// `start_image_to_gallery`.
        fn start_image_to_home(&mut self) {
            self.cancel_gallery_hires_fetch();
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
                .entity_mut(self.gallery_enlarged_base)
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

        /// Cancels any in-flight hires fetch and hides/clears the overlay —
        /// called whenever leaving `GalleryImage` before (or after) the
        /// hires image arrives. `gallery_hires_for_tile = None` is what
        /// makes a late `set_gallery_hires_image` call (the network request
        /// already in flight, JS's `AbortController` notwithstanding) a
        /// no-op instead of applying to the wrong tile.
        fn cancel_gallery_hires_fetch(&mut self) {
            self.gallery_hires_for_tile = None;
            self.pending_gallery_hires_fetch = None;
            self.pending_gallery_hires_cancel = true;
            self.gallery_hires_fade = 0.0;
            self.ui_world
                .world
                .entity_mut(self.gallery_hires_overlay)
                .remove::<(BakedImage, TextureRef, Image)>();
            if let Some(mut vis) = self
                .ui_world
                .world
                .get_mut::<Visibility>(self.gallery_hires_overlay)
            {
                vis.visible = false;
            }
        }

        /// While resting in `Loading`, accumulates the fetch timeout — the
        /// "all 12 baked → go to Gallery" decision itself lives in
        /// `advance_demo`'s `Loading` arm (keeping this file's existing
        /// invariant that every state transition originates from
        /// `advance_demo`'s match, same as `Splash`'s own timer-driven arm).
        /// No draining step here (unlike native) — images arrive directly
        /// via `set_gallery_image`, called from JS.
        fn advance_gallery_fetch(&mut self, dt: f32) {
            if self.state != AppState::Loading
                || self.transition.is_some()
                || self.gallery_error_shown
            {
                return;
            }
            self.gallery_fetch_elapsed += dt;
            if self.gallery_fetch_elapsed >= GALLERY_FETCH_TIMEOUT {
                self.gallery_error_shown = true;
                self.show_gallery_error();
            }
        }

        /// Fades `loading_logo`/`loading_logo_dark` out once
        /// `gallery_error_shown` fires — the spinner otherwise keeps
        /// looping forever behind the error text, which now sits dead
        /// center where the logo would otherwise show through. Runs after
        /// `advance_theme` so it's the last write to `loading_logo_dark`'s
        /// alpha this frame, combining with (rather than fighting)
        /// `advance_theme`'s own `theme_progress` crossfade on that layer —
        /// `loading_logo`'s own alpha has no other owner, so this is a
        /// plain overwrite there.
        fn advance_gallery_error_fade(&mut self, dt: f32) {
            let target = if self.gallery_error_shown { 0.0 } else { 1.0 };
            let step = dt / NAV_ICON_FADE_DURATION;
            if self.gallery_logo_error_fade < target {
                self.gallery_logo_error_fade = (self.gallery_logo_error_fade + step).min(target);
            } else if self.gallery_logo_error_fade > target {
                self.gallery_logo_error_fade = (self.gallery_logo_error_fade - step).max(target);
            }
            let fade = self.gallery_logo_error_fade;
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.loading_logo) {
                qs.color.w = fade;
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.loading_logo_dark)
            {
                qs.color.w = self.theme_progress * fade;
            }
        }

        /// Keeps `gallery_hires_overlay` glued to `gallery_enlarged_base` —
        /// position/size/scale/corner_radius copied every frame, since that
        /// entity's own geometry is still animating during the
        /// `Gallery`↔`GalleryImage` morph (and `scale` alone still changes
        /// at rest, via hover — `QuadState::scale` is a separate multiplier
        /// the shader applies at render time, not baked into `size`, so
        /// skipping it here left the overlay a fixed size while the base
        /// scaled on hover underneath, reading as a second, non-scaling
        /// image ghosting behind the real one) — and crossfades its alpha
        /// in once the hires fetch has actually baked
        /// (`bake_gallery_hires_image`) *and* the `Gallery` → `GalleryImage`
        /// morph has fully settled. Gating on `self.state` (not just
        /// `has_bake`) matters because a hires fetch routinely resolves
        /// before the ~0.6s morph animation finishes — especially on web,
        /// where `fetch()` is much faster than native's blocking `ureq`
        /// calls — and crossfading in mid-morph reads as the sharp image
        /// popping in before the tile has finished growing into place; the
        /// low-res stand-in should hold until the morph is done, then
        /// crossfade.
        /// A no-op whenever nothing is enlarged (`gallery_hires_for_tile`
        /// is `None`, e.g. after `cancel_gallery_hires_fetch`), leaving the
        /// overlay wherever `cancel_gallery_hires_fetch` last hid it.
        fn advance_gallery_hires_overlay(&mut self, dt: f32) {
            let Some(idx) = self.gallery_hires_for_tile else {
                return;
            };
            let base = self.gallery_enlarged_base;
            let base_qs = self
                .ui_world
                .world
                .get::<QuadState>(base)
                .cloned()
                .unwrap_or_default();
            // Mirrored (not just position/size) so the border/hover-glow
            // ring stays visible once the overlay's alpha reaches 1 and
            // would otherwise occlude the base's own — `advance_gallery_enlarged_hover`
            // and `advance_theme` already drive these on `base` earlier this
            // same tick; this just copies their result across.
            let base_border = self.ui_world.world.get::<Border>(base).cloned();
            let base_glow = self.ui_world.world.get::<Glow>(base).cloned();
            let has_bake = self
                .ui_world
                .world
                .get::<BakedImage>(self.gallery_hires_overlay)
                .is_some();
            let settled = self.state == AppState::GalleryImage(idx);
            let target = if has_bake && settled { 1.0 } else { 0.0 };
            let step = dt / GALLERY_HIRES_CROSSFADE_DURATION;
            if self.gallery_hires_fade < target {
                self.gallery_hires_fade = (self.gallery_hires_fade + step).min(target);
            }
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_hires_overlay)
            {
                qs.position.x = base_qs.position.x;
                qs.position.y = base_qs.position.y;
                qs.size = base_qs.size;
                qs.scale = base_qs.scale;
                qs.corner_radius = base_qs.corner_radius;
                qs.color.w = self.gallery_hires_fade;
            }
            if let Some(b) = base_border {
                if let Some(mut ob) = self
                    .ui_world
                    .world
                    .get_mut::<Border>(self.gallery_hires_overlay)
                {
                    *ob = b;
                }
            }
            if let Some(g) = base_glow {
                if let Some(mut og) = self
                    .ui_world
                    .world
                    .get_mut::<Glow>(self.gallery_hires_overlay)
                {
                    *og = g;
                }
            }
            if let Some(mut vis) = self
                .ui_world
                .world
                .get_mut::<Visibility>(self.gallery_hires_overlay)
            {
                // Gated on `settled`, not just `has_bake` — `Border`'s alpha
                // is independent of the quad's own `color.w`, so revealing
                // the overlay while only the fill is faded to 0 would still
                // pop its full-alpha border in at the overlay's (static,
                // already-final) position/size, ahead of the morph
                // animation actually arriving there.
                vis.visible = has_bake && settled;
            }
        }

        /// Mirrors `advance_logo_animation`'s timer+index sweep, but loops
        /// forever while `state == Loading` (rather than playing once
        /// during `Splash`) and drives both the light layer
        /// (`loading_logo`, reusing the already-loaded `logo_frames`) and
        /// the dark layer (`loading_logo_dark`, from
        /// `loading_logo_frames_dark`) in lockstep — `advance_theme`'s own
        /// crossfade (added to its dark-overlay array) handles which one is
        /// actually visible.
        fn advance_loading_logo_animation(&mut self, dt: f32) {
            if self.logo_frames.is_empty() || self.state != AppState::Loading {
                return;
            }
            self.loading_logo_frame_elapsed += dt;
            while self.loading_logo_frame_elapsed >= LOGO_FRAME_DURATION {
                self.loading_logo_frame_elapsed -= LOGO_FRAME_DURATION;
                self.loading_logo_frame_index =
                    (self.loading_logo_frame_index + 1) % self.logo_frames.len();
                self.apply_loading_logo_frame();
            }
        }

        /// Pushes `loading_logo_frame_index`'s bake onto `loading_logo`/
        /// `loading_logo_dark` (both light/dark layers, if that frame
        /// loaded). Factored out of `advance_loading_logo_animation`'s loop
        /// body so `start_home_to_loading`/`start_gallery_to_loading` can
        /// call it once, immediately, to force frame 0 onto the entity the
        /// instant a fresh Loading visit begins — otherwise the entity
        /// keeps showing whichever frame the *previous* visit last left it
        /// on until `advance_loading_logo_animation`'s own timer first
        /// ticks past `LOGO_FRAME_DURATION`, which reads as the animation
        /// starting mid-sequence and jumping back to frame 0 a beat later.
        fn apply_loading_logo_frame(&mut self) {
            let Some(frame) = self.logo_frames.get(self.loading_logo_frame_index) else {
                return;
            };
            if let Some((texture_id, baked)) = frame.clone() {
                self.ui_world
                    .world
                    .entity_mut(self.loading_logo)
                    .insert((baked, TextureRef(texture_id)));
            }
            if let Some(Some((dark_id, dark_baked))) = self
                .loading_logo_frames_dark
                .get(self.loading_logo_frame_index)
                .cloned()
            {
                self.ui_world
                    .world
                    .entity_mut(self.loading_logo_dark)
                    .insert((dark_baked, TextureRef(dark_id)));
            }
        }

        /// Glow + scale-boost hover reaction for gallery tiles — no
        /// overlay-tint/title-label children exist on them (unlike the
        /// video tiles' full `advance_tile_hover`), so this is a smaller,
        /// separate function rather than a generalization of that one.
        /// `Gallery`-only: the enlarged image is a separate entity
        /// (`gallery_enlarged_base`, see `advance_gallery_enlarged_hover`),
        /// and these 12 real tiles are always hidden/non-interactive during
        /// `GalleryImage`.
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

        /// `gallery_enlarged_base`'s own hover reaction — glow only, no
        /// scale-boost (it's already as big as the grid box allows, so
        /// growing it further on hover would read as an odd wobble rather
        /// than an affordance, unlike the grid tiles' scale-boost). A
        /// separate function rather than folding into
        /// `advance_gallery_tile_hover` because it drives a different
        /// entity (not one of the 12 `gallery_tiles`) with its own
        /// non-array progress scalar.
        fn advance_gallery_enlarged_hover(&mut self, dt: f32) {
            let entity = self.gallery_enlarged_base;
            let suppressed =
                self.transition.is_some() || !matches!(self.state, AppState::GalleryImage(_));
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&entity) {
                    self.gallery_enlarged_is_hovering = true;
                } else if events.hover_exited.contains(&entity) {
                    self.gallery_enlarged_is_hovering = false;
                }
            }
            let target = if suppressed {
                0.0
            } else if self.gallery_enlarged_is_hovering {
                1.0
            } else {
                0.0
            };
            let step = dt / GLOW_DURATION;
            if self.gallery_enlarged_hover_progress < target {
                self.gallery_enlarged_hover_progress =
                    (self.gallery_enlarged_hover_progress + step).min(target);
            } else if self.gallery_enlarged_hover_progress > target {
                self.gallery_enlarged_hover_progress =
                    (self.gallery_enlarged_hover_progress - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.gallery_enlarged_hover_progress * GLOW_MAX_RADIUS;
            }
        }

        /// Fades `gallery_fetch_button` in only once fully settled in
        /// `Gallery` (i.e. after the 1→12 morph has already revealed the
        /// tiles), and out the instant a Gallery→Home morph begins — paced
        /// by `GALLERY_GRID_MORPH_DURATION` so it lands at 0 exactly when
        /// that morph completes. The button has no background fill (same
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

        /// The independent heading entity rendered above
        /// `example_detail_panel` for category `idx`, if it has one — not
        /// every category does (yet).
        fn example_heading_entity(&self, idx: usize) -> Option<Entity> {
            match idx {
                0 => Some(self.effects_title),
                1 => Some(self.text_title),
                2 => Some(self.transforms_title),
                3 => Some(self.stress_title),
                _ => None,
            }
        }

        /// Makes category `idx` the *only* one `ChildOf`
        /// `example_detail_panel` — every other category's content gets
        /// `ChildOf` removed. Call this before starting an
        /// `ExamplesHome`→`ExampleDetail(idx)` merge.
        ///
        /// Every category's content is spawned `ChildOf` the panel (see
        /// `example_content`'s doc for why: it's what makes content bake
        /// as part of the panel's own crossfade instead of popping in
        /// after). But `gather_bake_instances` walks a subtree
        /// unconditionally, ignoring `Visibility` — so with *two or more*
        /// categories simultaneously `ChildOf` the same panel, any bake of
        /// that panel (entering *or* leaving *any* category) would gather
        /// all of them at once, flashing every other category's content
        /// too. This keeps only the category actually being entered
        /// attached at any given moment, so a bake only ever sees that one
        /// category's content.
        ///
        /// Skips each category's own heading (`example_heading_entity`)
        /// and any tracked `example_reparent_exempt` even though both are
        /// tracked in the same `example_content[idx]` list — the heading
        /// is deliberately never `ChildOf` the panel (see
        /// `effects_title`'s doc), and a grandchild is `ChildOf` a
        /// *different* entity entirely (see `example_reparent_exempt`'s
        /// doc), so neither must ever gain or lose `ChildOf` the panel
        /// here.
        fn set_example_category_active(&mut self, idx: usize) {
            let panel = self.example_detail_panel;
            for category in 0..6 {
                let heading = self.example_heading_entity(category);
                let is_active = category == idx;
                for &content in &self.example_content[category] {
                    if Some(content) == heading || self.example_reparent_exempt.contains(&content) {
                        continue;
                    }
                    let mut entity = self.ui_world.world.entity_mut(content);
                    if is_active {
                        entity.insert(ChildOf(panel));
                    } else {
                        entity.remove::<ChildOf>();
                    }
                }
            }
        }

        /// Fades the active category's own heading in only once fully
        /// settled and idle in its `ExampleDetail` — never while the 6→1
        /// merge into `example_detail_panel` is still in flight — so the
        /// heading visibly fades in *after* the panel morph lands, not
        /// alongside it. Mirrors `advance_gallery_button_fade`'s pattern.
        /// Explicitly zeroes every *other* category's heading each tick
        /// too (rather than assuming the state machine always fades one
        /// out before fading the next in) — robust regardless of how the
        /// `ExampleDetail(idx)` transition graph evolves later.
        fn advance_example_title_fade(&mut self, dt: f32) {
            let active_heading = if self.transition.is_none() {
                match self.state {
                    AppState::ExampleDetail(idx) => self.example_heading_entity(idx),
                    _ => None,
                }
            } else {
                None
            };
            let target = if active_heading.is_some() { 1.0 } else { 0.0 };
            let step = dt / BUTTON_TILES_MORPH_DURATION;
            if self.example_title_fade < target {
                self.example_title_fade = (self.example_title_fade + step).min(target);
            } else if self.example_title_fade > target {
                self.example_title_fade = (self.example_title_fade - step).max(target);
            }
            for idx in 0..6 {
                if let Some(heading) = self.example_heading_entity(idx) {
                    let alpha = if Some(heading) == active_heading {
                        self.example_title_fade
                    } else {
                        0.0
                    };
                    if let Some(mut title) = self.ui_world.world.get_mut::<Text>(heading) {
                        title.color.w = alpha;
                    }
                }
            }
        }

        /// Drives the Continuous Animation box's rotation/scale/hue for
        /// as long as this screen is fully settled and idle in
        /// `ExampleDetail(2)` — paused (not reset) otherwise, so leaving
        /// and returning resumes from wherever it left off rather than
        /// snapping back to a fixed start pose.
        fn advance_example_animation(&mut self, dt: f32) {
            if self.transition.is_some() || self.state != AppState::ExampleDetail(2) {
                return;
            }
            self.transforms_anim_elapsed += dt;
            let t = self.transforms_anim_elapsed;

            const ROTATION_PERIOD_SECS: f32 = 4.0;
            const SCALE_PERIOD_SECS: f32 = 2.0;
            const HUE_PERIOD_SECS: f32 = 5.0;
            const SCALE_MIN: f32 = 0.8;
            const SCALE_MAX: f32 = 1.2;

            let rotation = (t / ROTATION_PERIOD_SECS) * std::f32::consts::TAU;
            let scale_mid = (SCALE_MIN + SCALE_MAX) / 2.0;
            let scale_amplitude = (SCALE_MAX - SCALE_MIN) / 2.0;
            let scale =
                scale_mid + scale_amplitude * (t / SCALE_PERIOD_SECS * std::f32::consts::TAU).sin();
            let hue = (t / HUE_PERIOD_SECS) * 360.0;
            let rgb = hsv_to_rgb(hue);

            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.transforms_continuous_box)
            {
                qs.rotation = rotation;
                qs.scale = scale;
                qs.color = Vec4::new(rgb.x, rgb.y, rgb.z, 1.0);
            }
        }

        /// Hover glow/scale for both Stress Tests buttons — same
        /// `hover_entered`/`hover_exited` → progress → `glow.radius`/`scale`
        /// pattern as `advance_gallery_fetch_button_hover`, just looped
        /// over `stress_buttons`'s 2 entries instead of a single button.
        /// Suppressed (forced toward 0) whenever a transition is in
        /// flight or the app isn't on this screen, same rationale as
        /// every other hover function in this file.
        fn advance_stress_button_hover(&mut self, dt: f32) {
            for i in 0..2 {
                let entity = self.stress_buttons[i];
                {
                    let events = self.ui_world.world.resource::<InteractionEvents>();
                    if events.hover_entered.contains(&entity) {
                        self.stress_button_is_hovering[i] = true;
                    } else if events.hover_exited.contains(&entity) {
                        self.stress_button_is_hovering[i] = false;
                    }
                }
                let suppressed =
                    self.transition.is_some() || self.state != AppState::ExampleDetail(3);
                let target = if suppressed {
                    0.0
                } else if self.stress_button_is_hovering[i] {
                    1.0
                } else {
                    0.0
                };
                let step = dt / GLOW_DURATION;
                if self.stress_button_hover_progress[i] < target {
                    self.stress_button_hover_progress[i] =
                        (self.stress_button_hover_progress[i] + step).min(target);
                } else if self.stress_button_hover_progress[i] > target {
                    self.stress_button_hover_progress[i] =
                        (self.stress_button_hover_progress[i] - step).max(target);
                }
                if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                    glow.radius = self.stress_button_hover_progress[i] * GLOW_MAX_RADIUS;
                }
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                    qs.scale = 1.0 + self.stress_button_hover_progress[i] * HOVER_SCALE_BOOST;
                }
            }
        }

        /// Fades both Stress Tests buttons in (once settled and idle
        /// here) and out (in sync with any transition away from this
        /// screen) — mirrors `advance_example_title_fade`'s exact
        /// pattern for the same reason: the buttons are
        /// independent/absolute (never baked into any transition — see
        /// `stress_buttons`'s doc), so without this they'd sit at full
        /// opacity for the whole outbound morph and pop off abruptly
        /// only once settle hides them via `example_content[3]`'s
        /// Visibility toggle. Only touches `Border`/`Glow`/`Text` alpha
        /// — `Visibility` itself stays owned by that same generic
        /// toggle, same division of labor as
        /// `advance_example_title_fade`.
        fn advance_stress_button_fade(&mut self, dt: f32) {
            let target = if self.transition.is_none() && self.state == AppState::ExampleDetail(3) {
                1.0
            } else {
                0.0
            };
            let step = dt / BUTTON_TILES_MORPH_DURATION;
            if self.stress_button_fade < target {
                self.stress_button_fade = (self.stress_button_fade + step).min(target);
            } else if self.stress_button_fade > target {
                self.stress_button_fade = (self.stress_button_fade - step).max(target);
            }
            for i in 0..2 {
                let button = self.stress_buttons[i];
                if let Some(mut b) = self.ui_world.world.get_mut::<Border>(button) {
                    b.color.w = self.stress_button_fade;
                }
                if let Some(mut g) = self.ui_world.world.get_mut::<Glow>(button) {
                    g.color.w = self.stress_button_fade;
                }
                let label = self.stress_button_labels[i];
                if let Some(mut t) = self.ui_world.world.get_mut::<Text>(label) {
                    t.color.w = self.stress_button_fade;
                }
            }
        }

        /// Shows the photosensitivity warning only while settled and
        /// idle on the Stress Tests screen with no test currently
        /// running — hidden the instant a test starts (Texture Churn's
        /// rapid color swaps are exactly what it warns about) and shown
        /// again once `finalize_stress_test`/`cancel_stress_test` clears
        /// the run.
        fn advance_stress_warning_visibility(&mut self) {
            let visible = self.transition.is_none()
                && self.state == AppState::ExampleDetail(3)
                && self.stress_test_run.is_none();
            if let Some(mut vis) = self
                .ui_world
                .world
                .get_mut::<Visibility>(self.stress_warning_text)
            {
                vis.visible = visible;
            }
        }

        /// Overwrites `stress_result_text`'s content and forces a fresh
        /// bake (`bake_pending_text` only ever bakes entities *without*
        /// `BakedText` — see the result text's own spawn-site doc — so
        /// updating `content` alone would leave the old baked glyph
        /// texture on screen forever). Removing `TextureRef` alongside it
        /// also frees the old atlas region via the M11 ref-counting path
        /// instead of leaking it.
        fn set_stress_result_text(&mut self, message: &str) {
            if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.stress_result_text) {
                text.content = message.to_string();
            }
            self.ui_world
                .world
                .entity_mut(self.stress_result_text)
                .remove::<BakedText>();
            self.ui_world
                .world
                .entity_mut(self.stress_result_text)
                .remove::<TextureRef>();
        }

        /// A fresh random target `QuadState` for one Burst Spawn entity,
        /// somewhere within `panel`'s current bounds — position, scale,
        /// and hue all randomized per the plan's "random
        /// position/color/scale" spec, reusing `hsv_to_rgb` (already
        /// built for the Transforms & Animation screen) for varied
        /// colors. Excludes the `RESULT_TEXT_RESERVED_HEIGHT_PX` strip at
        /// the bottom (see `layout_stress_content`) so quads fill the
        /// rest of the panel without ever rendering over the result
        /// text.
        fn random_burst_target(&mut self, panel: &QuadState) -> QuadState {
            let half_w =
                (panel.size.x / 2.0 - EXAMPLE_PANEL_PADDING_PX - BURST_ITEM_SIZE / 2.0).max(0.0);
            let usable_top = panel.position.y + panel.size.y / 2.0
                - EXAMPLE_PANEL_PADDING_PX
                - BURST_ITEM_SIZE / 2.0;
            let usable_bottom = panel.position.y - panel.size.y / 2.0
                + RESULT_TEXT_RESERVED_HEIGHT_PX
                + BURST_ITEM_SIZE / 2.0;
            let usable_center_y = (usable_top + usable_bottom) / 2.0;
            let usable_half_h = ((usable_top - usable_bottom) / 2.0).max(0.0);

            let x = panel.position.x
                + (random_unit_f32(&mut self.stress_rng_state) * 2.0 - 1.0) * half_w;
            let y = usable_center_y
                + (random_unit_f32(&mut self.stress_rng_state) * 2.0 - 1.0) * usable_half_h;
            let scale = 0.6 + random_unit_f32(&mut self.stress_rng_state) * 0.8;
            let hue = random_unit_f32(&mut self.stress_rng_state) * 360.0;
            let rgb = hsv_to_rgb(hue);
            QuadState {
                position: Vec3::new(x, y, 0.51),
                size: Vec2::new(BURST_ITEM_SIZE, BURST_ITEM_SIZE),
                rotation: 0.0,
                scale,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::new(rgb.x, rgb.y, rgb.z, 1.0),
                corner_radius: 4.0,
            }
        }

        /// "Run Burst Spawn" click handler — a no-op if a run is already
        /// in flight (per the plan: clicking either button mid-run
        /// doesn't restart it). Spawns `BURST_SPAWN_COUNT` small quads at
        /// random positions within the panel's current bounds, each
        /// already `Lifecycle::Idle` (so `advance_burst_spawn_entities`
        /// retriggers them on the very first tick rather than waiting a
        /// full `BURST_SPAWN_ITEM_DURATION`), and starts tracking the
        /// run.
        fn run_burst_spawn(&mut self) {
            if self.stress_test_run.is_some() {
                return;
            }
            let panel = self
                .ui_world
                .world
                .get::<QuadState>(self.example_detail_panel)
                .cloned()
                .unwrap_or_default();
            let mut entities = Vec::with_capacity(BURST_SPAWN_COUNT);
            for _ in 0..BURST_SPAWN_COUNT {
                let state = self.random_burst_target(&panel);
                let entity = self
                    .ui_world
                    .world
                    .spawn((state, Lifecycle::Idle, Visibility::VISIBLE))
                    .id();
                entities.push(entity);
            }
            self.stress_test_run = Some(StressTestRun {
                kind: StressTestKind::BurstSpawn,
                elapsed: 0.0,
                frame_count: 0,
                entities,
                churn_iterations: 0,
            });
        }

        /// "Run Texture Churn" click handler — same no-op-while-a-run-is-
        /// active guard as `run_burst_spawn`. Spawns `TEXTURE_CHURN_SLOTS`
        /// fixed quads in a `TEXTURE_CHURN_COLS`-column grid, centered in
        /// the panel's usable area (same reserved-bottom-strip exclusion
        /// as Burst Spawn's random target area, so the grid doesn't
        /// overlap the result text either) — each slot starts untextured
        /// (flat white); `advance_texture_churn_entities` gives all 8
        /// their first real texture on the very next tick.
        fn run_texture_churn(&mut self) {
            if self.stress_test_run.is_some() {
                return;
            }
            let panel = self
                .ui_world
                .world
                .get::<QuadState>(self.example_detail_panel)
                .cloned()
                .unwrap_or_default();

            let cols = TEXTURE_CHURN_COLS;
            let rows = (TEXTURE_CHURN_SLOTS as f32 / cols as f32).ceil();
            const GAP: f32 = 30.0;
            let total_w = cols as f32 * TEXTURE_CHURN_SLOT_SIZE + (cols as f32 - 1.0) * GAP;
            let total_h = rows * TEXTURE_CHURN_SLOT_SIZE + (rows - 1.0) * GAP;

            let usable_top = panel.position.y + panel.size.y / 2.0 - EXAMPLE_PANEL_PADDING_PX;
            let usable_bottom =
                panel.position.y - panel.size.y / 2.0 + RESULT_TEXT_RESERVED_HEIGHT_PX;
            let usable_center_y = (usable_top + usable_bottom) / 2.0;

            let start_x = panel.position.x - total_w / 2.0 + TEXTURE_CHURN_SLOT_SIZE / 2.0;
            let start_y = usable_center_y + total_h / 2.0 - TEXTURE_CHURN_SLOT_SIZE / 2.0;

            let mut entities = Vec::with_capacity(TEXTURE_CHURN_SLOTS);
            for i in 0..TEXTURE_CHURN_SLOTS {
                let row = i / cols;
                let col = i % cols;
                let x = start_x + col as f32 * (TEXTURE_CHURN_SLOT_SIZE + GAP);
                let y = start_y - row as f32 * (TEXTURE_CHURN_SLOT_SIZE + GAP);
                let entity = self
                    .ui_world
                    .world
                    .spawn((
                        QuadState {
                            position: Vec3::new(x, y, 0.51),
                            size: Vec2::new(TEXTURE_CHURN_SLOT_SIZE, TEXTURE_CHURN_SLOT_SIZE),
                            rotation: 0.0,
                            scale: 1.0,
                            anchor: Vec2::new(0.5, 0.5),
                            color: white(),
                            corner_radius: 8.0,
                        },
                        Lifecycle::Idle,
                        Visibility::VISIBLE,
                    ))
                    .id();
                entities.push(entity);
            }

            self.stress_test_run = Some(StressTestRun {
                kind: StressTestKind::TextureChurn,
                elapsed: 0.0,
                frame_count: 0,
                entities,
                churn_iterations: 0,
            });
        }

        /// A synthetic solid-color RGBA pixel buffer — Texture Churn's
        /// "isolate the resource-management path, not JPEG decode speed"
        /// source data (see its own doc). No image format, no allocation
        /// beyond the flat buffer itself.
        fn synthetic_texture_rgba(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
            let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
            for _ in 0..(width * height) {
                pixels.extend_from_slice(&color);
            }
            pixels
        }

        /// One Texture Churn register/evict cycle for a single slot
        /// entity: generates a fresh synthetic buffer (varying size and
        /// hue), registers + uploads it into `main_atlas`, then replaces
        /// the entity's `BakedImage`/`TextureRef` — `TextureRef`'s own
        /// `ComponentHooks` decref the old texture (freeing it once
        /// unreferenced, via the same eviction path `register_static`
        /// already falls back to when the atlas is full) and incref the
        /// new one, so no manual free call is needed here.
        fn churn_texture(&mut self, entity: Entity) {
            let width = TEXTURE_CHURN_SIZE_MIN
                + random_unit_f32(&mut self.stress_rng_state) * TEXTURE_CHURN_SIZE_SPREAD;
            let height = TEXTURE_CHURN_SIZE_MIN
                + random_unit_f32(&mut self.stress_rng_state) * TEXTURE_CHURN_SIZE_SPREAD;
            let (width, height) = (width as u32, height as u32);
            let hue = random_unit_f32(&mut self.stress_rng_state) * 360.0;
            let rgb = hsv_to_rgb(hue);
            let color = [
                (rgb.x * 255.0) as u8,
                (rgb.y * 255.0) as u8,
                (rgb.z * 255.0) as u8,
                255,
            ];
            let pixels = Self::synthetic_texture_rgba(width, height, color);

            let Some(texture_id) = self
                .ui_world
                .world
                .resource_mut::<QuadPipeline>()
                .texture_registry
                .register_static(width, height, false)
            else {
                // Atlas full even after eviction — skip this cycle
                // rather than panicking; the next one tries again.
                return;
            };
            let pipeline = self.ui_world.world.resource::<QuadPipeline>();
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, placement, &pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");

            self.ui_world.world.entity_mut(entity).insert((
                BakedImage {
                    uv_offset: uv.uv_offset,
                    uv_scale: uv.uv_scale,
                    page: uv.page,
                    pixel_size: [width as f32, height as f32],
                },
                TextureRef(texture_id),
            ));
        }

        /// Churns every Texture Churn slot once per tick — unlike Burst
        /// Spawn's per-entity `Lifecycle::Idle` gating (a texture swap is
        /// instantaneous, no transition to wait out), so all
        /// `TEXTURE_CHURN_SLOTS` entities get a fresh texture every frame
        /// the test runs, each swap counted toward `churn_iterations`.
        fn advance_texture_churn_entities(&mut self) {
            let entities = match &self.stress_test_run {
                Some(run) if run.kind == StressTestKind::TextureChurn => run.entities.clone(),
                _ => return,
            };
            for &entity in &entities {
                self.churn_texture(entity);
            }
            if let Some(run) = self.stress_test_run.as_mut() {
                run.churn_iterations += entities.len() as u32;
            }
        }

        /// For every Burst Spawn entity that's finished its current
        /// transition (`Lifecycle::Idle`), fires a fresh one to a new
        /// random target — the "continuously re-triggers... for the
        /// full 3s window" sustained load the plan calls for, rather
        /// than a one-shot initial burst.
        fn advance_burst_spawn_entities(&mut self) {
            let entities = match &self.stress_test_run {
                Some(run) if run.kind == StressTestKind::BurstSpawn => run.entities.clone(),
                _ => return,
            };
            let panel = self
                .ui_world
                .world
                .get::<QuadState>(self.example_detail_panel)
                .cloned()
                .unwrap_or_default();
            for entity in entities {
                let lifecycle = self.ui_world.world.get::<Lifecycle>(entity);
                if !matches!(lifecycle, Some(Lifecycle::Idle)) {
                    continue;
                }
                let to = self.random_burst_target(&panel);
                self.ui_world
                    .world
                    .entity_mut(entity)
                    .insert(TransitionRequest {
                        to,
                        config: TransitionConfig {
                            duration: BURST_SPAWN_ITEM_DURATION,
                            delay: 0.0,
                            easing: ease_in_out_quad,
                        },
                        from_state: None,
                    });
            }
        }

        /// Ticks the in-flight stress test (if any), dispatching
        /// per-frame work by `StressTestKind`, and hands off to
        /// `finalize_stress_test` once `STRESS_TEST_DURATION` has
        /// elapsed.
        fn advance_stress_test(&mut self, dt: f32) {
            let kind = match &self.stress_test_run {
                Some(run) => run.kind,
                None => return,
            };
            let elapsed = {
                let run = self.stress_test_run.as_mut().expect("checked above");
                run.elapsed += dt;
                run.frame_count += 1;
                run.elapsed
            };
            if elapsed >= STRESS_TEST_DURATION {
                self.finalize_stress_test();
                return;
            }
            match kind {
                StressTestKind::BurstSpawn => self.advance_burst_spawn_entities(),
                StressTestKind::TextureChurn => self.advance_texture_churn_entities(),
            }
        }

        /// Computes the average FPS, formats the result message,
        /// despawns the run's scratch entities, and clears
        /// `stress_test_run` back to `None` so a fresh click can start
        /// another run.
        fn finalize_stress_test(&mut self) {
            let Some(run) = self.stress_test_run.take() else {
                return;
            };
            let avg_fps = run.frame_count as f32 / run.elapsed;
            // Both shells set `PresentMode::AutoVsync` (see `lib.rs`'s own
            // surface config) — a real, deliberate cap, not a
            // stress-test bottleneck, so a result that bumped up against
            // it deserves a callout rather than reading like this demo
            // can't push past ~60 FPS on its own.
            let vsync_note = if avg_fps >= VSYNC_FPS_CAP_THRESHOLD {
                " (FPS capped at 60 by vsync)"
            } else {
                ""
            };
            let message = match run.kind {
                StressTestKind::BurstSpawn => format!(
                    "Burst Spawn: {} entities, avg {avg_fps:.1} FPS over {STRESS_TEST_DURATION:.1}s{vsync_note}",
                    run.entities.len()
                ),
                StressTestKind::TextureChurn => format!(
                    "Texture Churn: {} register/evict cycles, avg {avg_fps:.1} FPS over {STRESS_TEST_DURATION:.1}s{vsync_note}",
                    run.churn_iterations
                ),
            };
            for &entity in &run.entities {
                self.ui_world.world.despawn(entity);
            }
            self.set_stress_result_text(&message);
        }

        /// Immediately cancels the in-flight stress test (if any) —
        /// despawns its scratch entities and clears `stress_test_run`
        /// back to `None`, without reporting a result (unlike
        /// `finalize_stress_test`, this isn't a completion). Called when
        /// navigating away from the Stress Tests screen mid-run, so a
        /// test's entities don't keep existing (and, since they're
        /// independent/absolute rather than `ChildOf` the panel, keep
        /// rendering) over the transition or on whatever screen comes
        /// next.
        fn cancel_stress_test(&mut self) {
            let Some(run) = self.stress_test_run.take() else {
                return;
            };
            for &entity in &run.entities {
                self.ui_world.world.despawn(entity);
            }
        }

        /// Hover glow/scale for `gallery_fetch_button` — same
        /// `hover_entered`/`hover_exited` → progress → `glow.radius`/`scale`
        /// pattern as `advance_nav_hover`/`advance_gallery_tile_hover`. Only
        /// touches `radius`/`scale`, never `color.w` — that axis is
        /// `advance_gallery_button_fade`'s job (fade in/out), and the two
        /// don't conflict since they're orthogonal components of the same
        /// `Glow`/`QuadState`.
        fn advance_gallery_fetch_button_hover(&mut self, dt: f32) {
            let entity = self.gallery_fetch_button;
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&entity) {
                    self.gallery_fetch_button_is_hovering = true;
                } else if events.hover_exited.contains(&entity) {
                    self.gallery_fetch_button_is_hovering = false;
                }
            }
            let suppressed = self.transition.is_some() || self.state != AppState::Gallery;
            let target = if suppressed {
                0.0
            } else if self.gallery_fetch_button_is_hovering {
                1.0
            } else {
                0.0
            };
            let step = dt / GLOW_DURATION;
            if self.gallery_fetch_button_hover_progress < target {
                self.gallery_fetch_button_hover_progress =
                    (self.gallery_fetch_button_hover_progress + step).min(target);
            } else if self.gallery_fetch_button_hover_progress > target {
                self.gallery_fetch_button_hover_progress =
                    (self.gallery_fetch_button_hover_progress - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.gallery_fetch_button_hover_progress * GLOW_MAX_RADIUS;
            }
            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(entity) {
                qs.scale = 1.0 + self.gallery_fetch_button_hover_progress * HOVER_SCALE_BOOST;
            }
        }

        /// Drives the manual fades that accompany `start_tiles_to_screen`'s
        /// `TransitionRequest` (which only lerps `QuadState` — position, size,
        /// fill color, corner radius — and leaves `Border`/`Text`/`Glow` alone):
        /// the morphing tile's label fades out over the full morph duration, and
        /// the other two tiles fade out completely (fill, border, label, glow)
        /// over half the duration.
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
            // Live crossfade (M9.8): box art → video, same easing as the
            // morph's own geometry so both read as one motion.
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
    }
} // mod inner

// Re-export ProteusApp at crate root so wasm-bindgen can generate bindings.
#[cfg(target_arch = "wasm32")]
pub use inner::ProteusApp;
