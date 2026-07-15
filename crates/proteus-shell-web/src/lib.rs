//! `proteus-shell-web` — WebGL2 / WASM shell (M5 reference demo).
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
//! Identical to `proteus-shell-native` but uses the wgpu WebGL2 backend:
//! - `wgpu::Backends::GL` instead of `all()`
//! - `wgpu::SurfaceTarget::Canvas(canvas)` instead of a winit surface
//! - `wgpu::Limits::downlevel_webgl2_defaults()` to match WebGL2 caps
//! - No `pollster`; `init` is `async fn` called directly from JS via `await`

use wasm_bindgen::prelude::*;

// ---------------------------------------------------------------------------
// Legacy stub (always compiled — keep for backward compatibility)
// ---------------------------------------------------------------------------

/// Legacy entry point.  The full `ProteusApp` class is preferred for M5+.
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

    use proteus_render::{FontAtlas, QuadPipeline, MAIN_ATLAS_SIZE};
    use proteus_ui::{
        collect_instances, ease_in_out_quad,
        transition::{CompletedTransitions, TransitionConfig},
        BakedText, DropShadow, Entity, Glow, GroupSource, GroupTarget, Interactable,
        InteractionEvents, Lifecycle, NToOneRequest, OneToNRequest, PointerInput, ProteusWorld,
        QuadState, SplitStrategy, Text, TransitionRequest, Visibility,
    };

    // -------------------------------------------------------------------------
    // Demo scene geometry  (identical to proteus-shell-native)
    // -------------------------------------------------------------------------

    fn button_quad() -> QuadState {
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(200.0, 48.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(0.37, 0.65, 1.00, 1.0),
            corner_radius: 0.0,
        }
    }

    fn item_quad(idx: usize) -> QuadState {
        let y = 80.0 - idx as f32 * 72.0;
        let color = match idx {
            0 => Vec4::new(0.25, 0.90, 0.60, 1.0),
            1 => Vec4::new(0.20, 0.80, 0.90, 1.0),
            _ => Vec4::new(0.60, 0.65, 1.00, 1.0),
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

    fn detail_quad() -> QuadState {
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(280.0, 100.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.00, 0.82, 0.28, 1.0),
            corner_radius: 0.0,
        }
    }

    fn demo_config() -> TransitionConfig {
        TransitionConfig {
            duration: 0.60,
            delay: 0.0,
            easing: ease_in_out_quad,
        }
    }

    // -------------------------------------------------------------------------
    // Demo phase state machine (identical to native)
    // -------------------------------------------------------------------------

    /// Click-driven demo phases (M7). Idle phases wait for user input; transition
    /// phases are in-flight and block input until they complete.
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

    // (Instance collection is handled by proteus_ui::collect_instances.)

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
        pipeline: QuadPipeline,
        ui_world: ProteusWorld,
        font_atlas: FontAtlas,
        button: Entity,
        items: [Entity; 3],
        phase: DemoPhase,
        staged_pointer: StagedPointer,
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

            // WebGL2 instance.
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..Default::default()
            });

            // Surface bound to the canvas.  On wasm32 the canvas reference has
            // JS-managed lifetime so the Surface is effectively 'static.
            let surface = instance
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                .map_err(|e| JsValue::from_str(&format!("create_surface: {e}")))?;

            // Adapter — WebGL2 only, no high-performance preference on mobile.
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::None,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await
                .ok_or_else(|| JsValue::from_str("no suitable WebGL2 adapter"))?;

            log::info!("Adapter: {}", adapter.get_info().name);

            // Device & queue — WebGL2 limits.
            let (device, queue) = adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("proteus-web"),
                        required_features: wgpu::Features::empty(),
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                        memory_hints: Default::default(),
                    },
                    None,
                )
                .await
                .map_err(|e| JsValue::from_str(&format!("request_device: {e}")))?;

            // Surface configuration.
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
                        item_quad(2),
                        Lifecycle::Idle,
                        Visibility::HIDDEN,
                        Text::new(item_labels[2], 18.0),
                        Interactable,
                        Glow {
                            radius: 17.0,
                            color: Vec4::new(0.60, 0.65, 1.00, 1.0), // lavender — matches item[2] fill
                            intensity: 0.7,
                        },
                    ))
                    .id(),
            ];

            log::info!("Entities — button {:?}, items {:?}", button, items);

            Ok(ProteusApp {
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
            self.advance_demo(dt);

            // Collect visible instances.
            // See `proteus_ui::collect` for the two-instance-per-text-entity model.
            let instances = collect_instances(&mut self.ui_world.world);

            let frame = match self.surface.get_current_texture() {
                Ok(f) => f,
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    self.surface.configure(&self.device, &self.surface_config);
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
                    label: Some("frame"),
                });

            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("main"),
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
            self.pipeline.set_view_projection(
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
    }

    // ── private helpers ────────────────────────────────────────────────────────

    impl ProteusApp {
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
                self.ui_world.world.entity_mut(entity).insert(BakedText {
                    uv_offset,
                    uv_scale,
                });
            }
        }

        fn advance_demo(&mut self, dt: f32) {
            // Snapshot the click events produced by hit_test_system this frame.
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
                        DemoPhase::ListIdle
                    } else {
                        DemoPhase::ButtonToList { items_done }
                    }
                }

                // ── List idle: click Elephant to zoom in ──────────────────────────
                DemoPhase::ListIdle => {
                    if clicked.contains(&self.items[0]) {
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
                        DemoPhase::DetailIdle
                    } else {
                        DemoPhase::ListToDetail
                    }
                }

                // ── Detail idle: click anywhere on detail to go back ──────────────
                DemoPhase::DetailIdle => {
                    if clicked.contains(&self.items[0]) {
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
                        DemoPhase::ListSoloIdle { timer: 0.0 }
                    } else {
                        DemoPhase::DetailToList
                    }
                }

                // ── Solo idle: 1.5 s auto-pause, Elephant alone ───────────────────
                DemoPhase::ListSoloIdle { timer } => {
                    let t = timer + dt;
                    if t >= 1.5 {
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
                        DemoPhase::ButtonIdle
                    } else {
                        DemoPhase::ListToButton {
                            request_inserted: true,
                        }
                    }
                }
            };
        }

        fn start_button_to_list(&mut self) {
            // Snap items to button geometry before making them visible —
            // eliminates the 2-frame flash caused by Command deferral.
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
                    .insert(src.clone())
                    .insert(Visibility::VISIBLE);
            }
            // child_behavior: None → all items share default_config (delay 0).
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

        fn start_list_to_detail(&mut self) {
            self.ui_world
                .world
                .entity_mut(self.items[1])
                .insert(Visibility::HIDDEN);
            self.ui_world
                .world
                .entity_mut(self.items[2])
                .insert(Visibility::HIDDEN);
            self.ui_world
                .world
                .entity_mut(self.items[0])
                .insert(TransitionRequest {
                    to: detail_quad(),
                    from_state: None,
                    config: demo_config(),
                });
        }

        fn start_detail_to_list(&mut self) {
            // items[1] and [2] stay hidden; they're revealed in ListSoloIdle
            // after a beat so the viewer notices Elephant alone first.
            self.ui_world
                .world
                .entity_mut(self.items[0])
                .insert(TransitionRequest {
                    to: item_quad(0),
                    from_state: None,
                    config: demo_config(),
                });
        }

        fn start_list_to_button(&mut self) {
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
    }
} // mod inner

// Re-export ProteusApp at crate root so wasm-bindgen can generate bindings.
#[cfg(target_arch = "wasm32")]
pub use inner::ProteusApp;
