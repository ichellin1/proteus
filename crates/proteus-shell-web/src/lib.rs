//! `proteus-shell-web` — WebGL2 / WebGPU WASM shell.
//!
//! M12.5 Step 9 cutover: this used to carry its own ~8800-line hand-rolled
//! copy of the reference demo's entire app/UI logic — every `advance_*`/
//! `start_*`/`settle_*` method, one per screen, kept "scene-for-scene"
//! identical to `proteus-shell-native`'s own copy by hand. All of that is
//! now `proteus_demo::Demo` — a shared, shell-agnostic crate built once and
//! also linked by `proteus-shell-native` — and this file is just the wasm
//! platform glue around it: WebGPU/WebGL2 device setup on a `<canvas>`,
//! decoding image bytes and uploading them to the GPU (`Demo` is headless,
//! see its own crate-root doc for why), forwarding JS pointer events to
//! `Demo`'s `pointer_*` methods, and the `take_pending_*`/`set_*` injection
//! points `Demo` queues but can't service itself (video texture upload,
//! gallery/video fetch — done in JS, see `www/index.html`).
//!
//! Unlike the native cutover (which promoted `proteus-demo/examples/
//! native_preview/` — a harness that had already proven the exact shell
//! shape needed), there was no wasm equivalent to promote here: this
//! crate's own real, working `www/index.html` (untouched by this cutover,
//! beyond its wasm method names — see the module docs on the JS-facing
//! methods below for the renames) was the reference instead.
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
//! `Backends::GL` (WebGL2-only) hangs at `request_adapter` in Chrome builds
//! where native WebGPU is also present — the GL adapter future stalls
//! waiting on internal wgpu machinery that expects the WebGPU path.
//! `Backends::all()` resolves this: wgpu picks BROWSER_WEBGPU first (fast,
//! no stall), and falls back to WebGL2 if the browser doesn't support
//! WebGPU.
//!
//! ## Video: real HLS, not a local file
//!
//! The one place this shell's platform glue is genuinely different from
//! native's, not just a mechanical repeat: there's no background decode
//! thread on wasm32 — the browser's own `<video>`/`MediaSource` element is
//! the decoder (`www/index.html`'s `playHls`, a real, working HLS client:
//! manifest parsing, segment buffering, Safari/Chrome quirk handling).
//! `Demo` still only ever signals *when* via `take_pending_video_start`/
//! `take_pending_video_stop`/`take_pending_video_cancel` (polled once per
//! `tick()`, same "polled take" shape native's own `apply_video_actions`
//! uses) — this file's [`ProteusApp::start_video`]/[`ProteusApp::
//! push_video_frame`] are the GPU-texture-upload half `Demo` never touches,
//! called from JS once metadata/frames are ready.
//!
//! `debug_video_state` (an old, dev-only introspection helper for
//! diagnosing the video-loading fallback on a browser this project's own
//! tooling can't drive) is deliberately **not** ported — `Demo` has no
//! public introspection surface for its own internal `state`/`video_tiles`
//! fields, and manufacturing one solely for this one dev tool wasn't worth
//! it. Revisit if it's actually needed again.

use wasm_bindgen::prelude::*;

use bevy_ecs::prelude::{Entity, Without};
use glam::Vec2;

use proteus_demo::Demo;
use proteus_render::{
    validate_atlas_config, AtlasConfig, FontAtlas, GpuContext, QuadPipeline, TextureId,
};
use proteus_sdk::TextureHandle;
use proteus_ui::{BakedImage, BakedText, Image, Text, TextureRef};

// ---------------------------------------------------------------------------
// Legacy stub (always compiled — keep for backward compatibility)
// ---------------------------------------------------------------------------

/// Legacy entry point. The full `ProteusApp` class is preferred.
#[wasm_bindgen]
pub async fn proteus_init(canvas_id: String) -> Result<(), JsValue> {
    ProteusApp::init(canvas_id).await.map(|_| ())
}

// ---------------------------------------------------------------------------
// Baking — decoding raw bytes into GPU-resident atlas content
// ---------------------------------------------------------------------------

/// Real photos routinely arrive far larger than any on-screen footprint
/// this demo needs; cap before packing into `main_atlas` (2048×2048, shared
/// with baked text). Also used for the background. Mirrors
/// `proteus-shell-native::MAX_IMAGE_SIDE`.
const MAX_IMAGE_SIDE: u32 = 400;
/// Source frame art (208×288) is noticeably larger than the mark's
/// on-screen footprint, so it's downscaled before packing into
/// `main_atlas`, same reasoning as any other baked image. Mirrors
/// `proteus-shell-native::LOGO_FRAME_MAX_SIDE`.
const LOGO_FRAME_MAX_SIDE: u32 = 220;
/// `www/index.html` fetches exactly 19 frames per (light/dark) set —
/// `logo_frames`/`logo_frames_dark` below are fixed-size slot arrays this
/// wide so a frame can land in its own slot regardless of fetch order (see
/// [`ProteusApp::add_logo_frame`]'s own doc).
const LOGO_FRAME_COUNT: usize = 19;
/// The shared 400px cap (`MAX_IMAGE_SIDE`) is sized for 12 simultaneous
/// grid tiles; only one hires image is ever resident at a time, so it can
/// be bigger. Mirrors `proteus-shell-native::GALLERY_LARGE_IMAGE_MAX_SIDE`.
const GALLERY_LARGE_IMAGE_MAX_SIDE: u32 = 900;
/// Matches `screens::gallery::TILE_COUNT` (not itself part of
/// `proteus-demo`'s public API, so re-stated here).
const GALLERY_TILE_COUNT: usize = 12;

/// The page background visible behind/around content before the real
/// background image loads (and at the surface's own clear color every
/// frame, underneath it). A light lavender, not black — this app's actual
/// resting palette, identical to `proteus-shell-native::BG_COLOR`.
const BG_COLOR: wgpu::Color = wgpu::Color {
    r: 0xCD as f64 / 255.0,
    g: 0xC7 as f64 / 255.0,
    b: 0xED as f64 / 255.0,
    a: 1.0,
};

/// Decodes and uploads any `Image` component that doesn't have a
/// `BakedImage` yet, at up to `max_side` pixels — the shared logic behind
/// both [`bake_pending_images`] (the 400px-capped generic pass) and
/// [`bake_gallery_hires_image`] (a dedicated, bigger-capped pass for one
/// specific entity). Mirrors `proteus-shell-native`'s identical helper of
/// the same name.
fn bake_images(
    world: &mut bevy_ecs::world::World,
    queue: &wgpu::Queue,
    max_side: u32,
    entities: impl Iterator<Item = (Entity, std::sync::Arc<[u8]>)>,
) {
    for (entity, bytes) in entities {
        let decoded = match proteus_render::decode_image(&bytes) {
            Ok(decoded) => decoded,
            Err(e) => {
                log::warn!("bake_images: entity {entity:?}: {e}");
                continue;
            }
        };
        let decoded = proteus_render::resize_to_fit(decoded, max_side);

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(decoded.width, decoded.height, false)
            else {
                log::warn!(
                    "bake_images: main_atlas full — could not register {}x{} image for entity {entity:?}",
                    decoded.width,
                    decoded.height,
                );
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &decoded.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            (uv, texture_id)
        };

        world.entity_mut(entity).insert((
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

/// Rasterizes and uploads any `Text` component that doesn't have a
/// `BakedText` yet. Mirrors `proteus-shell-native`'s identical helper.
fn bake_pending_text(
    world: &mut bevy_ecs::world::World,
    font_atlas: &mut FontAtlas,
    queue: &wgpu::Queue,
) {
    let pending: Vec<(Entity, Text)> = {
        let mut query = world.query_filtered::<(Entity, &Text), Without<BakedText>>();
        query.iter(world).map(|(e, t)| (e, t.clone())).collect()
    };

    for (entity, text) in pending {
        let Some(glyphs) =
            font_atlas.rasterize_text_tracked(&text.content, text.size_px, text.letter_spacing_px)
        else {
            continue;
        };

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(glyphs.width, glyphs.height, false)
            else {
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &glyphs.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            (uv, texture_id)
        };

        world.entity_mut(entity).insert((
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

/// Decodes and uploads any `Image` component that doesn't have a
/// `BakedImage` yet — the generic counterpart to `bake_pending_text`, for
/// any entity carrying raw image bytes (background, video tile box art,
/// gallery photos), capped at `MAX_IMAGE_SIDE`. Call
/// [`bake_gallery_hires_image`] *first* each frame — once that's baked the
/// hires overlay at its own, bigger cap, this pass's `Without<BakedImage>`
/// filter naturally skips it, instead of re-baking it here at the wrong
/// (smaller) size.
fn bake_pending_images(world: &mut bevy_ecs::world::World, queue: &wgpu::Queue) {
    let pending: Vec<(Entity, std::sync::Arc<[u8]>)> = {
        let mut query = world.query_filtered::<(Entity, &Image), Without<BakedImage>>();
        query
            .iter(world)
            .map(|(e, img)| (e, img.bytes.clone()))
            .collect()
    };
    bake_images(world, queue, MAX_IMAGE_SIDE, pending.into_iter());
}

/// Dedicated bake step for `Demo::gallery_hires_overlay()`, mirroring
/// `bake_pending_images` but resizing to `GALLERY_LARGE_IMAGE_MAX_SIDE`
/// instead of `MAX_IMAGE_SIDE`. Must run before `bake_pending_images` each
/// frame — see that function's doc.
fn bake_gallery_hires_image(demo: &mut Demo, queue: &wgpu::Queue) {
    let entity = demo.gallery_hires_overlay().id();
    let world = demo.app_mut().world_mut();
    if world.get::<BakedImage>(entity).is_some() {
        return;
    }
    let Some(bytes) = world.get::<Image>(entity).map(|img| img.bytes.clone()) else {
        return;
    };
    bake_images(
        world,
        queue,
        GALLERY_LARGE_IMAGE_MAX_SIDE,
        std::iter::once((entity, bytes)),
    );
}

// ---------------------------------------------------------------------------
// Gallery hires request (returned to JS)
// ---------------------------------------------------------------------------

/// A pending hires fetch for the enlarged gallery image — returned by
/// [`ProteusApp::take_pending_gallery_hires_fetch`]. `width`/`height` are
/// the actual on-screen fitted pixel dimensions `Demo` computed, already
/// proportionally capped at `GALLERY_LARGE_IMAGE_MAX_SIDE`; `photo_id`
/// requests the same picsum.photos photo the tile's low-res image already
/// shows, just bigger — `Demo` itself never tracks a photo id (see
/// `ProteusApp::tile_photo_id`'s own doc), so this shell stitches it back
/// in from `Demo::take_pending_gallery_hires_fetch`'s bare `idx`.
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct GalleryHiresFetchRequest {
    pub tile_idx: u32,
    pub width: u32,
    pub height: u32,
    pub photo_id: u32,
}

// ---------------------------------------------------------------------------
// ProteusApp
// ---------------------------------------------------------------------------

/// The currently-playing tile's GPU video texture id (needed by
/// `QuadPipeline::suspend_video` on teardown — see
/// [`ProteusApp::apply_video_stop`]).
struct PlayingVideo {
    texture_id: TextureId,
}

/// Proteus web application. Create via `ProteusApp.init(canvasId)`.
#[wasm_bindgen]
pub struct ProteusApp {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    demo: Demo,
    font_atlas: FontAtlas,
    playing_video: Option<PlayingVideo>,
    /// Which picsum photo id each tile's current low-res fetch landed on —
    /// `Demo::set_gallery_tile_image` never takes a photo id (only bytes +
    /// aspect), so this shell stashes it itself, same as `proteus-shell-
    /// native::RenderState::tile_photo_id` (a plain array there — this is
    /// too, since `GALLERY_TILE_COUNT` is a fixed, known constant).
    tile_photo_id: [Option<u32>; GALLERY_TILE_COUNT],
    /// Slot per frame index (`None` until that frame's own fetch resolves)
    /// — `Demo::set_logo_frames` wants the whole set as one `Vec` in order,
    /// but frames arrive one fetch at a time and not necessarily in index
    /// order (see [`ProteusApp::add_logo_frame`]'s own doc for why this
    /// shell still calls `Demo::set_logo_frames` incrementally anyway,
    /// deviating a little from that method's own "call once" doc).
    logo_frames: Vec<Option<TextureHandle>>,
    /// Dark-treatment counterpart of `logo_frames`, feeding `Demo::
    /// set_loading_logo_frames_dark` the same way.
    logo_frames_dark: Vec<Option<TextureHandle>>,
}

impl ProteusApp {
    /// Shared by [`Self::add_logo_frame`]/[`Self::add_logo_frame_dark`]:
    /// decodes+registers+uploads one frame's bytes, stores it in `slots[
    /// frame_idx]`, then calls `set_on_demo` with every slot that has
    /// landed *so far*, in index order (gaps for any not-yet-arrived frame
    /// simply skipped) — so the very first frame to land is already usable
    /// immediately (matching this shell's own former per-frame `add_logo_
    /// frame`'s "shows frame 0 the instant it's baked" behavior, rather
    /// than waiting for all 19 fetches — the worse choice specifically on
    /// a slow connection, exactly what this project's own staging deploy
    /// exists to surface). Calling `Demo::set_logo_frames`/`set_loading_
    /// logo_frames_dark` more than once (its own doc says "once, before
    /// the first tick") only ever resets the idle sweep's own phase back
    /// to frame 0 — harmless: this only happens during the ~1s the Splash
    /// screen's own `INTRO_DELAY_SECS` already keeps it invisible, on a
    /// typical connection well before any of it is visible at all.
    fn add_frame(
        &mut self,
        label: &str,
        slots: &mut [Option<TextureHandle>],
        frame_idx: u32,
        bytes: &[u8],
        set_on_demo: impl FnOnce(&mut Demo, Vec<TextureHandle>),
    ) {
        let frame_idx = frame_idx as usize;
        let decoded = match proteus_render::decode_image(bytes) {
            Ok(decoded) => decoded,
            Err(e) => {
                log::warn!("{label} frame {frame_idx}: could not decode: {e}");
                return;
            }
        };
        let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);
        let texture_id = {
            let Some(mut pipeline) = self
                .demo
                .app_mut()
                .world_mut()
                .get_resource_mut::<QuadPipeline>()
            else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "{label} frame {frame_idx}: main_atlas full — could not register {}x{}",
                    decoded.width,
                    decoded.height,
                );
                return;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(&self.queue, placement, &decoded.rgba_pixels);
            texture_id
        };
        let handle = self.demo.app_mut().texture(texture_id);
        if let Some(slot) = slots.get_mut(frame_idx) {
            *slot = Some(handle);
        }
        set_on_demo(&mut self.demo, slots.iter().filter_map(|s| *s).collect());
    }
}

#[wasm_bindgen]
impl ProteusApp {
    /// Initialise Proteus on the `<canvas>` element with the given `id`.
    ///
    /// Returns a JS `Promise<ProteusApp>`. Call `tick(dt_ms)` inside
    /// `requestAnimationFrame` to drive the render loop.
    #[wasm_bindgen]
    pub async fn init(canvas_id: String) -> Result<ProteusApp, JsValue> {
        console_error_panic_hook::set_once();
        wasm_logger::init(wasm_logger::Config::default());

        log::info!("ProteusApp::init — canvas #{canvas_id}");

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

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| JsValue::from_str(&format!("create_surface: {e}")))?;

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

        let surface_caps = surface.get_capabilities(&adapter);
        // Explicitly avoid an sRGB-tagged surface format — see the matching
        // comment in proteus-shell-native/src/main.rs. This build's WebGL2
        // surface doesn't offer an sRGB-capable format today anyway (falls
        // back to plain `Bgra8Unorm`), but picking it explicitly rather
        // than by accident guards against a future WebGPU backend
        // reintroducing the native/web color mismatch this was the actual
        // cause of.
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

        let mut demo = Demo::new();
        demo.app_mut().world_mut().insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        demo.app_mut().world_mut().insert_resource(pipeline);
        // No devicePixelRatio scaling in this shell (see the module doc on
        // `www/index.html`'s own canvas-sizing convention) — canvas
        // resolution *is* CSS/logical pixels 1:1, so `Demo`'s own logical
        // units need no conversion here, unlike native's `scale_factor`.
        demo.set_viewport_size(Vec2::new(width as f32, height as f32));

        Ok(Self {
            surface,
            surface_config,
            device,
            queue,
            demo,
            font_atlas: FontAtlas::with_embedded_font(),
            playing_video: None,
            tile_photo_id: [None; GALLERY_TILE_COUNT],
            logo_frames: vec![None; LOGO_FRAME_COUNT],
            logo_frames_dark: vec![None; LOGO_FRAME_COUNT],
        })
    }

    /// Advance one frame. `dt_ms` is the elapsed time in milliseconds (pass
    /// `performance.now()` delta from the rAF callback).
    #[wasm_bindgen]
    pub fn tick(&mut self, dt_ms: f32) {
        // Clamped to 20fps-equivalent, matching `proteus-shell-native::
        // render`'s identical clamp — see that file's own doc for why: any
        // real stall between frames (most visibly the very first one, after
        // asset baking/GPU warm-up) would otherwise get fed straight into
        // `Demo::tick` as one giant `dt`, easily enough to blow through
        // Splash's entire delay+fade+hold budget (~3.1s) in a single tick.
        let dt = (dt_ms / 1000.0).min(0.05);
        self.demo.tick(dt); // Demo::tick already calls refresh_cascades internally.

        bake_pending_text(
            self.demo.app_mut().world_mut(),
            &mut self.font_atlas,
            &self.queue,
        );
        bake_gallery_hires_image(&mut self.demo, &self.queue);
        bake_pending_images(self.demo.app_mut().world_mut(), &self.queue);

        let instances = proteus_ui::collect_instances(self.demo.app_mut().world_mut());

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

        let mut pipeline = self
            .demo
            .app_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>();
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
    /// CSS pixels. Call this from a `ResizeObserver` callback.
    #[wasm_bindgen]
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.demo
            .app_mut()
            .world_mut()
            .resource::<QuadPipeline>()
            .set_view_projection(
                &self.queue,
                QuadPipeline::ortho(width as f32, height as f32),
            );
        self.demo
            .set_viewport_size(Vec2::new(width as f32, height as f32));
    }

    // ── Pointer event entry points (called from JS) ────────────────────────

    /// Report a pointer move. `x`/`y` are CSS pixels (origin top-left).
    /// Converts to world-space (origin center, Y up) before forwarding to
    /// `Demo`. Call from `canvas.addEventListener('mousemove', ...)`.
    #[wasm_bindgen]
    pub fn on_mouse_move(&mut self, x: f32, y: f32) {
        let w = self.surface_config.width as f32;
        let h = self.surface_config.height as f32;
        self.demo
            .pointer_moved(Some(Vec2::new(x - w / 2.0, h / 2.0 - y)));
    }

    /// Report that the pointer has left the canvas.
    /// Call from `canvas.addEventListener('mouseleave', ...)`.
    #[wasm_bindgen]
    pub fn on_mouse_leave(&mut self) {
        self.demo.pointer_moved(None);
    }

    /// Report a primary-button press.
    /// Call from `canvas.addEventListener('mousedown', ...)`.
    #[wasm_bindgen]
    pub fn on_mouse_down(&mut self) {
        self.demo.pointer_pressed();
    }

    /// Report a primary-button release.
    /// Call from `canvas.addEventListener('mouseup', ...)`.
    #[wasm_bindgen]
    pub fn on_mouse_up(&mut self) {
        self.demo.pointer_released();
    }

    // ── Video (real HLS via <video>/MediaSource — see the module doc) ──────

    /// Returns the tile index playback should start for, once, or
    /// `undefined` if nothing changed since the last call. Call once per
    /// `tick()`; on `Some`, load/play that tile's HLS stream and — once
    /// `loadedmetadata` fires — call [`Self::start_video`].
    #[wasm_bindgen]
    pub fn take_pending_video_start(&mut self) -> Option<u32> {
        self.demo.take_pending_video_start().map(|i| i as u32)
    }

    /// Returns `true` once, the first `tick()` after the screen was clicked
    /// to stop playback — the corresponding `<video>` element should be
    /// paused. `Demo`-side texture/component cleanup has already happened
    /// by the time this flips true; this call also releases the GPU video
    /// texture, since `Demo` itself never touches the GPU.
    #[wasm_bindgen]
    pub fn take_pending_video_stop(&mut self) -> bool {
        let stopped = self.demo.take_pending_video_stop();
        if stopped {
            if let Some(playing) = self.playing_video.take() {
                self.demo
                    .app_mut()
                    .world_mut()
                    .resource_mut::<QuadPipeline>()
                    .suspend_video(&self.device, playing.texture_id);
            }
        }
        stopped
    }

    /// Returns `true` once, the first `tick()` after a video load timed
    /// out — unlike `take_pending_video_stop`, no cleanup has happened
    /// here (the screen stays on `VideoScreen`, now showing the error
    /// text); this just tells JS to abort whatever HLS segment fetch is
    /// still in flight so a failed load stops burning bandwidth in the
    /// background. See `Demo::take_pending_video_cancel`'s own doc.
    #[wasm_bindgen]
    pub fn take_pending_video_cancel(&mut self) -> bool {
        self.demo.take_pending_video_cancel()
    }

    /// Sizes the pipeline's video texture. Call once `<video>`'s
    /// `loadedmetadata` event has fired, passing its `videoWidth`/
    /// `videoHeight` — the entity-side `VideoPlayer`/`VideoCrossfade`
    /// attachment already happened synchronously inside `Demo` itself, the
    /// instant the tile was clicked (see `Demo::take_pending_video_start`'s
    /// own doc); this is purely the GPU-texture half `Demo` never touches,
    /// mirroring `proteus-shell-native::apply_video_actions` exactly (it
    /// doesn't touch any entity either, for the same reason).
    ///
    /// Rejects `0×0` outright rather than trusting it — some browsers'
    /// `loadedmetadata` can fire on an MSE-backed `<video>` before
    /// `videoWidth`/`videoHeight` are actually populated, and a `0×0` wgpu
    /// texture is itself invalid.
    #[wasm_bindgen]
    pub fn start_video(&mut self, tile_idx: u32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            log::warn!("start_video: rejecting 0×0 dimensions for tile {tile_idx}");
            return;
        }
        let (texture_id, _sender) = self
            .demo
            .app_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>()
            .init_video(&self.device, &self.queue, width, height);
        // `_sender` (the BYOV channel's sending half) goes unused on
        // wasm32 — `push_video_frame` uploads directly instead of routing
        // through the channel, since blocking on a full bounded channel
        // would deadlock with no second thread free to drain it.
        self.playing_video = Some(PlayingVideo { texture_id });
    }

    /// Uploads one decoded RGBA frame (`width×height×4` bytes, matching
    /// whatever `start_video` was called with) straight to the video
    /// texture, and latches `Demo::set_video_first_frame_shown` — drives
    /// `Demo::advance_video_loading`'s loading-dots visibility. Call once
    /// per `<video>` `requestVideoFrameCallback`.
    #[wasm_bindgen]
    pub fn push_video_frame(&mut self, rgba: &[u8]) {
        if self.playing_video.is_some() {
            self.demo
                .app_mut()
                .world_mut()
                .resource::<QuadPipeline>()
                .upload_video_frame(&self.queue, rgba);
            self.demo.set_video_first_frame_shown();
        }
    }

    // ── Photo gallery (M12) ──────────────────────────────────────────────

    /// Returns `Some(side_px)` exactly once per `Loading` entry — the
    /// square pixel size JS should request each of the 12 gallery images
    /// at. `None` if nothing changed since the last call.
    #[wasm_bindgen]
    pub fn take_pending_gallery_fetch(&mut self) -> Option<u32> {
        self.demo
            .take_pending_gallery_fetch()
            .map(|r| r.tile_side_px)
    }

    /// Attaches a fetched gallery image. Call once per tile, after JS has
    /// fetched its bytes. `photo_id` is stashed (`tile_photo_id`) for
    /// later reuse if this tile gets enlarged — `Demo::
    /// set_gallery_tile_image` only ever needs the aspect ratio, not the
    /// id itself.
    #[wasm_bindgen]
    pub fn set_gallery_tile_image(
        &mut self,
        tile_idx: u32,
        bytes: &[u8],
        photo_id: u32,
        aspect_w: f32,
        aspect_h: f32,
    ) {
        self.tile_photo_id[tile_idx as usize] = Some(photo_id);
        self.demo.set_gallery_tile_image(
            tile_idx as usize,
            bytes.to_vec(),
            Vec2::new(aspect_w, aspect_h),
        );
    }

    /// Returns the pending hires fetch for the enlarged gallery image, once
    /// per `GalleryImage` entry — polled once per `tick()` from
    /// `index.html`, same "polled take" shape as
    /// [`Self::take_pending_gallery_fetch`].
    #[wasm_bindgen]
    pub fn take_pending_gallery_hires_fetch(&mut self) -> Option<GalleryHiresFetchRequest> {
        let request = self.demo.take_pending_gallery_hires_fetch()?;
        Some(GalleryHiresFetchRequest {
            tile_idx: request.idx as u32,
            width: request.width_px,
            height: request.height_px,
            photo_id: self.tile_photo_id[request.idx].unwrap_or(0),
        })
    }

    /// `true` exactly once whenever the hires fetch was just cancelled
    /// (backing out of `GalleryImage` before the hires image arrived) — JS
    /// should `.abort()` its in-flight fetch's `AbortController`, if any,
    /// on seeing this.
    #[wasm_bindgen]
    pub fn take_pending_gallery_hires_cancel(&mut self) -> bool {
        self.demo.take_pending_gallery_hires_cancel()
    }

    /// Attaches the fetched hires bytes to the enlarged gallery view.
    #[wasm_bindgen]
    pub fn set_gallery_hires_image(&mut self, tile_idx: u32, bytes: &[u8]) {
        self.demo
            .set_gallery_hires_image(tile_idx as usize, bytes.to_vec());
    }

    // ── One-shot asset injection points ─────────────────────────────────

    /// Attaches box-cover art to tile `tile_idx`.
    #[wasm_bindgen]
    pub fn set_tile_image(&mut self, tile_idx: u32, bytes: &[u8]) {
        self.demo.set_tile_image(tile_idx as usize, bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_background_image(&mut self, bytes: &[u8]) {
        self.demo.set_background_image(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_background_image_dark(&mut self, bytes: &[u8]) {
        self.demo.set_background_image_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_home_icon(&mut self, bytes: &[u8]) {
        self.demo.set_nav_home_icon(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_home_icon_dark(&mut self, bytes: &[u8]) {
        self.demo.set_nav_home_icon_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_home_icon_selected(&mut self, bytes: &[u8]) {
        self.demo.set_nav_home_icon_selected(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_home_icon_selected_dark(&mut self, bytes: &[u8]) {
        self.demo.set_nav_home_icon_selected_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_back_icon(&mut self, bytes: &[u8]) {
        self.demo.set_nav_back_icon(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_back_icon_dark(&mut self, bytes: &[u8]) {
        self.demo.set_nav_back_icon_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_logo_lockup(&mut self, bytes: &[u8]) {
        self.demo.set_nav_logo_lockup(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_nav_logo_lockup_dark(&mut self, bytes: &[u8]) {
        self.demo.set_nav_logo_lockup_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_theme_sun_icon(&mut self, bytes: &[u8]) {
        self.demo.set_theme_sun_icon(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_theme_sun_icon_dark(&mut self, bytes: &[u8]) {
        self.demo.set_theme_sun_icon_dark(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_theme_moon_icon(&mut self, bytes: &[u8]) {
        self.demo.set_theme_moon_icon(bytes.to_vec());
    }
    #[wasm_bindgen]
    pub fn set_theme_moon_icon_dark(&mut self, bytes: &[u8]) {
        self.demo.set_theme_moon_icon_dark(bytes.to_vec());
    }

    /// Bakes one frame (1-indexed to match `index.html`'s own
    /// `frame-NN.png` file naming, 1..=19) of the Splash logo's idle
    /// hatch-sweep animation and hands the whole set-so-far to `Demo::
    /// set_logo_frames` — see [`ProteusApp::add_frame`]'s own doc for why
    /// incrementally, and why that's fine.
    #[wasm_bindgen]
    pub fn add_logo_frame(&mut self, frame_idx: u32, bytes: &[u8]) {
        let mut slots = std::mem::take(&mut self.logo_frames);
        self.add_frame("logo", &mut slots, frame_idx, bytes, |demo, frames| {
            demo.set_logo_frames(frames)
        });
        self.logo_frames = slots;
    }

    /// Color-dark counterpart of `add_logo_frame` — feeds `Demo::
    /// set_loading_logo_frames_dark` instead (the `Loading` screen's own
    /// dark-treatment logo; the Splash logo above never has to
    /// theme-crossfade).
    #[wasm_bindgen]
    pub fn add_logo_frame_dark(&mut self, frame_idx: u32, bytes: &[u8]) {
        let mut slots = std::mem::take(&mut self.logo_frames_dark);
        self.add_frame("logo dark", &mut slots, frame_idx, bytes, |demo, frames| {
            demo.set_loading_logo_frames_dark(frames)
        });
        self.logo_frames_dark = slots;
    }
}
