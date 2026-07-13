//! `proteus-shell-native` — native desktop entry point.
//!
//! Creates a winit window, attaches a wgpu surface, initializes `QuadPipeline`,
//! and runs the render loop. Targets macOS (Metal), Linux (Vulkan), Windows (DX12).
//!
//! ## M4 wiring (text)
//!
//! Each frame, after `ui_world.update(dt)`:
//!
//! 1. Query all entities that have a `Text` component but no `BakedText`.
//! 2. For each such entity, call `font_atlas.bake_text()` to rasterize the string.
//! 3. Upload the RGBA pixels to `main_atlas` via `pipeline.write_to_main_atlas()`.
//! 4. Insert `BakedText { uv_offset, uv_scale }` on the entity.
//!
//! Subsequent frames skip the bake step (entity now has `BakedText`).
//! The `quad_state_to_instance` helper checks for `BakedText` and uses its UV
//! coordinates instead of the white-pixel sentinel, so text renders as a
//! textured quad through the standard pipeline.
//!
//! ## M2 wiring (retained)
//!
//! `ProteusWorld` (bevy_ecs + system schedule) drives the scene. Each frame:
//!
//! 1. Compute wall-clock delta time.
//! 2. Call `ui_world.update(dt)` — runs the full transition pipeline.
//! 3. Bake any pending `Text` entities (see M4 above).
//! 4. Query all `QuadState` components and convert to `QuadInstance`s.
//! 5. If any transitions completed this frame, queue the reverse ping-pong.
//! 6. Upload the instance buffer and submit the draw call.

use std::sync::Arc;
use std::time::Instant;

use glam::{Vec2, Vec3, Vec4};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

use proteus_render::{FontAtlas, QuadInstance, QuadPipeline, MAIN_ATLAS_SIZE};
use proteus_ui::{
    component::{Lifecycle, TransitionRequest},
    ease_in_out_quad,
    transition::{CompletedTransitions, TransitionConfig},
    BakedText, Entity, ProteusWorld, QuadState, Text,
};

// ---------------------------------------------------------------------------
// Demo scene constants
// ---------------------------------------------------------------------------

/// Starting state — wide blue pill near the upper-left quadrant.
fn state_a() -> QuadState {
    QuadState {
        position: Vec3::new(-130.0, 60.0, 0.5),
        size: Vec2::new(240.0, 90.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.224, 0.510, 1.0, 1.0), // Proteus blue
        corner_radius: 14.0,
    }
}

/// End state — narrow tall orange pill near the lower-right quadrant.
fn state_b() -> QuadState {
    QuadState {
        position: Vec3::new(130.0, -60.0, 0.5),
        size: Vec2::new(90.0, 240.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 0.44, 0.09, 1.0), // warm orange
        corner_radius: 45.0,
    }
}

/// Transition config used in both directions of the ping-pong.
fn ping_pong_config() -> TransitionConfig {
    TransitionConfig {
        duration: 1.4,
        delay: 0.25, // brief pause at each end before reversing
        easing: ease_in_out_quad,
    }
}

/// Convert a `QuadState` + optional `BakedText` into a `QuadInstance` (GPU struct).
///
/// If `baked` is `Some`, UV fields address the text sub-region in `main_atlas`.
/// Otherwise, UV fields point at the white-pixel sentinel so the entity renders
/// as a solid-color quad.
fn quad_state_to_instance(qs: &QuadState, baked: Option<&BakedText>) -> QuadInstance {
    let (uv_offset, uv_scale) = match baked {
        Some(b) => (b.uv_offset, b.uv_scale),
        None => (
            QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            QuadPipeline::WHITE_PIXEL_UV_SCALE,
        ),
    };

    QuadInstance {
        position: qs.position.to_array(),
        size: qs.size.to_array(),
        rotation: qs.rotation,
        scale: qs.scale,
        anchor: qs.anchor.to_array(),
        color: qs.color.to_array(),
        opacity: 1.0,
        corner_radius: qs.corner_radius,
        uv_offset,
        uv_scale,
        atlas_page: 0,
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    // `RUST_LOG=info cargo run` to see log output.
    env_logger::init();
    log::info!("Proteus native shell starting");

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = ProteusApp::default();
    event_loop.run_app(&mut app).expect("event loop error");
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

/// Top-level application state. Holds `RenderState` once the window is created.
/// winit 0.30 creates the window inside `resumed()`, not in `main()`.
#[derive(Default)]
struct ProteusApp {
    state: Option<RenderState>,
}

impl ApplicationHandler for ProteusApp {
    /// Called when the application is ready to create its window.
    /// On desktop this fires once at startup; on mobile it may fire after a suspend.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return; // already initialized — ignore re-resume on desktop
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("Proteus")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32)),
                )
                .expect("failed to create window"),
        );

        // Block on async GPU init. `pollster` is a minimal executor that only
        // blocks the calling thread — appropriate here since we have no work to
        // do until the GPU is ready.
        let state = pollster::block_on(RenderState::new(window));
        self.state = Some(state);

        // Kick the render loop: on macOS (and sometimes other platforms) winit 0.30
        // does NOT automatically deliver `RedrawRequested` for a newly created window.
        // This explicit request ensures the first frame is painted immediately.
        self.state.as_ref().unwrap().window.request_redraw();
    }

    /// Called when the event queue is drained and the loop is about to block.
    /// Requesting a redraw here drives continuous rendering — every time the loop
    /// goes idle we schedule another frame.
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

            // Reconfigure the surface when the window is resized.
            WindowEvent::Resized(size) => {
                state.resize(size);
            }

            // The OS is asking us to draw a frame.
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

/// All GPU resources and ECS world for one window.
///
/// Created once in `resumed()` and lives until the application exits.
struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: QuadPipeline,
    /// The ECS world + Proteus system schedule. Ticked once per frame.
    ui_world: ProteusWorld,
    /// The single entity whose `QuadState` the demo animates (M2 ping-pong).
    demo_entity: Entity,
    /// A second entity with a `Text` label, demonstrating M4 text rendering.
    /// Prefixed `_` because the entity is managed through the ECS world by ID
    /// after spawn; direct field access is reserved for future milestones (M5+).
    _label_entity: Entity,
    /// CPU-side font atlas for rasterizing text strings (M4).
    font_atlas: FontAtlas,
    /// Ping-pong direction: `true` = currently going toward state_b,
    /// `false` = going back toward state_a. Flipped on each completion.
    going_forward: bool,
    /// Wall-clock time of the previous frame, used to compute delta time.
    last_frame: Instant,
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
        // `Arc<Window>` implements `SurfaceTarget<'static>` so the surface
        // lifetime is tied to the Arc, not a raw borrow of the window.
        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        // --- Adapter ---
        // Request an adapter that is compatible with our surface so the chosen
        // backend (Metal / Vulkan / DX12) can actually present to the window.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter found");

        log::info!("GPU adapter: {}", adapter.get_info().name);

        // --- Device and queue ---
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
        // Prefer an sRGB format so the OS compositor receives gamma-correct output.
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
        let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096);

        // Set up the initial orthographic projection for the window size.
        pipeline.set_view_projection(
            &queue,
            QuadPipeline::ortho(size.width as f32, size.height as f32),
        );

        log::info!(
            "Render state ready — {}×{} px, format: {:?}",
            size.width,
            size.height,
            surface_format,
        );

        // --- Font atlas (M4) ---
        let font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

        // --- ECS world ---
        let mut ui_world = ProteusWorld::new();

        // Demo entity (M2 ping-pong).
        let demo_entity = ui_world
            .world
            .spawn((
                state_a(),
                Lifecycle::Idle,
                TransitionRequest {
                    to: state_b(),
                    config: ping_pong_config(),
                    from_state: None,
                },
            ))
            .id();

        // Label entity (M4 text) — a static label above the animated quad.
        // It has no transition; it stays fixed and displays text.
        let label_entity = ui_world
            .world
            .spawn((
                QuadState {
                    // Positioned above center. The quad is sized to the text label.
                    // The shell will resize the quad after baking to match the baked
                    // pixel dimensions (future enhancement); for M4 we declare a
                    // fixed size that comfortably fits a short label.
                    position: Vec3::new(0.0, 220.0, 0.6),
                    size: Vec2::new(220.0, 36.0),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: Vec4::new(1.0, 1.0, 1.0, 1.0), // white — text tinted white
                    corner_radius: 0.0,
                },
                Lifecycle::Idle,
                Text::new("Proteus — M4 Text", 24.0),
            ))
            .id();

        log::info!(
            "Demo entity {:?} — ping-pong transition started",
            demo_entity
        );
        log::info!("Label entity {:?} — pending text bake", label_entity);

        Self {
            window,
            surface,
            surface_config,
            device,
            queue,
            pipeline,
            ui_world,
            demo_entity,
            _label_entity: label_entity,
            font_atlas,
            going_forward: true, // first transition is state_a → state_b
            last_frame: Instant::now(),
        }
    }

    /// Reconfigure the surface and update the projection matrix when the window resizes.
    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return; // minimized — skip
        }
        log::debug!("Resize: {}×{}", size.width, size.height);
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        self.pipeline.set_view_projection(
            &self.queue,
            QuadPipeline::ortho(size.width as f32, size.height as f32),
        );
    }

    // ---------------------------------------------------------------------------
    // Text bake pass (M4)
    // ---------------------------------------------------------------------------

    /// Bake all entities that have a `Text` component but not yet a `BakedText`.
    ///
    /// For each such entity:
    /// 1. Rasterize the string via `font_atlas.bake_text()`.
    /// 2. Upload the pixel data to `main_atlas` via `pipeline.write_to_main_atlas()`.
    /// 3. Insert `BakedText` with the resulting UV coords on the entity.
    ///
    /// This runs once per entity (bake is skipped on subsequent frames because
    /// `BakedText` is now present).
    fn bake_pending_text(&mut self) {
        // Step 1: Collect (entity, content, size_px) for every Text entity.
        // The inner block ends the query borrow before we touch the world again.
        let all_text: Vec<(Entity, String, f32)> = {
            let mut q = self.ui_world.world.query::<(Entity, &Text)>();
            q.iter(&self.ui_world.world)
                .map(|(e, t)| (e, t.content.clone(), t.size_px))
                .collect()
        };

        // Step 2: Filter to those that still lack a BakedText.
        // Now that the query borrow has ended we can call world.get() freely.
        let pending: Vec<(Entity, String, f32)> = all_text
            .into_iter()
            .filter(|(e, _, _)| self.ui_world.world.get::<BakedText>(*e).is_none())
            .collect();

        for (entity, content, size_px) in pending {
            let Some(region) = self.font_atlas.bake_text(&content, size_px) else {
                log::warn!("FontAtlas full or empty string for entity {:?}", entity);
                continue;
            };

            // Upload pixels to GPU.
            self.pipeline.write_to_main_atlas(
                &self.queue,
                region.x,
                region.y,
                region.width,
                region.height,
                &region.rgba_pixels,
            );

            // Compute normalised UVs.
            let uv_offset = region.uv_offset(MAIN_ATLAS_SIZE);
            let uv_scale = region.uv_scale(MAIN_ATLAS_SIZE);

            log::info!(
                "Text baked: {:?} '{content}' @ {size_px}px → atlas ({},{}) {}×{} \
                 uv_offset={uv_offset:?} uv_scale={uv_scale:?}",
                entity,
                region.x,
                region.y,
                region.width,
                region.height,
            );

            // Write BakedText back onto the entity.
            self.ui_world.world.entity_mut(entity).insert(BakedText {
                uv_offset,
                uv_scale,
            });
        }
    }

    // ---------------------------------------------------------------------------
    // Render
    // ---------------------------------------------------------------------------

    /// Render one frame.
    ///
    /// Frame order:
    /// 1. Compute delta time.
    /// 2. Tick the ECS world (transition systems run).
    /// 3. Bake any pending text entities (M4).
    /// 4. Handle completed transitions (ping-pong: queue the reverse).
    /// 5. Collect `QuadState`s + optional `BakedText` → `QuadInstance`s.
    /// 6. Upload instances, encode render pass, submit.
    fn render(&mut self) {
        // 1. Delta time — cap at 50 ms so a paused/background app doesn't
        //    cause a huge lerp jump when it resumes.
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;

        // 2. Advance the ECS world one frame.
        self.ui_world.update(dt);

        // 3. Bake pending text entities (M4).
        self.bake_pending_text();

        // 4. Ping-pong: when a transition completes, immediately queue the reverse.
        //    `CompletedTransitions` is populated by `transition_complete_system` and
        //    holds exactly this frame's completions. Clone the entity list so the
        //    immutable borrow on the resource is released before we mutate the world.
        let completions: Vec<Entity> = self
            .ui_world
            .world
            .resource::<CompletedTransitions>()
            .entities
            .clone();

        for entity in completions {
            if entity == self.demo_entity {
                self.going_forward = !self.going_forward;
                let target = if self.going_forward {
                    state_b()
                } else {
                    state_a()
                };
                self.ui_world
                    .world
                    .entity_mut(entity)
                    .insert(TransitionRequest {
                        to: target,
                        config: ping_pong_config(),
                        from_state: None,
                    });
                log::debug!(
                    "Transition complete — queued {} transition",
                    if self.going_forward { "A→B" } else { "B→A" }
                );
            }
        }

        // 5. Collect instance data from every entity with a QuadState.
        //    Entities with a BakedText get text UV; others get the white-pixel sentinel.
        //
        //    Two-step pattern: query first (collect clones), then look up BakedText.
        //    This avoids a nested borrow of the world (the query holds &World for its
        //    lifetime; calling world.get() inside the same iterator would conflict
        //    with borrow checker in some Bevy ECS versions).
        let instances: Vec<QuadInstance> = {
            // Step 1: collect (entity, cloned QuadState) — ends the query borrow.
            let states: Vec<(Entity, QuadState)> = {
                let mut q = self.ui_world.world.query::<(Entity, &QuadState)>();
                q.iter(&self.ui_world.world)
                    .map(|(e, qs)| (e, qs.clone()))
                    .collect()
            };
            // Step 2: BakedText lookup is safe now; the query borrow has dropped.
            states
                .iter()
                .map(|(e, qs)| {
                    let baked = self.ui_world.world.get::<BakedText>(*e);
                    quad_state_to_instance(qs, baked)
                })
                .collect()
        };

        // 6. Acquire the next swap-chain texture to draw into.
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            // Surface lost or outdated (e.g. window resized between events) —
            // reconfigure and skip this frame.
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

        // Upload instance buffer — skip GPU work if there's nothing to draw.
        if !instances.is_empty() {
            self.pipeline.upload_instances(&self.queue, &instances);
        }

        // Encode render pass.
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
                        // Dark background so the animated quad is clearly visible.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.05,
                            b: 0.05,
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

        // Submit and present.
        self.queue.submit([encoder.finish()]);
        frame.present();
        // Continuous redraws are driven by `about_to_wait()` — no request needed here.
    }
}
