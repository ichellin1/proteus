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
//! never drift (see PLANNING.md's M9.6 note on a prior divergence): a
//! circular "START" button that fades in and glows on hover, splits into
//! three video tiles on click (1→N Slice, baked crossfade), and — on
//! clicking any tile — morphs that tile directly into a video screen (plain
//! 1→1) while the other two fade out, reversing the same way on click.

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

    use proteus_render::{FontAtlas, GpuContext, QuadPipeline, TextureId, MAIN_ATLAS_SIZE};
    use proteus_ui::{
        collect_instances, ease_in_out_quad, ease_out_quad, transition::TransitionConfig,
        BakedImage, BakedText, Border, ChildOf, Entity, Glow, GroupTarget, Image, Interactable,
        InteractionEvents, Lifecycle, OneToNRequest, PointerInput, ProteusWorld, QuadState,
        SplitStrategy, Text, TransitionRequest, VideoCrossfade, VideoPlayer, Visibility,
    };

    // -------------------------------------------------------------------------
    // Design tokens  (identical to proteus-shell-native)
    // -------------------------------------------------------------------------

    /// App background — mid gray (#BBBBBB).
    const BG_COLOR: wgpu::Color = wgpu::Color {
        r: 0xBB as f64 / 255.0,
        g: 0xBB as f64 / 255.0,
        b: 0xBB as f64 / 255.0,
        a: 1.0,
    };

    /// Button fill — navy blue.
    fn navy() -> Vec4 {
        Vec4::new(0.0, 0.0, 0.502, 1.0)
    }

    /// Border and label color — white.
    fn white() -> Vec4 {
        Vec4::ONE
    }

    const BUTTON_DIAMETER: f32 = 200.0;
    const BORDER_WIDTH: f32 = 5.0;

    /// Seconds to wait before the entry fade begins.
    const INTRO_DELAY: f32 = 1.0;
    /// Seconds for the entry fade (opacity 0 → 1).
    const INTRO_DURATION: f32 = 0.6;
    /// Seconds for a full 0 → 30 px (or 30 → 0 px) hover glow sweep.
    const GLOW_DURATION: f32 = 0.33;
    const GLOW_MAX_RADIUS: f32 = 30.0;

    /// Seconds for the button ↔ tiles morph, either direction.
    const BUTTON_TILES_MORPH_DURATION: f32 = 0.667;

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
    const TILE_CORNER_RADIUS: f32 = 12.0;

    const TILE_COLORS: [Vec4; 3] = [
        Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Big Buck Bunny
        Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
        Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
    ];

    /// Real box-cover photos are routinely far larger than the tiles'
    /// on-screen footprint (200×300) — cap decoded images to this before
    /// packing them into `main_atlas` (2048×2048, shared with baked text),
    /// which they'd otherwise not fit in at all or would starve of remaining
    /// space. 2x the tile height leaves headroom for high-DPI displays.
    const MAX_TILE_IMAGE_SIDE: u32 = 600;

    /// Title shown in the hover overlay (M10) — each tile's `tile_labels[idx]`
    /// `Text` child.
    const TILE_TITLES: [&str; 3] = ["Big Buck Bunny", "Sintel", "Jellyfish"];

    /// Hover-overlay label size.
    const TILE_LABEL_SIZE_PX: f32 = 20.0;

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
    const TILE_OVERLAY_MAX_ALPHA: f32 = 0.55;

    // -------------------------------------------------------------------------
    // Demo scene geometry  (identical to proteus-shell-native)
    // -------------------------------------------------------------------------

    /// The circular "START" button — perfectly centered, alpha 0 (fades in via
    /// `advance_intro_and_hover`).
    fn start_button_quad() -> QuadState {
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(BUTTON_DIAMETER, BUTTON_DIAMETER),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(navy().x, navy().y, navy().z, 0.0), // starts transparent
            // Half the diameter → a square quad with this corner radius renders as
            // a full circle (see sdf_rounded_rect).
            corner_radius: BUTTON_DIAMETER / 2.0,
        }
    }

    fn start_button_border() -> Border {
        Border {
            width: BORDER_WIDTH,
            color: Vec4::new(white().x, white().y, white().z, 0.0),
            offset: -1.0, // inner — the only placement that renders correctly today
        }
    }

    /// Shared by the button and the tiles — "the same hover over/out effect".
    fn hover_glow() -> Glow {
        Glow {
            radius: 0.0, // animated by advance_intro_and_hover / advance_tile_hover
            color: navy(),
            intensity: 0.8,
        }
    }

    /// One of the three video tiles the button spreads into.
    /// `idx` 0 = left, 1 = center, 2 = right.
    fn tile_quad(idx: usize) -> QuadState {
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
            corner_radius: TILE_CORNER_RADIUS,
        }
    }

    /// Same white inner border as the START button. Full alpha immediately —
    /// tiles appear via the button-spread morph, not a separate fade.
    fn tile_border() -> Border {
        Border {
            width: BORDER_WIDTH,
            color: white(),
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
    const SCREEN_CORNER_RADIUS: f32 = 8.0;

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
    fn video_screen_quad(canvas_width: f32) -> QuadState {
        let width = canvas_width * SCREEN_WIDTH_FRACTION;
        let height = width * SCREEN_ASPECT;
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
    // Demo phase state machine
    // -------------------------------------------------------------------------

    /// Click-driven demo phases. Idle phases wait for user input; the transition
    /// phase is in-flight and tracks elapsed time to drive the manual fades that
    /// run alongside the framework-driven `TransitionRequest` morph.
    enum DemoPhase {
        /// Button visible — click it to spread into the three video tiles.
        ButtonIdle,
        /// 1→N Slice in progress: button splitting into three slices that morph
        /// into the tiles.
        ButtonToTiles,
        /// Three tiles visible — click any tile to converge into the video screen.
        TilesIdle,
        /// The clicked tile (`tiles[clicked_idx]`) is morphing into the video
        /// screen while the other two fade out; `elapsed` drives the manual fade.
        TilesToScreen { clicked_idx: usize, elapsed: f32 },
        /// Video screen visible (as `tiles[screen_idx]`) — click it to morph back.
        ScreenIdle { screen_idx: usize },
        /// `tiles[screen_idx]` is morphing back into its tile shape while the
        /// other two fade back in; `elapsed` drives the manual fade.
        ScreenToTiles { screen_idx: usize, elapsed: f32 },
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
        // `QuadPipeline`/`GpuContext` live inside `ui_world.world` as ECS
        // resources, not as fields here — see proteus-shell-native's
        // RenderState doc comment for why (lets transition-setup systems
        // bake Slice transitions automatically).
        ui_world: ProteusWorld,
        font_atlas: FontAtlas,

        button: Entity,
        /// The button's "START" label — a `Text` child entity (M10), not a
        /// component on the button entity itself. See its spawn site for why.
        button_label: Entity,
        tiles: [Entity; 3],
        /// Per-tile black hover overlay — a `Quad` child of `tiles[i]` (M10).
        tile_overlays: [Entity; 3],
        /// Per-tile title label — a `Text` child of `tiles[i]` (M10).
        tile_labels: [Entity; 3],
        phase: DemoPhase,

        staged_pointer: StagedPointer,

        // ── demo animation state ───────────────────────────────────────────
        intro_delay_remaining: f32,
        intro_elapsed: f32,
        hover_progress: f32,
        is_hovering: bool,
        tile_hover_progress: [f32; 3],
        tile_is_hovering: [bool; 3],

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
            let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096);
            pipeline.set_view_projection(&queue, QuadPipeline::ortho(width as f32, height as f32));

            log::info!(
                "GPU ready — {}×{} px, format {:?}",
                width,
                height,
                surface_format,
            );

            // Font atlas.
            let font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

            // ECS world + demo entities.
            let mut ui_world = ProteusWorld::new();
            ui_world.world.insert_resource(GpuContext {
                device: device.clone(),
                queue: queue.clone(),
            });
            ui_world.world.insert_resource(pipeline);

            let button = ui_world
                .world
                .spawn((
                    start_button_quad(),
                    Lifecycle::Idle,
                    Visibility::VISIBLE,
                    Interactable,
                    start_button_border(),
                    hover_glow(),
                ))
                .id();

            // The "START" label (M10): a `Quad` parent (the button, above)
            // containing a `Text` child — the composition model this
            // milestone introduces, replacing the M5 shortcut where Text
            // lived on the same entity as its container. The child's
            // QuadState is declared relative to the button's origin (zero
            // offset — centered on the button, same as the old single-entity
            // layout) with fully transparent color so its own background
            // instance (every text-bearing entity emits one, per the
            // two-instance render model) stays invisible; only the BakedText
            // overlay — tinted by `Text::color` below — is visible.
            let button_label = ui_world
                .world
                .spawn((
                    QuadState {
                        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                        ..Default::default()
                    },
                    Lifecycle::Idle,
                    Visibility::VISIBLE,
                    Text::new("START", 36.0).with_color(Vec4::new(
                        white().x,
                        white().y,
                        white().z,
                        0.0, // fades in with the rest of the button
                    )),
                    ChildOf(button),
                ))
                .id();

            // Tiles start hidden; box-cover art (fetched separately, see
            // set_tile_image) makes a text label redundant, so tiles carry
            // no Text component.
            let tiles = [
                ui_world
                    .world
                    .spawn((
                        tile_quad(0),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Interactable,
                        tile_border(),
                        hover_glow(),
                    ))
                    .id(),
                ui_world
                    .world
                    .spawn((
                        tile_quad(1),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Interactable,
                        tile_border(),
                        hover_glow(),
                    ))
                    .id(),
                ui_world
                    .world
                    .spawn((
                        tile_quad(2),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Interactable,
                        tile_border(),
                        hover_glow(),
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
                            .with_color(Vec4::new(1.0, 1.0, 1.0, 0.0)), // alpha animated by advance_tile_hover
                        ChildOf(tile),
                    ))
                    .id();
            }

            log::info!(
                "Demo entities — button {:?}, button_label {:?}, tiles {:?}, tile_overlays {:?}, tile_labels {:?}",
                button,
                button_label,
                tiles,
                tile_overlays,
                tile_labels
            );

            Ok(ProteusApp {
                surface,
                surface_config,
                device,
                queue,
                ui_world,
                font_atlas,
                button,
                button_label,
                tiles,
                tile_overlays,
                tile_labels,
                phase: DemoPhase::ButtonIdle,
                staged_pointer: StagedPointer::default(),
                intro_delay_remaining: INTRO_DELAY,
                intro_elapsed: 0.0,
                hover_progress: 0.0,
                is_hovering: false,
                tile_hover_progress: [0.0; 3],
                tile_is_hovering: [false; 3],
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
            self.advance_intro_and_hover(dt);
            self.advance_tile_hover(dt);
            self.advance_demo(dt);

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
    }

    // ── private helpers ────────────────────────────────────────────────────────

    impl ProteusApp {
        /// For every entity with `Text` but no `BakedText`: rasterise → upload to
        /// main_atlas → insert `BakedText`.
        fn bake_pending_text(&mut self) {
            let all_text: Vec<(Entity, String, f32)> = {
                let mut q = self.ui_world.world.query::<(Entity, &Text)>();
                q.iter(&self.ui_world.world)
                    .map(|(e, t)| (e, t.content.clone(), t.size_px))
                    .collect()
            };
            let pending: Vec<(Entity, String, f32)> = all_text
                .into_iter()
                .filter(|(e, _, _)| self.ui_world.world.get::<BakedText>(*e).is_none())
                .collect();

            for (entity, content, size_px) in pending {
                let Some(region) = self.font_atlas.bake_text(&content, size_px) else {
                    log::warn!("FontAtlas: could not bake '{content}'");
                    continue;
                };
                self.ui_world
                    .world
                    .resource::<QuadPipeline>()
                    .write_to_main_atlas(
                        &self.queue,
                        region.x,
                        region.y,
                        region.width,
                        region.height,
                        &region.rgba_pixels,
                    );
                let uv_offset = region.uv_offset(MAIN_ATLAS_SIZE);
                let uv_scale = region.uv_scale(MAIN_ATLAS_SIZE);
                self.ui_world.world.entity_mut(entity).insert(BakedText {
                    uv_offset,
                    uv_scale,
                    pixel_size: [region.width as f32, region.height as f32],
                });
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

                let Some(region) =
                    self.font_atlas
                        .bake_image(&decoded.rgba_pixels, decoded.width, decoded.height)
                else {
                    log::warn!(
                        "bake_pending_images: atlas full — could not bake {}×{} image",
                        decoded.width,
                        decoded.height,
                    );
                    continue;
                };
                self.ui_world
                    .world
                    .resource::<QuadPipeline>()
                    .write_to_main_atlas(
                        &self.queue,
                        region.x,
                        region.y,
                        region.width,
                        region.height,
                        &region.rgba_pixels,
                    );
                let uv_offset = region.uv_offset(MAIN_ATLAS_SIZE);
                let uv_scale = region.uv_scale(MAIN_ATLAS_SIZE);
                self.ui_world.world.entity_mut(entity).insert(BakedImage {
                    uv_offset,
                    uv_scale,
                    pixel_size: [region.width as f32, region.height as f32],
                });
            }
        }

        /// Advances the one-shot entry fade and the hover glow sweep, then
        /// writes the results directly onto the button's
        /// `QuadState`/`Border`/`Text`/`Glow` components.
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

            // --- Hover glow ---
            // `InteractionEvents` only reports *changes* (enter/exit this
            // frame), so `is_hovering` latches that into persistent state
            // between events.
            {
                let events = self.ui_world.world.resource::<InteractionEvents>();
                if events.hover_entered.contains(&self.button) {
                    self.is_hovering = true;
                } else if events.hover_exited.contains(&self.button) {
                    self.is_hovering = false;
                }
            }
            // A full 0→1 (or 1→0) sweep takes GLOW_DURATION seconds;
            // reversing mid-sweep starts from wherever `hover_progress`
            // currently is.
            let target = if self.is_hovering { 1.0 } else { 0.0 };
            let step = dt / GLOW_DURATION;
            if self.hover_progress < target {
                self.hover_progress = (self.hover_progress + step).min(target);
            } else if self.hover_progress > target {
                self.hover_progress = (self.hover_progress - step).max(target);
            }

            if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(self.button) {
                qs.color.w = alpha;
            }
            if let Some(mut border) = self.ui_world.world.get_mut::<Border>(self.button) {
                border.color.w = alpha;
            }
            if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.button_label) {
                text.color.w = alpha;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(self.button) {
                // The shadow/glow SDF fills the shape's entire interior, not
                // just a ring at the edge — it's normally masked by the
                // opaque main fill sitting on top of it. While the main
                // fill's alpha is below 1 (during the intro fade) that
                // masking is incomplete, so scale the glow's own alpha by
                // the same `alpha` to keep it suppressed until the button is
                // actually opaque.
                glow.color.w = alpha;
                glow.radius = self.hover_progress * GLOW_MAX_RADIUS;
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
            let transitioning = matches!(
                self.phase,
                DemoPhase::TilesToScreen { .. } | DemoPhase::ScreenToTiles { .. }
            );
            for i in 0..3 {
                let entity = self.tiles[i];
                {
                    let events = self.ui_world.world.resource::<InteractionEvents>();
                    if events.hover_entered.contains(&entity) {
                        self.tile_is_hovering[i] = true;
                    } else if events.hover_exited.contains(&entity) {
                        self.tile_is_hovering[i] = false;
                    }
                }
                let target = if transitioning {
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
                // `self.phase` rather than tile geometry directly: the label's
                // alpha is already forced to zero for this tile during the
                // in-flight morph, so there's nothing to interpolate — it only
                // needs the right value once settled into `TilesIdle` or
                // `ScreenIdle`.
                let label_scale = match self.phase {
                    DemoPhase::ScreenIdle { screen_idx } if screen_idx == i => {
                        TILE_LABEL_SCREEN_SCALE
                    }
                    DemoPhase::TilesToScreen { clicked_idx, .. } if clicked_idx == i => {
                        TILE_LABEL_SCREEN_SCALE
                    }
                    DemoPhase::ScreenToTiles { screen_idx, .. } if screen_idx == i => {
                        TILE_LABEL_SCREEN_SCALE
                    }
                    _ => 1.0,
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

        // -------------------------------------------------------------------------
        // Demo state machine
        // -------------------------------------------------------------------------

        /// Advance the demo one frame: read `InteractionEvents` (populated by
        /// `hit_test_system` during `ui_world.update()`) and drive `DemoPhase` by
        /// click. `dt` drives the manual tile↔screen fade timers.
        fn advance_demo(&mut self, dt: f32) {
            let clicked: Vec<Entity> = self
                .ui_world
                .world
                .resource::<InteractionEvents>()
                .clicked
                .clone();

            let phase = std::mem::replace(&mut self.phase, DemoPhase::ButtonIdle);

            self.phase = match phase {
                // ── Button idle: click to spread into the three tiles ──────────
                DemoPhase::ButtonIdle => {
                    if clicked.contains(&self.button) {
                        self.start_button_to_tiles();
                        DemoPhase::ButtonToTiles
                    } else {
                        DemoPhase::ButtonIdle
                    }
                }

                // ── 1→N Slice: wait for the tiles to be revealed ────────────────
                // Slice transitions run through virtual entities and a group
                // coordinator (see `topology::group_transition_complete_system`),
                // not the per-entity `CompletedTransitions` list — the tiles are
                // held `Visibility::HIDDEN` until the whole group finishes, so
                // watching for that flip is the direct signal.
                DemoPhase::ButtonToTiles => {
                    let all_revealed = self.tiles.iter().all(|&e| {
                        matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible)
                    });
                    if all_revealed {
                        DemoPhase::TilesIdle
                    } else {
                        DemoPhase::ButtonToTiles
                    }
                }

                // ── Tiles idle: click a tile to morph it into the video screen ──
                DemoPhase::TilesIdle => {
                    if let Some(clicked_idx) = self.tiles.iter().position(|e| clicked.contains(e)) {
                        self.start_tiles_to_screen(clicked_idx);
                        // Playback starts the instant the tile is clicked, not
                        // when the morph finishes — it plays underneath the
                        // morph. JS polls `take_video_start_tile` once per
                        // tick and, once the browser's <video> element has
                        // loaded metadata, calls back into `start_video`.
                        self.pending_video_start = Some(clicked_idx as u32);
                        DemoPhase::TilesToScreen {
                            clicked_idx,
                            elapsed: 0.0,
                        }
                    } else {
                        DemoPhase::TilesIdle
                    }
                }

                // ── Clicked tile morphs into the screen; the other two fade out ─
                // The morph itself is a plain `TransitionRequest` on
                // `tiles[clicked_idx]`, ticked by the framework during
                // `ui_world.update()`; only the label/other-tile fades are driven
                // here, from the same elapsed clock so they stay in lockstep.
                DemoPhase::TilesToScreen {
                    clicked_idx,
                    elapsed,
                } => {
                    let elapsed = elapsed + dt;
                    self.advance_tiles_to_screen_fade(clicked_idx, elapsed);
                    let morphing = self.tiles[clicked_idx];
                    let lifecycle = self.ui_world.world.get::<Lifecycle>(morphing);
                    let done = matches!(lifecycle, Some(Lifecycle::Idle))
                        && elapsed >= BUTTON_TILES_MORPH_DURATION;
                    if done {
                        DemoPhase::ScreenIdle {
                            screen_idx: clicked_idx,
                        }
                    } else {
                        DemoPhase::TilesToScreen {
                            clicked_idx,
                            elapsed,
                        }
                    }
                }

                // ── Screen idle: click it to morph back into its tile shape ────
                // Playback keeps running through the reverse morph — see
                // advance_screen_to_tiles_fade — and only actually stops once
                // that morph completes, below, so the video crossfades live
                // into the box art on the way back instead of cutting instantly.
                DemoPhase::ScreenIdle { screen_idx } => {
                    if clicked.contains(&self.tiles[screen_idx]) {
                        self.start_screen_to_tiles(screen_idx);
                        DemoPhase::ScreenToTiles {
                            screen_idx,
                            elapsed: 0.0,
                        }
                    } else {
                        DemoPhase::ScreenIdle { screen_idx }
                    }
                }

                // ── Screen morphs back into its tile; the other two fade back in ─
                DemoPhase::ScreenToTiles {
                    screen_idx,
                    elapsed,
                } => {
                    let elapsed = elapsed + dt;
                    self.advance_screen_to_tiles_fade(screen_idx, elapsed);
                    let morphing = self.tiles[screen_idx];
                    let lifecycle = self.ui_world.world.get::<Lifecycle>(morphing);
                    // The other two tiles' fade-in now runs after the morph
                    // completes (delay = full morph duration), so the phase
                    // isn't done until that fade has finished too.
                    let fade_total =
                        BUTTON_TILES_MORPH_DURATION + BUTTON_TILES_MORPH_DURATION * 0.5;
                    let done = matches!(lifecycle, Some(Lifecycle::Idle)) && elapsed >= fade_total;
                    if done {
                        self.stop_video();
                        self.pending_video_stop = true;
                        DemoPhase::TilesIdle
                    } else {
                        DemoPhase::ScreenToTiles {
                            screen_idx,
                            elapsed,
                        }
                    }
                }
            };
        }

        /// 1→N Slice: the button splits into three vertical slices that each
        /// morph into their own tile. The baked-texture crossfade (each slice
        /// showing an actual crop of the button's rendered appearance — shape,
        /// border, and text — dissolving into a real bake of its target tile,
        /// not a flat-color approximation) is entirely `one_to_n_setup_system`'s
        /// job now: it reaches `GpuContext`/`QuadPipeline` as ECS resources and
        /// does the baking itself. This just declares the request.
        fn start_button_to_tiles(&mut self) {
            let targets = (0..3)
                .map(|i| {
                    let mut state = tile_quad(i);
                    // tile_quad() always bakes in the placeholder TILE_COLORS
                    // tint — fine before any image has loaded, but wrong once
                    // one has: each virtual slice's QuadState.color lerps
                    // toward this target state over the whole transition, so
                    // without this override the box art would render visibly
                    // tinted throughout the morph even though the real tile
                    // (already corrected to white() at load) shows it
                    // untinted the moment it's revealed.
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

        /// Direct 1→1 morph: `tiles[clicked_idx]` becomes the video screen. A
        /// plain `TransitionRequest` on the tile itself — no group/slice
        /// machinery, since only one entity is changing shape.
        fn start_tiles_to_screen(&mut self, clicked_idx: usize) {
            let canvas_width = self.surface_config.width as f32;
            self.ui_world
                .world
                .entity_mut(self.tiles[clicked_idx])
                .insert(TransitionRequest {
                    to: video_screen_quad(canvas_width),
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

        /// Direct 1→1 morph, reversed: `tiles[screen_idx]` returns to its own
        /// `tile_quad` shape.
        fn start_screen_to_tiles(&mut self, screen_idx: usize) {
            let mut to = tile_quad(screen_idx);
            // tile_quad() always bakes in the placeholder TILE_COLORS tint —
            // fine the first time (nothing has loaded yet), but wrong on
            // every replay: this TransitionRequest lerps QuadState.color all
            // the way to `to`, so without this override the reverse morph
            // would drag a tile whose box art already loaded back to its
            // placeholder tint, undoing the one-time white() set at load.
            if self
                .ui_world
                .world
                .get::<BakedImage>(self.tiles[screen_idx])
                .is_some()
            {
                to.color = white();
            }
            self.ui_world
                .world
                .entity_mut(self.tiles[screen_idx])
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

        /// Mirror image of `advance_tiles_to_screen_fade` for the reverse morph:
        /// the other two tiles fade back in over the first half.
        fn advance_screen_to_tiles_fade(&mut self, screen_idx: usize, elapsed: f32) {
            let t = (elapsed / BUTTON_TILES_MORPH_DURATION).clamp(0.0, 1.0);
            // M10: see the matching comment in advance_tiles_to_screen_fade — hover
            // overlay + title label are deactivated for every tile for the
            // duration of this morph too.
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
            // Live crossfade (M9.8), reversed: video → box art. Same easing
            // as the forward direction, counting down from 1.0 instead of up
            // from 0.0 — see push_entity_instances in proteus-ui's collect.rs
            // for why this works out correctly without inverting the
            // geometry morph's easing.
            if let Some(mut crossfade) = self
                .ui_world
                .world
                .get_mut::<VideoCrossfade>(self.tiles[screen_idx])
            {
                crossfade.video_t = 1.0 - ease_in_out_quad(t);
            }

            // The other two tiles wait out a delay matching the full morph
            // duration (staying invisible until the morphing tile has fully
            // settled back into shape) before fading in over half that duration.
            let fade_delay = BUTTON_TILES_MORPH_DURATION;
            let fade_duration = BUTTON_TILES_MORPH_DURATION * 0.5;
            let fade_t = ((elapsed - fade_delay) / fade_duration).clamp(0.0, 1.0);
            let fade_alpha = ease_out_quad(fade_t);
            for (i, &tile) in self.tiles.iter().enumerate() {
                if i == screen_idx {
                    continue;
                }
                if let Some(mut qs) = self.ui_world.world.get_mut::<QuadState>(tile) {
                    qs.color.w = fade_alpha;
                }
                if let Some(mut border) = self.ui_world.world.get_mut::<Border>(tile) {
                    border.color.w = fade_alpha;
                }
                if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
                    glow.color.w = fade_alpha;
                }
            }
        }
    }
} // mod inner

// Re-export ProteusApp at crate root so wasm-bindgen can generate bindings.
#[cfg(target_arch = "wasm32")]
pub use inner::ProteusApp;
