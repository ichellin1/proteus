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
        BakedImage, BakedText, Border, ChildOf, Entity, Glow, GroupSource, GroupTarget,
        HoveredEntity, Image, Interactable, InteractionEvents, Lifecycle, MergeLayout,
        NToOneRequest, OneToNRequest, PointerInput, ProteusWorld, QuadState, SplitStrategy, Text,
        TextureRef, TransitionRequest, VideoCrossfade, VideoPlayer, Visibility,
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
    const TILE_CORNER_RADIUS_DARK: f32 = 30.0;

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
    const GALLERY_CORNER_RADIUS: f32 = 12.0;
    const GALLERY_CORNER_RADIUS_DARK: f32 = 18.0;
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
    // Nav buttons — splash morphs into these; clicking "Video Demo" morphs into tiles
    // -------------------------------------------------------------------------

    const NAV_BUTTON_TITLES: [&str; 3] = ["Video Demo", "Photo Gallery", "Examples & Tests"];
    const NAV_BUTTON_LABEL_SIZE_PX: f32 = 24.0;
    const NAV_BUTTON_LETTER_SPACING_PX: f32 = NAV_BUTTON_LABEL_SIZE_PX * 0.02;
    const NAV_BUTTON_PADDING_PX: f32 = 15.0;
    const NAV_BUTTON_GAP_PX: f32 = 50.0;
    const NAV_BUTTON_CORNER_RADIUS: f32 = 20.0;
    /// Design System — Color-dark treatment corner radius. See `advance_theme`.
    const NAV_BUTTON_CORNER_RADIUS_DARK: f32 = 30.0;
    const NAV_BUTTON_FALLBACK_SIZE: Vec2 = Vec2::new(150.0, 46.0);

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
        let grid_w = GALLERY_COLS as f32 * cell + (GALLERY_COLS - 1) as f32 * GALLERY_GAP_PX;
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
        /// to the nav buttons (three row-grouped `NToOneRequest`s at once).
        Gallery,
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
            self.advance_gallery_fetch_button_hover(dt);
            self.advance_demo(dt);
            self.advance_nav_icons(dt);
            self.advance_theme(dt);

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
        #[wasm_bindgen]
        pub fn set_gallery_image(&mut self, tile_idx: u32, bytes: &[u8]) {
            self.ui_world
                .world
                .entity_mut(self.gallery_tiles[tile_idx as usize])
                .insert(Image::new(bytes));
            if let Some(mut qs) = self
                .ui_world
                .world
                .get_mut::<QuadState>(self.gallery_tiles[tile_idx as usize])
            {
                qs.color = white();
            }
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
            // "back" only ever fades in once idle on the video screen —
            // clicking home from there skips straight to a
            // (VideoScreen, Home) transition (see `start_screen_to_nav`),
            // which falls into the `_` arm below without needing a
            // `then_home` flag.
            let back_target: f32 = match (&self.transition, self.state) {
                (None, AppState::VideoScreen(_)) => 1.0,
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
                // every tile has baked.
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
                    } else if clicked.contains(&self.gallery_fetch_button) {
                        self.begin_transition(AppState::Gallery, AppState::Loading);
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
                (AppState::Loading, AppState::Home) | (AppState::Gallery, AppState::Home) => self
                    .nav_buttons
                    .iter()
                    .all(|&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)),
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
                    // Clear any bake left over from whatever fetch preceded
                    // this one — from here on, a tile with a `BakedImage` is
                    // this fetch's own image, not a stale one (see
                    // `start_gallery_to_loading`'s doc for why this can't
                    // happen any earlier: the collapse into `loading_logo`
                    // needs each tile's *current* bake intact when it runs).
                    for &tile in &self.gallery_tiles {
                        self.ui_world
                            .world
                            .entity_mut(tile)
                            .remove::<(BakedImage, TextureRef, Image)>();
                    }
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
        /// `settle_tile_idle`.
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
        /// re-fetches. Any stale bake from a previous gallery visit is
        /// cleared once `settle(AppState::Loading)` runs, not here — this
        /// path's source is the nav buttons, not the gallery tiles, so
        /// there's no bake-timing reason to clear them early.
        fn start_home_to_loading(&mut self) {
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
        /// placeholder instead. `settle(AppState::Loading)` clears the stale
        /// bake once the collapse itself has already captured it.
        fn start_gallery_to_loading(&mut self) {
            self.gallery_fetch_elapsed = 0.0;
            self.gallery_error_shown = false;
            self.hide_gallery_error();
            self.loading_logo_frame_elapsed = 0.0;
            self.loading_logo_frame_index = 0;

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
                if let Some((texture_id, baked)) =
                    self.logo_frames[self.loading_logo_frame_index].clone()
                {
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
        }

        /// Glow + scale-boost hover reaction for gallery tiles — no
        /// overlay-tint/title-label children exist on them (unlike the
        /// video tiles' full `advance_tile_hover`), so this is a smaller,
        /// separate function rather than a generalization of that one.
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
