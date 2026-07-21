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
//! three video tiles on click (1→N Slice, baked crossfade), and converges
//! back on clicking any tile (N→1 Slice).

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

    use proteus_render::{FontAtlas, GpuContext, QuadPipeline, MAIN_ATLAS_SIZE};
    use proteus_ui::{
        collect_instances, ease_in_out_quad, ease_out_quad, transition::TransitionConfig,
        BakedText, Border, Entity, Glow, GroupSource, GroupTarget, Interactable, InteractionEvents,
        Lifecycle, NToOneRequest, OneToNRequest, PointerInput, ProteusWorld, QuadState,
        SplitStrategy, Text, Visibility,
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

    const TILE_LABELS: [&str; 3] = ["Big Buck Bunny", "Sintel", "Jellyfish"];
    const TILE_COLORS: [Vec4; 3] = [
        Vec4::new(0.85, 0.55, 0.15, 1.0), // amber — Big Buck Bunny
        Vec4::new(0.10, 0.45, 0.35, 1.0), // deep teal — Sintel
        Vec4::new(0.10, 0.55, 0.65, 1.0), // aqua — Jellyfish
    ];

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

    // -------------------------------------------------------------------------
    // Demo phase state machine
    // -------------------------------------------------------------------------

    /// Click-driven demo phases. Idle phases wait for user input; the transition
    /// phase is in-flight and tracks per-target completion.
    enum DemoPhase {
        /// Button visible — click it to spread into the three video tiles.
        ButtonIdle,
        /// 1→N Slice in progress: button splitting into three slices that morph
        /// into the tiles.
        ButtonToTiles,
        /// Three tiles visible — click any tile to converge back into the button.
        TilesIdle,
        /// N→1 Slice in progress: tiles converging back into the button.
        TilesToButton { request_inserted: bool },
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
        tiles: [Entity; 3],
        phase: DemoPhase,

        staged_pointer: StagedPointer,

        // ── demo animation state ───────────────────────────────────────────
        intro_delay_remaining: f32,
        intro_elapsed: f32,
        hover_progress: f32,
        is_hovering: bool,
        tile_hover_progress: [f32; 3],
        tile_is_hovering: [bool; 3],
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

            Ok(ProteusApp {
                surface,
                surface_config,
                device,
                queue,
                ui_world,
                font_atlas,
                button,
                tiles,
                phase: DemoPhase::ButtonIdle,
                staged_pointer: StagedPointer::default(),
                intro_delay_remaining: INTRO_DELAY,
                intro_elapsed: 0.0,
                hover_progress: 0.0,
                is_hovering: false,
                tile_hover_progress: [0.0; 3],
                tile_is_hovering: [false; 3],
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
            self.advance_intro_and_hover(dt);
            self.advance_tile_hover(dt);
            self.advance_demo();

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
            if let Some(mut text) = self.ui_world.world.get_mut::<Text>(self.button) {
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
        /// click.
        fn advance_demo(&mut self) {
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

                // ── Tiles idle: click any tile to converge back into the button ─
                DemoPhase::TilesIdle => {
                    let any_tile_clicked = self.tiles.iter().any(|e| clicked.contains(e));
                    if any_tile_clicked {
                        DemoPhase::TilesToButton {
                            request_inserted: false,
                        }
                    } else {
                        DemoPhase::TilesIdle
                    }
                }

                // ── N→1 Slice: tiles converge back into the button ─────────────
                DemoPhase::TilesToButton { request_inserted } => {
                    if !request_inserted {
                        self.start_tiles_to_button();
                    }
                    let lifecycle = self.ui_world.world.get::<Lifecycle>(self.button);
                    let visibility = self.ui_world.world.get::<Visibility>(self.button);
                    let done = matches!(lifecycle, Some(Lifecycle::Idle))
                        && matches!(visibility, Some(v) if v.visible);
                    if done {
                        DemoPhase::ButtonIdle
                    } else {
                        DemoPhase::TilesToButton {
                            request_inserted: true,
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

        /// N→1 Slice: the three tiles converge back into the button — the same
        /// morph, played in reverse. `NToOneRequest` is inserted on the
        /// *destination* (button); it reads each tile's current `QuadState` from
        /// the `GroupSource.state` snapshot taken here.
        fn start_tiles_to_button(&mut self) {
            let sources: Vec<GroupSource> = self
                .tiles
                .iter()
                .enumerate()
                .map(|(i, &tile)| GroupSource {
                    entity: tile,
                    state: self
                        .ui_world
                        .world
                        .get::<QuadState>(tile)
                        .cloned()
                        .unwrap_or_else(|| tile_quad(i)),
                })
                .collect();

            self.ui_world
                .world
                .entity_mut(self.button)
                .insert(NToOneRequest {
                    sources,
                    default_config: TransitionConfig {
                        duration: BUTTON_TILES_MORPH_DURATION,
                        delay: 0.0,
                        easing: ease_in_out_quad,
                    },
                    child_behavior: None,
                });
        }
    }
} // mod inner

// Re-export ProteusApp at crate root so wasm-bindgen can generate bindings.
#[cfg(target_arch = "wasm32")]
pub use inner::ProteusApp;
