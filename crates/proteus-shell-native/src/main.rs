//! `proteus-shell-native` — native desktop entry point (M5 reference demo).
//!
//! Runs a scripted, looping transition sequence that exercises all three
//! Proteus topologies back-to-back:
//!
//! ```text
//! ButtonIdle ──(1→N Bake)──► ButtonToList ──► ListIdle
//!      ▲                                           │
//!      │                                     (1→1) │
//!      │                                           ▼
//! ListToButton ◄──(N→1 Slice)── ListIdle ◄── DetailIdle
//!                                               ▲
//!                                         (1→1) │
//!                                         DetailToList
//! ```
//!
//! ## Frame order each tick
//!
//! 1. Compute delta time (capped at 50 ms).
//! 2. `ui_world.update(dt)` — full ECS schedule (Transition + Group systems).
//! 3. `bake_pending_text()` — rasterise any Text entities that lack BakedText.
//! 4. `advance_demo(dt)` — read `CompletedTransitions`, mutate `DemoPhase`.
//! 5. Collect visible `QuadState`s → `QuadInstance`s (visibility-filtered).
//! 6. GPU render pass.

use std::sync::Arc;
use std::time::Instant;

use glam::{Vec2, Vec3, Vec4};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

use std::thread;

use proteus_render::{FontAtlas, QuadPipeline, TextureId, MAIN_ATLAS_SIZE};
use proteus_ui::{
    collect_instances, ease_in_out_quad,
    transition::{CompletedTransitions, TransitionConfig},
    BakedText, DropShadow, Entity, Glow, GroupSource, GroupTarget, Interactable, InteractionEvents,
    Lifecycle, NToOneRequest, OneToNRequest, PointerInput, ProteusWorld, QuadState, SplitStrategy,
    Text, TransitionRequest, VideoPlayer, Visibility,
};


// ---------------------------------------------------------------------------
// Video constants (M9)
// ---------------------------------------------------------------------------

/// Width of the synthetic video texture uploaded per frame.
const VIDEO_W: u32 = 320;
/// Height of the synthetic video texture uploaded per frame.
const VIDEO_H: u32 = 180;

/// Generate one frame of synthetic video: animated sinusoidal colour bands.
/// `t` is elapsed time in seconds. Returns `VIDEO_W × VIDEO_H × 4` RGBA bytes.
fn generate_video_frame(t: f64, width: u32, height: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let ft = t as f32;
    for y in 0..height {
        for x in 0..width {
            let nx = x as f32 / width as f32;
            let ny = y as f32 / height as f32;
            // Bands along X, Y, and diagonal — each at a different phase speed.
            let r = ((nx * 6.0 + ft * 1.1).sin() * 0.5 + 0.5) * 0.8 + 0.1;
            let g = ((ny * 4.0 + ft * 0.7).sin() * 0.5 + 0.5) * 0.8 + 0.1;
            let b = (((nx + ny) * 5.0 + ft * 1.3).sin() * 0.5 + 0.5) * 0.8 + 0.1;
            let i = ((y * width + x) * 4) as usize;
            rgba[i]     = (r * 255.0) as u8;
            rgba[i + 1] = (g * 255.0) as u8;
            rgba[i + 2] = (b * 255.0) as u8;
            rgba[i + 3] = 255;
        }
    }
    rgba
}

// ---------------------------------------------------------------------------
// Demo scene geometry
// ---------------------------------------------------------------------------

/// The central button that starts and ends each loop.
fn button_quad() -> QuadState {
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(200.0, 48.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.37, 0.65, 1.00, 1.0), // sky blue
        corner_radius: 0.0,
    }
}

/// One of the three list items that the button expands into.
/// `idx` 0 = top, 1 = middle, 2 = bottom.
fn item_quad(idx: usize) -> QuadState {
    // Stack vertically: top item at +80, spacing of 72 px.
    let y = 80.0 - idx as f32 * 72.0;
    let color = match idx {
        0 => Vec4::new(0.25, 0.90, 0.60, 1.0), // emerald
        1 => Vec4::new(0.20, 0.80, 0.90, 1.0), // cyan
        _ => Vec4::new(0.60, 0.65, 1.00, 1.0), // lavender
    };
    QuadState {
        position: Vec3::new(0.0, y, 0.5),
        size: Vec2::new(220.0, 44.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color,
        corner_radius: 0.0,
    }
}

/// The detail view that item[0] morphs into.
fn detail_quad() -> QuadState {
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(280.0, 100.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.00, 0.82, 0.28, 1.0), // gold
        corner_radius: 0.0,
    }
}

/// Default 1→1 transition timing used for most phases.
fn demo_config() -> TransitionConfig {
    TransitionConfig {
        duration: 0.60,
        delay: 0.0,
        easing: ease_in_out_quad,
    }
}

// ---------------------------------------------------------------------------
// Demo phase state machine
// ---------------------------------------------------------------------------

/// Click-driven demo phases (M7). Idle phases wait for user input; transition
/// phases are in-flight and block input until they complete.
#[derive(Debug)]
enum DemoPhase {
    /// Button visible — click it to expand.
    ButtonIdle,

    /// 1→N Bake in progress: button → three list items.
    ButtonToList { items_done: usize },

    /// Three items visible — click item[0] (Elephant) to zoom in.
    ListIdle,

    /// 1→1 transition: item[0] → detail view.
    ListToDetail,

    /// Detail view — click anywhere on it to return to list.
    DetailIdle,

    /// 1→1 transition: item[0] → list position.
    DetailToList,

    /// item[0] alone; 1.5 s auto-pause before flanking items appear.
    ListSoloIdle { timer: f32 },

    /// All three items visible — click any item to converge back to button.
    ListReformIdle,

    /// N→1 Slice in progress: items → button.
    ListToButton { request_inserted: bool },
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
    log::info!("Proteus M5 reference demo — native shell");

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
        if self.state.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Proteus — M5 Reference Demo")
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

    /// Release the video texture GPU memory when the app is backgrounded (M9).
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            state.pipeline.suspend_video(&state.device, state.video_id);
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
                // `position` is in physical pixels; `surface_config` dimensions are also
                // physical pixels (set from PhysicalSize in resize()).  `ortho()` is
                // called with physical dimensions, so world-space units = physical pixels.
                // Do NOT divide by scale_factor — that would compress the cursor into
                // logical-pixel space while QuadState positions stay in physical-pixel space.
                let w = state.surface_config.width as f32;
                let h = state.surface_config.height as f32;
                // Convert window-space (origin top-left, Y down) → world-space (origin centre, Y up).
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
struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: QuadPipeline,

    /// Proteus ECS world + schedule.
    ui_world: ProteusWorld,

    /// CPU-side font atlas for rasterising text (M4 / M5).
    font_atlas: FontAtlas,

    // ── demo entities ──────────────────────────────────────────────────────
    /// The central button (always-present, starts visible).
    button: Entity,

    /// The three list items (start hidden; expand from button via 1→N Bake).
    items: [Entity; 3],

    // ── demo state ─────────────────────────────────────────────────────────
    phase: DemoPhase,

    // ── input ──────────────────────────────────────────────────────────────
    staged_pointer: StagedPointer,

    // ── timing ─────────────────────────────────────────────────────────────
    last_frame: Instant,

    // ── video (M9) ────────────────────────────────────────────────────────────
    /// TextureId returned by `pipeline.init_video` — used for suspend/resume.
    video_id: TextureId,
}

impl RenderState {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        // --- wgpu instance ---
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // --- Surface ---
        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        // --- Adapter ---
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter found");

        log::info!("GPU adapter: {}", adapter.get_info().name);

        // --- Device & queue ---
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("proteus-native"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .expect("failed to create GPU device");

        // --- Surface configuration ---
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

        // --- Pipeline ---
        let mut pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096);
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

        // --- Font atlas ---
        let font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

        // --- Video texture (M9) ---
        // Allocate the video texture slot at 320×180.  The sender is moved into
        // a background thread that generates synthetic colour-band frames at 30 fps.
        // The render loop calls consume_video_frame() to upload the latest frame.
        let (video_id, video_sender) = pipeline.init_video(&device, VIDEO_W, VIDEO_H);

        // Spawn the synthetic video producer on a background thread.
        // Replace the body of this closure with any real decoder you like.
        //
        // No sleep here: sync_channel(2) provides natural backpressure.  When
        // the render loop hasn't consumed yet, send() blocks — so the producer
        // is gated by the display refresh automatically, with no hardcoded rate.
        // Animation time comes from the wall clock so speed is always correct.
        thread::spawn(move || {
            let start = std::time::Instant::now();
            loop {
                let t = start.elapsed().as_secs_f64();
                let frame = generate_video_frame(t, VIDEO_W, VIDEO_H);
                // Blocks when the 2-frame buffer is full (backpressure).
                // Returns false if the pipeline has been dropped — exit cleanly.
                if !video_sender.send(frame) {
                    break;
                }
            }
        });

        // --- ECS world ---
        let mut ui_world = ProteusWorld::new();

        // Button: starts visible, shows "View Items" label.
        // Interactable marks it as a hit-test target.
        let button = ui_world
            .world
            .spawn((
                button_quad(),
                Lifecycle::Idle,
                Visibility::VISIBLE,
                Text::new("View Items", 22.0),
                Interactable,
                DropShadow {
                    offset: Vec2::new(4.0, -4.0),
                    color: Vec4::new(0.0, 0.0, 0.0, 0.45),
                    softness: 13.0,
                    spread: 0.0,
                },
                Glow {
                    radius: 17.0,
                    color: Vec4::new(0.37, 0.65, 1.0, 1.0), // sky blue — matches button fill
                    intensity: 0.7,
                },
            ))
            .id();

        // List items: start hidden; text is pre-baked even while hidden so it
        // is ready the instant the items become visible.
        let item_labels = ["Elephant", "Tiger", "Whale"];
        let items = [
            ui_world
                .world
                .spawn((
                    item_quad(0),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(item_labels[0], 18.0),
                    Interactable,
                    Glow {
                        radius: 17.0,
                        color: Vec4::new(1.00, 0.82, 0.28, 1.0), // gold — matches detail view fill
                        intensity: 0.7,
                    },
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    item_quad(1),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(item_labels[1], 18.0),
                    Interactable,
                    Glow {
                        radius: 17.0,
                        color: Vec4::new(0.20, 0.80, 0.90, 1.0), // cyan — matches item[1] fill
                        intensity: 0.7,
                    },
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    // item[2] streams video — set color to white so the frame is
                    // displayed unfiltered.  The label "Whale" floats on top.
                    QuadState { color: Vec4::ONE, ..item_quad(2) },
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(item_labels[2], 18.0),
                    Interactable,
                    VideoPlayer,
                    Glow {
                        radius: 17.0,
                        color: Vec4::new(0.60, 0.65, 1.00, 1.0), // lavender glow
                        intensity: 0.7,
                    },
                ))
                .id(),
        ];

        log::info!("Demo entities — button {:?}, items {:?}", button, items);

        Self {
            window,
            surface,
            surface_config,
            device,
            queue,
            pipeline,
            ui_world,
            font_atlas,
            button,
            items,
            phase: DemoPhase::ButtonIdle,
            staged_pointer: StagedPointer::default(),
            last_frame: Instant::now(),
            video_id,
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
        self.pipeline.set_view_projection(
            &self.queue,
            QuadPipeline::ortho(size.width as f32, size.height as f32),
        );
    }

    // -------------------------------------------------------------------------
    // Text bake pass (M4 / M5)
    // -------------------------------------------------------------------------

    /// For every entity with `Text` but no `BakedText`:
    /// rasterise → upload to main_atlas → insert `BakedText`.
    ///
    /// Processes hidden entities too, so text is ready before items become visible.
    fn bake_pending_text(&mut self) {
        // Step 1: collect (entity, content, size_px) — ends the query borrow.
        let all_text: Vec<(Entity, String, f32)> = {
            let mut q = self.ui_world.world.query::<(Entity, &Text)>();
            q.iter(&self.ui_world.world)
                .map(|(e, t)| (e, t.content.clone(), t.size_px))
                .collect()
        };

        // Step 2: filter to those missing BakedText.
        let pending: Vec<(Entity, String, f32)> = all_text
            .into_iter()
            .filter(|(e, _, _)| self.ui_world.world.get::<BakedText>(*e).is_none())
            .collect();

        for (entity, content, size_px) in pending {
            let Some(region) = self.font_atlas.bake_text(&content, size_px) else {
                log::warn!("FontAtlas: could not bake '{content}' for entity {entity:?}");
                continue;
            };

            self.pipeline.write_to_main_atlas(
                &self.queue,
                region.x,
                region.y,
                region.width,
                region.height,
                &region.rgba_pixels,
            );

            let uv_offset = region.uv_offset(MAIN_ATLAS_SIZE);
            let uv_scale = region.uv_scale(MAIN_ATLAS_SIZE);

            log::info!(
                "Text baked: {entity:?} '{content}' @ {size_px}px → atlas \
                 ({},{}) {}×{} uv={uv_offset:?} scale={uv_scale:?}",
                region.x,
                region.y,
                region.width,
                region.height,
            );

            self.ui_world.world.entity_mut(entity).insert(BakedText {
                uv_offset,
                uv_scale,
            });
        }
    }

    // -------------------------------------------------------------------------
    // Demo state machine
    // -------------------------------------------------------------------------

    /// Advance the demo one frame.
    ///
    /// Reads `InteractionEvents` (populated by `hit_test_system` during
    /// `ui_world.update()`) and drives the state machine by click.
    /// Transition phases ignore input — clicks are only acted on in idle phases.
    fn advance_demo(&mut self, dt: f32) {
        // Snapshot the events for this frame.
        let clicked: Vec<Entity> = self
            .ui_world
            .world
            .resource::<InteractionEvents>()
            .clicked
            .clone();

        let phase = std::mem::replace(&mut self.phase, DemoPhase::ButtonIdle);

        self.phase = match phase {
            // ── Button idle: click the button to expand ───────────────────────
            DemoPhase::ButtonIdle => {
                if clicked.contains(&self.button) {
                    log::info!("Phase: ButtonIdle → ButtonToList (1→N Bake)");
                    self.start_button_to_list();
                    DemoPhase::ButtonToList { items_done: 0 }
                } else {
                    DemoPhase::ButtonIdle
                }
            }

            // ── 1→N Bake: count per-item completions ─────────────────────────
            DemoPhase::ButtonToList { mut items_done } => {
                let completed: Vec<Entity> = self
                    .ui_world
                    .world
                    .resource::<CompletedTransitions>()
                    .entities
                    .clone();
                for e in completed {
                    if self.items.contains(&e) {
                        items_done += 1;
                    }
                }
                if items_done >= self.items.len() {
                    log::info!("Phase: ButtonToList → ListIdle");
                    DemoPhase::ListIdle
                } else {
                    DemoPhase::ButtonToList { items_done }
                }
            }

            // ── List idle: click Elephant to zoom in ──────────────────────────
            DemoPhase::ListIdle => {
                if clicked.contains(&self.items[0]) {
                    log::info!("Phase: ListIdle → ListToDetail (1→1)");
                    self.start_list_to_detail();
                    DemoPhase::ListToDetail
                } else {
                    DemoPhase::ListIdle
                }
            }

            // ── 1→1: item[0] → detail view ───────────────────────────────────
            DemoPhase::ListToDetail => {
                let completed: Vec<Entity> = self
                    .ui_world
                    .world
                    .resource::<CompletedTransitions>()
                    .entities
                    .clone();
                if completed.contains(&self.items[0]) {
                    log::info!("Phase: ListToDetail → DetailIdle");
                    DemoPhase::DetailIdle
                } else {
                    DemoPhase::ListToDetail
                }
            }

            // ── Detail idle: click anywhere on detail to go back ──────────────
            DemoPhase::DetailIdle => {
                if clicked.contains(&self.items[0]) {
                    log::info!("Phase: DetailIdle → DetailToList (1→1)");
                    self.start_detail_to_list();
                    DemoPhase::DetailToList
                } else {
                    DemoPhase::DetailIdle
                }
            }

            // ── 1→1: item[0] → list position ─────────────────────────────────
            DemoPhase::DetailToList => {
                let completed: Vec<Entity> = self
                    .ui_world
                    .world
                    .resource::<CompletedTransitions>()
                    .entities
                    .clone();
                if completed.contains(&self.items[0]) {
                    log::info!("Phase: DetailToList → ListSoloIdle");
                    DemoPhase::ListSoloIdle { timer: 0.0 }
                } else {
                    DemoPhase::DetailToList
                }
            }

            // ── Solo idle: 1.5 s auto-pause, Elephant alone ───────────────────
            DemoPhase::ListSoloIdle { timer } => {
                let t = timer + dt;
                if t >= 1.5 {
                    log::info!("Phase: ListSoloIdle → ListReformIdle");
                    self.ui_world
                        .world
                        .entity_mut(self.items[1])
                        .insert(Visibility::VISIBLE);
                    self.ui_world
                        .world
                        .entity_mut(self.items[2])
                        .insert(Visibility::VISIBLE);
                    DemoPhase::ListReformIdle
                } else {
                    DemoPhase::ListSoloIdle { timer: t }
                }
            }

            // ── Reform idle: click any item to converge back to button ────────
            DemoPhase::ListReformIdle => {
                let any_item_clicked = self.items.iter().any(|e| clicked.contains(e));
                if any_item_clicked {
                    log::info!("Phase: ListReformIdle → ListToButton (N→1 Slice)");
                    DemoPhase::ListToButton {
                        request_inserted: false,
                    }
                } else {
                    DemoPhase::ListReformIdle
                }
            }

            // ── N→1 Slice: items → button ─────────────────────────────────────
            DemoPhase::ListToButton { request_inserted } => {
                if !request_inserted {
                    self.start_list_to_button();
                }
                let lifecycle = self.ui_world.world.get::<Lifecycle>(self.button);
                let visibility = self.ui_world.world.get::<Visibility>(self.button);
                let done = matches!(lifecycle, Some(Lifecycle::Idle))
                    && matches!(visibility, Some(v) if v.visible);
                if done {
                    log::info!("Phase: ListToButton → ButtonIdle");
                    DemoPhase::ButtonIdle
                } else {
                    DemoPhase::ListToButton {
                        request_inserted: true,
                    }
                }
            }
        };
    }

    // -------------------------------------------------------------------------
    // Transition helpers
    // -------------------------------------------------------------------------

    /// Phase 1 — 1→N Bake: button expands into the three list items.
    ///
    /// The Bake strategy does NOT make targets visible automatically;
    /// the caller (us) must do so first.
    ///
    /// All three items use the same `default_config` (no stagger) so they
    /// start simultaneously and feel like a single parallel burst.
    fn start_button_to_list(&mut self) {
        // Snap each item to the button's current geometry BEFORE making it
        // visible.  `one_to_n_setup_system` runs in the next `update()` call
        // and its Commands aren't applied until the frame after that, so
        // there is a 2-frame window between "item becomes visible" and
        // "transition_tick starts interpolating from button geometry".
        // During that window the item must render where the button already
        // is — otherwise the final item positions flash for 2 frames before
        // the animation rewinds them to the start.
        let src = self
            .ui_world
            .world
            .get::<QuadState>(self.button)
            .cloned()
            .unwrap_or_else(button_quad);

        for &item in &self.items {
            self.ui_world
                .world
                .entity_mut(item)
                .insert(src.clone()) // at button position — no flash
                .insert(Visibility::VISIBLE);
        }

        // Insert OneToNRequest on the button (source).
        // child_behavior: None → all targets share default_config (delay 0).
        self.ui_world
            .world
            .entity_mut(self.button)
            .insert(OneToNRequest {
                targets: vec![
                    GroupTarget {
                        entity: self.items[0],
                        state: item_quad(0),
                    },
                    GroupTarget {
                        entity: self.items[1],
                        state: item_quad(1),
                    },
                    GroupTarget {
                        entity: self.items[2],
                        state: item_quad(2),
                    },
                ],
                default_config: demo_config(),
                child_behavior: None,
                strategy: SplitStrategy::Bake,
            });
    }

    /// Phase 3 — 1→1: item[0] expands to the detail quad.
    ///
    /// Items[1] and [2] are hidden (snapped off-screen during the detail view).
    fn start_list_to_detail(&mut self) {
        // Snap-hide the flanking items while item[0] takes centre stage.
        self.ui_world
            .world
            .entity_mut(self.items[1])
            .insert(Visibility::HIDDEN);
        self.ui_world
            .world
            .entity_mut(self.items[2])
            .insert(Visibility::HIDDEN);

        // 1→1: item[0] → detail_quad().
        self.ui_world
            .world
            .entity_mut(self.items[0])
            .insert(TransitionRequest {
                to: detail_quad(),
                from_state: None,
                config: demo_config(),
            });
    }

    /// Phase 5 — 1→1: item[0] contracts back to its list geometry.
    ///
    /// Items[1] and [2] remain hidden here; they are revealed later in
    /// `ListSoloIdle` after a beat so the viewer notices Elephant alone.
    fn start_detail_to_list(&mut self) {
        // 1→1: item[0] → item_quad(0).
        self.ui_world
            .world
            .entity_mut(self.items[0])
            .insert(TransitionRequest {
                to: item_quad(0),
                from_state: None,
                config: demo_config(),
            });
    }

    /// Phase 7 — N→1 Slice: three list items merge back into the button.
    ///
    /// `NToOneRequest` is inserted on the *destination* (button). It reads the
    /// current `QuadState` of each source entity from the `GroupSource.state`
    /// snapshot provided by the caller.
    fn start_list_to_button(&mut self) {
        // Snapshot the current geometry of each item (should be item_quad(0..2)
        // after DetailToList completed).
        let s0 = self
            .ui_world
            .world
            .get::<QuadState>(self.items[0])
            .cloned()
            .unwrap_or_else(|| item_quad(0));
        let s1 = self
            .ui_world
            .world
            .get::<QuadState>(self.items[1])
            .cloned()
            .unwrap_or_else(|| item_quad(1));
        let s2 = self
            .ui_world
            .world
            .get::<QuadState>(self.items[2])
            .cloned()
            .unwrap_or_else(|| item_quad(2));

        // child_behavior: None → all three converge in parallel with default_config.
        //
        // With stagger, each virtual entity completes at a different time and
        // sits at its sky-blue destination slice while the others are still
        // animating.  That produces up to 0.24 s of "ghost" sky-blue rectangles
        // at the button position before the group detects completion — exactly
        // the "three white rectangles" the user reported.  Parallel convergence
        // collapses all three completions to the same frame, making the snap to
        // the button invisible.
        self.ui_world
            .world
            .entity_mut(self.button)
            .insert(NToOneRequest {
                sources: vec![
                    GroupSource {
                        entity: self.items[0],
                        state: s0,
                    },
                    GroupSource {
                        entity: self.items[1],
                        state: s1,
                    },
                    GroupSource {
                        entity: self.items[2],
                        state: s2,
                    },
                ],
                default_config: demo_config(),
                child_behavior: None,
            });
    }

    // -------------------------------------------------------------------------
    // Render
    // -------------------------------------------------------------------------

    fn render(&mut self) {
        // 1. Delta time — cap to prevent large jumps after pauses.
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;

        // 2. Flush staged pointer events into the ECS PointerInput resource.
        {
            let mut pi = self.ui_world.world.resource_mut::<PointerInput>();
            pi.position = self.staged_pointer.position;
            pi.just_pressed = self.staged_pointer.just_pressed;
            pi.just_released = self.staged_pointer.just_released;
            pi.is_pressed = self.staged_pointer.is_pressed;
        }
        // Clear one-shot flags — they are only true for one frame.
        self.staged_pointer.just_pressed = false;
        self.staged_pointer.just_released = false;

        // 3. Tick ECS (hit_test_system → transition systems).
        self.ui_world.update(dt);

        // 4. Bake any pending text entities.
        self.bake_pending_text();

        // 5. Advance demo state machine (reads InteractionEvents from step 3).
        self.advance_demo(dt);

        // 6. Pull the latest video frame from the BYOV channel (M9).
        //    The background producer thread sends frames at ~30 fps; we drain
        //    the channel here and upload only the freshest one.  Entities
        //    without VideoPlayer are unaffected (they use atlas_page 0).
        self.pipeline.consume_video_frame(&self.queue);

        // 7. Collect visible instance data.
        //    See `proteus_ui::collect` for the two-instance-per-text-entity model.
        let instances = collect_instances(&mut self.ui_world.world);

        // 8. Acquire swap-chain texture.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
                return;
            }
            Err(e) => {
                log::error!("Surface error: {e}");
                return;
            }
        };

        let view = frame.texture.create_view(&Default::default());

        if !instances.is_empty() {
            self.pipeline.upload_instances(&self.queue, &instances);
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
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.94,
                            g: 0.94,
                            b: 0.96,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if !instances.is_empty() {
                self.pipeline.draw(&mut pass);
            }
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
    }
}
