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
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes},
};

use proteus_render::{FontAtlas, QuadPipeline, MAIN_ATLAS_SIZE};
use proteus_ui::{
    collect_instances, ease_in_out_quad,
    transition::{CompletedTransitions, TransitionConfig},
    BakedText, Entity, GroupSource, GroupTarget, Lifecycle, NToOneRequest, OneToNRequest,
    ProteusWorld, QuadState, SplitStrategy, Text, TransitionRequest, Visibility,
};

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

/// The demo cycles through these phases indefinitely.
#[derive(Debug)]
enum DemoPhase {
    /// Button is fully visible; wait `timer` seconds before expanding.
    ButtonIdle { timer: f32 },

    /// 1→N Bake in progress: button → three list items.
    /// Counts per-item completions via `CompletedTransitions`.
    ButtonToList { items_done: usize },

    /// All three items visible; wait before drilling into item[0].
    ListIdle { timer: f32 },

    /// 1→1 transition: item[0] → `detail_quad()`.
    ListToDetail,

    /// Detail view showing; wait before collapsing back.
    DetailIdle { timer: f32 },

    /// 1→1 transition: item[0] → `item_quad(0)`.
    DetailToList,

    /// item[0] has returned to list size; items[1] and [2] are still hidden.
    /// Gives the viewer a beat to notice Elephant alone before the rest reform.
    ListSoloIdle { timer: f32 },

    /// All three items are visible again; brief pause before converging to button.
    ListReformIdle { timer: f32 },

    /// N→1 Slice in progress: list items merge back into button.
    /// `request_inserted` is false on first call; set to true after inserting.
    ListToButton { request_inserted: bool },

    /// The N→1 transition just finished and the button has reappeared.
    /// Hold here so the viewer can appreciate the final state before the
    /// loop restarts.
    LoopEndIdle { timer: f32 },
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

    // ── timing ─────────────────────────────────────────────────────────────
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

        // --- Font atlas ---
        let font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

        // --- ECS world ---
        let mut ui_world = ProteusWorld::new();

        // Button: starts visible, shows "View Items" label.
        let button = ui_world
            .world
            .spawn((
                button_quad(),
                Lifecycle::Idle,
                Visibility::VISIBLE,
                Text::new("View Items", 22.0),
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
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    item_quad(1),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(item_labels[1], 18.0),
                ))
                .id(),
            ui_world
                .world
                .spawn((
                    item_quad(2),
                    Lifecycle::Idle,
                    Visibility::HIDDEN,
                    Text::new(item_labels[2], 18.0),
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
            phase: DemoPhase::ButtonIdle { timer: 0.0 },
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
    /// Uses `std::mem::replace` to move `self.phase` out, compute the
    /// next phase (potentially calling `self.start_*()` helpers), then store
    /// it back. This avoids a borrow conflict between the match arm and `&mut self`.
    fn advance_demo(&mut self, dt: f32) {
        // Temporarily replace phase with a cheap placeholder.
        let phase = std::mem::replace(&mut self.phase, DemoPhase::ButtonIdle { timer: 0.0 });

        self.phase = match phase {
            // ── Button idle ──────────────────────────────────────────────────
            DemoPhase::ButtonIdle { timer } => {
                let t = timer + dt;
                if t >= 2.0 {
                    log::info!("Phase: ButtonIdle → ButtonToList (1→N Bake)");
                    self.start_button_to_list();
                    DemoPhase::ButtonToList { items_done: 0 }
                } else {
                    DemoPhase::ButtonIdle { timer: t }
                }
            }

            // ── 1→N Bake: count per-item completions ─────────────────────────
            DemoPhase::ButtonToList { mut items_done } => {
                // `transition_complete_system` refreshes this resource every frame.
                let completed: Vec<Entity> = self
                    .ui_world
                    .world
                    .resource::<CompletedTransitions>()
                    .entities
                    .clone();
                for e in completed {
                    if self.items.contains(&e) {
                        items_done += 1;
                        log::debug!("Item transition done ({items_done}/{}).", self.items.len());
                    }
                }
                if items_done >= self.items.len() {
                    log::info!("Phase: ButtonToList → ListIdle");
                    DemoPhase::ListIdle { timer: 0.0 }
                } else {
                    DemoPhase::ButtonToList { items_done }
                }
            }

            // ── List idle ────────────────────────────────────────────────────
            DemoPhase::ListIdle { timer } => {
                let t = timer + dt;
                if t >= 2.0 {
                    log::info!("Phase: ListIdle → ListToDetail (1→1)");
                    self.start_list_to_detail();
                    DemoPhase::ListToDetail
                } else {
                    DemoPhase::ListIdle { timer: t }
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
                    DemoPhase::DetailIdle { timer: 0.0 }
                } else {
                    DemoPhase::ListToDetail
                }
            }

            // ── Detail idle ──────────────────────────────────────────────────
            DemoPhase::DetailIdle { timer } => {
                let t = timer + dt;
                if t >= 2.0 {
                    log::info!("Phase: DetailIdle → DetailToList (1→1)");
                    self.start_detail_to_list();
                    DemoPhase::DetailToList
                } else {
                    DemoPhase::DetailIdle { timer: t }
                }
            }

            // ── 1→1: item[0] → list position ────────────────────────────────
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

            // ── Solo idle: Elephant alone, flanking items still hidden ────────
            DemoPhase::ListSoloIdle { timer } => {
                let t = timer + dt;
                if t >= 1.5 {
                    log::info!("Phase: ListSoloIdle → ListReformIdle (flanking items appear)");
                    self.ui_world
                        .world
                        .entity_mut(self.items[1])
                        .insert(Visibility::VISIBLE);
                    self.ui_world
                        .world
                        .entity_mut(self.items[2])
                        .insert(Visibility::VISIBLE);
                    DemoPhase::ListReformIdle { timer: 0.0 }
                } else {
                    DemoPhase::ListSoloIdle { timer: t }
                }
            }

            // ── Reform idle: all three visible, pause before converging ──────
            DemoPhase::ListReformIdle { timer } => {
                let t = timer + dt;
                if t >= 1.5 {
                    log::info!("Phase: ListReformIdle → ListToButton (N→1 Slice)");
                    DemoPhase::ListToButton {
                        request_inserted: false,
                    }
                } else {
                    DemoPhase::ListReformIdle { timer: t }
                }
            }

            // ── N→1 Slice: items merge back into button ───────────────────────
            DemoPhase::ListToButton { request_inserted } => {
                if !request_inserted {
                    self.start_list_to_button();
                }
                // Before n_to_one_setup_system runs (next frame), the button is
                // HIDDEN + Idle (hidden since ButtonToList). After it runs the
                // button becomes HIDDEN + Transitioning. After group completion
                // the button becomes VISIBLE + Idle.
                //
                // Checking `Idle && VISIBLE` is therefore a correct completion
                // signal — it is false in all intermediate states.
                let lifecycle = self.ui_world.world.get::<Lifecycle>(self.button);
                let visibility = self.ui_world.world.get::<Visibility>(self.button);
                let done = matches!(lifecycle, Some(Lifecycle::Idle))
                    && matches!(visibility, Some(v) if v.visible);
                if done {
                    log::info!("Phase: ListToButton → LoopEndIdle");
                    DemoPhase::LoopEndIdle { timer: 0.0 }
                } else {
                    DemoPhase::ListToButton {
                        request_inserted: true,
                    }
                }
            }
            // ── Post-loop pause ──────────────────────────────────────────────
            DemoPhase::LoopEndIdle { timer } => {
                let t = timer + dt;
                if t >= 2.0 {
                    log::info!("Phase: LoopEndIdle → ButtonIdle (loop restart)");
                    DemoPhase::ButtonIdle { timer: 0.0 }
                } else {
                    DemoPhase::LoopEndIdle { timer: t }
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

        // 2. Tick ECS (transition + group systems).
        self.ui_world.update(dt);

        // 3. Bake any pending text entities.
        self.bake_pending_text();

        // 4. Advance demo state machine.
        self.advance_demo(dt);

        // 5. Collect visible instance data.
        //    See `proteus_ui::collect` for the two-instance-per-text-entity model.
        let instances = collect_instances(&mut self.ui_world.world);

        // 6. Acquire swap-chain texture.
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
                            r: 0.05,
                            g: 0.05,
                            b: 0.07,
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
