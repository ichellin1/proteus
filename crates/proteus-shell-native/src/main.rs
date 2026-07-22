//! `proteus-shell-native` — native desktop entry point.
//!
//! ## Demo redesign (in progress)
//!
//! The reference demo is being rebuilt from scratch. This is step 1: the
//! entry screen — a single circular "START" button, centered on screen.
//!
//! - Fades in on load (opacity 0 → 1).
//! - Glows on hover: halo radius animates 0 → 15 px over 1 s while the
//!   pointer is over the button, and reverses (from wherever it currently is)
//!   over 1 s when the pointer leaves.
//!
//! Subsequent steps will add the next scene(s) on top of this.
//!
//! ## Frame order each tick
//!
//! 1. Compute delta time (capped at 50 ms).
//! 2. Flush staged pointer events → `PointerInput`.
//! 3. `ui_world.update(dt)` — full ECS schedule (hit test, transitions).
//! 4. `bake_pending_text()` — rasterise any Text entities that lack BakedText.
//! 5. Advance intro fade + hover glow (drives `QuadState`/`Border`/`Text`/`Glow` directly).
//! 6. Collect visible `QuadState`s → `QuadInstance`s.
//! 7. GPU render pass.

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
    collect_instances, ease_in_out_quad, ease_out_quad, transition::TransitionConfig, BakedText,
    Border, Entity, Glow, GroupTarget, Interactable, InteractionEvents, Lifecycle, OneToNRequest,
    PointerInput, ProteusWorld, QuadState, SplitStrategy, Text, TransitionRequest, VideoPlayer,
    Visibility,
};

// ---------------------------------------------------------------------------
// Design tokens
// ---------------------------------------------------------------------------

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
const TILE_CORNER_RADIUS: f32 = 12.0;

const TILE_LABELS: [&str; 3] = ["Big Buck Bunny", "Sintel", "Jellyfish"];
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

// ---------------------------------------------------------------------------
// Demo scene geometry
// ---------------------------------------------------------------------------

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
const SCREEN_CORNER_RADIUS: f32 = 8.0;

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
fn video_screen_quad(window_width: f32) -> QuadState {
    let width = window_width * SCREEN_WIDTH_FRACTION;
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

// ---------------------------------------------------------------------------
// Demo phase state machine
// ---------------------------------------------------------------------------

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
                let w = state.surface_config.width as f32;
                let h = state.surface_config.height as f32;
                let wx = position.x as f32 - w / 2.0;
                let wy = h / 2.0 - position.y as f32;
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
/// `QuadPipeline` and `GpuContext` live *inside* `ui_world.world` as ECS
/// resources, not as fields here — `topology::one_to_n_setup_system` /
/// `n_to_one_setup_system` need to reach them to bake Slice transitions
/// automatically. `device`/`queue` stay as fields too (cheap clones) for the
/// swapchain/surface work below, which the ECS has no business touching.
struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,

    ui_world: ProteusWorld,
    font_atlas: FontAtlas,

    button: Entity,
    tiles: [Entity; 3],
    phase: DemoPhase,

    // ── demo animation state ───────────────────────────────────────────────
    intro_delay_remaining: f32,
    intro_elapsed: f32,
    hover_progress: f32,
    is_hovering: bool,
    tile_hover_progress: [f32; 3],
    tile_is_hovering: [bool; 3],

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
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
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
        pipeline.set_view_projection(
            &queue,
            QuadPipeline::ortho(size.width as f32, size.height as f32),
        );

        log::info!(
            "Render state ready — {}×{} px, format {:?}",
            size.width,
            size.height,
            surface_format,
        );

        let font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

        let mut ui_world = ProteusWorld::new();
        // GpuContext + QuadPipeline live in the ECS world, not as RenderState
        // fields — this is what lets transition-setup systems bake Slice
        // transitions automatically (see topology::one_to_n_setup_system).
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
                Text::new("START", 36.0).with_color(Vec4::new(
                    white().x,
                    white().y,
                    white().z,
                    0.0, // fades in with the rest of the button
                )),
                Interactable,
                start_button_border(),
                hover_glow(),
            ))
            .id();

        // Tiles start hidden; text is pre-baked while hidden so it's ready
        // the instant the button-spread transition reveals them.
        let tiles = [
            ui_world
                .world
                .spawn((
                    tile_quad(0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(TILE_LABELS[0], 20.0).with_color(white()),
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
                    Text::new(TILE_LABELS[1], 20.0).with_color(white()),
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
                    Text::new(TILE_LABELS[2], 20.0).with_color(white()),
                    Interactable,
                    tile_border(),
                    hover_glow(),
                ))
                .id(),
        ];

        log::info!("Demo entities — button {:?}, tiles {:?}", button, tiles);

        Self {
            window,
            surface,
            surface_config,
            device,
            queue,
            ui_world,
            font_atlas,
            button,
            tiles,
            phase: DemoPhase::ButtonIdle,
            intro_delay_remaining: INTRO_DELAY,
            intro_elapsed: 0.0,
            hover_progress: 0.0,
            is_hovering: false,
            tile_hover_progress: [0.0; 3],
            tile_is_hovering: [false; 3],
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
        self.ui_world
            .world
            .resource::<QuadPipeline>()
            .set_view_projection(
                &self.queue,
                QuadPipeline::ortho(size.width as f32, size.height as f32),
            );
    }

    // -------------------------------------------------------------------------
    // Text bake pass
    // -------------------------------------------------------------------------

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
                log::warn!("FontAtlas: could not bake '{content}' for entity {entity:?}");
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

    // -------------------------------------------------------------------------
    // Intro fade + hover glow
    // -------------------------------------------------------------------------

    /// Advances the one-shot entry fade and the hover glow sweep, then writes
    /// the results directly onto the button's `QuadState`/`Border`/`Text`/`Glow`
    /// components.
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

        // --- Hover glow ---
        // `InteractionEvents` only reports *changes* (enter/exit this frame),
        // so `is_hovering` latches that into persistent state between events.
        {
            let events = self.ui_world.world.resource::<InteractionEvents>();
            if events.hover_entered.contains(&self.button) {
                self.is_hovering = true;
            } else if events.hover_exited.contains(&self.button) {
                self.is_hovering = false;
            }
        }
        // A full 0→1 (or 1→0) sweep takes GLOW_DURATION seconds; reversing
        // mid-sweep starts from wherever `hover_progress` currently is.
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
        if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.button) {
            text.color.w = alpha;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(self.button) {
            // The shadow/glow SDF fills the shape's entire interior, not just a
            // ring at the edge — it's normally masked by the opaque main fill
            // sitting on top of it. While the main fill's alpha is below 1
            // (during the intro fade) that masking is incomplete, so scale the
            // glow's own alpha by the same `alpha` to keep it suppressed until
            // the button is actually opaque.
            glow.color.w = alpha;
            glow.radius = self.hover_progress * GLOW_MAX_RADIUS;
        }
    }

    /// Same hover glow sweep as the button, applied to each of the three
    /// tiles. Tiles are always fully opaque once visible (they arrive via
    /// the button-spread morph, not a fade), so unlike
    /// `advance_intro_and_hover` there's no alpha to suppress the glow with.
    fn advance_tile_hover(&mut self, dt: f32) {
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
            let target = if self.tile_is_hovering[i] { 1.0 } else { 0.0 };
            let step = dt / GLOW_DURATION;
            if self.tile_hover_progress[i] < target {
                self.tile_hover_progress[i] = (self.tile_hover_progress[i] + step).min(target);
            } else if self.tile_hover_progress[i] > target {
                self.tile_hover_progress[i] = (self.tile_hover_progress[i] - step).max(target);
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(entity) {
                glow.radius = self.tile_hover_progress[i] * GLOW_MAX_RADIUS;
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
                let all_revealed = self.tiles.iter().all(
                    |&e| matches!(self.ui_world.world.get::<Visibility>(e), Some(v) if v.visible),
                );
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
                    // when the morph finishes — it plays underneath the morph.
                    self.start_video_playback(clicked_idx);
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
            DemoPhase::ScreenIdle { screen_idx } => {
                if clicked.contains(&self.tiles[screen_idx]) {
                    self.stop_video_playback();
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
                // completes (delay = full morph duration), so the phase isn't
                // done until that fade has finished too.
                let fade_total = BUTTON_TILES_MORPH_DURATION + BUTTON_TILES_MORPH_DURATION * 0.5;
                let done = matches!(lifecycle, Some(Lifecycle::Idle)) && elapsed >= fade_total;
                if done {
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
            .map(|i| GroupTarget {
                entity: self.tiles[i],
                state: tile_quad(i),
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
        let window_width = self.surface_config.width as f32;
        self.ui_world
            .world
            .entity_mut(self.tiles[clicked_idx])
            .insert(TransitionRequest {
                to: video_screen_quad(window_width),
                config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                from_state: None,
            });
    }

    /// Direct 1→1 morph, reversed: `tiles[screen_idx]` returns to its own
    /// `tile_quad` shape.
    fn start_screen_to_tiles(&mut self, screen_idx: usize) {
        self.ui_world
            .world
            .entity_mut(self.tiles[screen_idx])
            .insert(TransitionRequest {
                to: tile_quad(screen_idx),
                config: TransitionConfig {
                    duration: BUTTON_TILES_MORPH_DURATION,
                    delay: 0.0,
                    easing: ease_in_out_quad,
                },
                from_state: None,
            });
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
            .insert(VideoPlayer);

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
            .remove::<VideoPlayer>();
        self.ui_world
            .world
            .resource_mut::<QuadPipeline>()
            .suspend_video(&self.device, playing.texture_id);
        self.present_timing.reset();
    }

    /// Drives the manual fades that accompany `start_tiles_to_screen`'s
    /// `TransitionRequest` (which only lerps `QuadState` — position, size,
    /// fill color, corner radius — and leaves `Border`/`Text`/`Glow` alone):
    /// the morphing tile's label fades out over the full morph duration, and
    /// the other two tiles fade out completely (fill, border, label, glow)
    /// over half the duration.
    fn advance_tiles_to_screen_fade(&mut self, clicked_idx: usize, elapsed: f32) {
        let label_t = (elapsed / BUTTON_TILES_MORPH_DURATION).clamp(0.0, 1.0);
        let label_alpha = 1.0 - ease_out_quad(label_t);
        if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.tiles[clicked_idx]) {
            text.color.w = label_alpha;
        }
        if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(self.tiles[clicked_idx]) {
            glow.radius = 0.0;
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
            if let Some(mut text) = self.ui_world.world.get_mut::<Text>(tile) {
                text.color.w = fade_alpha;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
                glow.radius = 0.0;
                glow.color.w = fade_alpha;
            }
        }
    }

    /// Mirror image of `advance_tiles_to_screen_fade` for the reverse morph:
    /// the returning tile's label fades back in over the full duration, and
    /// the other two tiles fade back in over the first half.
    fn advance_screen_to_tiles_fade(&mut self, screen_idx: usize, elapsed: f32) {
        let label_t = (elapsed / BUTTON_TILES_MORPH_DURATION).clamp(0.0, 1.0);
        let label_alpha = ease_out_quad(label_t);
        if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.tiles[screen_idx]) {
            text.color.w = label_alpha;
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
            if let Some(mut text) = self.ui_world.world.get_mut::<Text>(tile) {
                text.color.w = fade_alpha;
            }
            if let Some(mut glow) = self.ui_world.world.get_mut::<Glow>(tile) {
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
        self.advance_intro_and_hover(dt);
        self.advance_tile_hover(dt);
        self.advance_demo(dt);

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
