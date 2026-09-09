//! `proteus-shell-native` — native desktop entry point.
//!
//! M12.5 Step 9 cutover: this used to carry its own ~8700-line hand-rolled
//! copy of the reference demo's entire app/UI logic (`RenderState`'s own
//! `advance_*`/`start_*`/`settle_*` methods, one per screen). All of that is
//! now `proteus_demo::Demo` — a shared, shell-agnostic crate built once and
//! also linked by `proteus-shell-web` — and this file is just the native
//! platform glue around it: window/GPU setup, decoding image/video/font
//! bytes and uploading them to the GPU (`Demo` is headless, see its own
//! crate-root doc for why), forwarding winit input events to `Demo`'s
//! `pointer_*` methods, and the real asset I/O (`std::fs::read`, `ureq` for
//! the gallery fetch, `ffmpeg` via [`mp4_player`] for video decode) `Demo`'s
//! own `take_pending_*`/`set_*` injection points exist to hand off.
//!
//! Ported from `proteus-demo/examples/native_preview/main.rs`, which proved
//! this exact shell shape against `Demo` throughout M12.5/M12.5.5 (using
//! this crate's own asset files, reached via a relative path, since it
//! didn't own canonical assets itself) — now promoted to the real thing,
//! pointed at its own local `images/`/`assets/` directly. That preview
//! harness (and its own copies of [`gallery_fetch`]/[`mp4_player`],
//! superseded by this crate's originals) is deleted as part of this same
//! cutover.
//!
//! ## Frame order each tick (`RenderState::render`)
//!
//! 1. Compute delta time (capped at 50ms — see that clamp's own doc for why).
//! 2. `Demo::tick(dt)` — the entire app/UI update (hit test, transitions,
//!    every `advance_*`), headless.
//! 3. Bake any `Text`/`Image` entities that don't have a baked counterpart
//!    yet, and apply this tick's texture-churn/video/gallery-fetch actions —
//!    all shell-owned GPU/subprocess/network work `Demo` queued but can't do
//!    itself.
//! 4. Collect visible `QuadState`s → `QuadInstance`s and run the GPU render
//!    pass.

mod gallery_fetch;
mod mp4_player;

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use bevy_ecs::prelude::{Entity, Without};
use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use proteus_demo::Demo;
use proteus_render::{
    validate_atlas_config, AtlasConfig, FontAtlas, GpuContext, QuadPipeline, TextureId,
};
use proteus_sdk::{Proteus, TextureHandle};
use proteus_ui::{BakedImage, BakedText, Image, Text, TextureRef};

// ---------------------------------------------------------------------------
// Real asset paths
// ---------------------------------------------------------------------------

/// `assets/videos/{tiger,sintel_fixed,jellyfish_fixed}.mp4`. Index order
/// matches `screens::video_tiles`' left/center/right tiles (same order as
/// `TILE_IMAGE_PATHS`).
const TILE_VIDEO_PATHS: [&str; 3] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/assets/videos/tiger.mp4"),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/videos/sintel_fixed.mp4"
    ),
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/videos/jellyfish_fixed.mp4"
    ),
];

/// `proteus-shell-native::LOGO_FRAME_COUNT` (the original demo's own
/// constant, before this cutover) — see `bake_logo_frames`'s doc.
const LOGO_FRAME_COUNT: usize = 19;
/// Source frame art (208×288) is noticeably larger than the mark's
/// on-screen footprint, so it's downscaled before packing into
/// `main_atlas`, same reasoning as any other baked image.
const LOGO_FRAME_MAX_SIDE: u32 = 220;
/// Real photos routinely arrive far larger than any on-screen footprint
/// this demo needs; cap before packing into `main_atlas` (2048×2048, shared
/// with baked text). Also used for the background.
const MAX_IMAGE_SIDE: u32 = 400;

/// `images/logo/frame-01.png` … `frame-19.png`.
fn logo_frame_path(n: usize) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo"))
        .join(format!("frame-{n:02}.png"))
}

/// `images/logo/frame-01-dark.png` … `frame-19-dark.png` — the Color-dark
/// treatment set `Demo::set_loading_logo_frames_dark` wants.
fn logo_frame_dark_path(n: usize) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo"))
        .join(format!("frame-{n:02}-dark.png"))
}

const BG_IMAGE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/bg/ocean-blur.jpg");
const BG_IMAGE_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/bg/ocean-blur-dark.jpg");

/// `images/icons/*.png` and `images/logo/lockup*.png`. Order matches
/// `Demo::set_nav_*`'s 8 setters.
const NAV_HOME_ICON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/home-idle.png");
const NAV_HOME_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-idle-dark.png"
);
const NAV_HOME_ICON_SELECTED_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-selected.png"
);
const NAV_HOME_ICON_SELECTED_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/home-selected-dark.png"
);
const NAV_BACK_ICON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/back-idle.png");
const NAV_BACK_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/back-idle-dark.png"
);
const NAV_LOGO_LOCKUP_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo/lockup.png");
const NAV_LOGO_LOCKUP_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/logo/lockup-dark.png");

/// `images/icons/{sun,moon}-*.png` — order matches `Demo::set_theme_*`'s 4
/// setters. Filenames don't map onto light/dark the way every other themed
/// pair here does — see `proteus_demo::screens::theme`'s own module doc for
/// the inverted asset-role convention `sun`/`sun_dark` use.
const THEME_SUN_ICON_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/sun-idle-dark.png"
);
const THEME_SUN_ICON_DARK_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/sun-selected.png");
const THEME_MOON_ICON_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/icons/moon-idle.png");
const THEME_MOON_ICON_DARK_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/images/icons/moon-selected-dark.png"
);

/// `images/{tiger,sintel,jellyfish}.jpg`. Index order matches
/// `screens::video_tiles`' left/center/right tiles.
const TILE_IMAGE_PATHS: [&str; 3] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/tiger.jpg"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/sintel.jpg"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/images/jellyfish.jpg"),
];

/// The page background visible behind/around content before the real
/// background image loads (and at the surface's own clear color every
/// frame, underneath it). A light lavender, not black — this app's actual
/// resting palette.
const BG_COLOR: wgpu::Color = wgpu::Color {
    r: 0xCD as f64 / 255.0,
    g: 0xC7 as f64 / 255.0,
    b: 0xED as f64 / 255.0,
    a: 1.0,
};

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
        _window_id: WindowId,
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
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::CursorMoved { position, .. } => {
                // `position` (like `surface_config.width`/`height`) is in
                // physical pixels; `Demo`'s own coordinate system is logical
                // pixels (see `RenderState::new`'s comment on the view
                // projection), so both need the same scale_factor division
                // to land in the same space.
                let scale_factor = state.window.scale_factor() as f32;
                let w = state.surface_config.width as f32 / scale_factor;
                let h = state.surface_config.height as f32 / scale_factor;
                let wx = (position.x as f32 / scale_factor) - w / 2.0;
                let wy = h / 2.0 - (position.y as f32 / scale_factor);
                state.demo.pointer_moved(Some(Vec2::new(wx, wy)));
            }
            WindowEvent::CursorLeft { .. } => state.demo.pointer_moved(None),
            WindowEvent::MouseInput {
                state: btn_state,
                button: MouseButton::Left,
                ..
            } => match btn_state {
                ElementState::Pressed => state.demo.pointer_pressed(),
                ElementState::Released => state.demo.pointer_released(),
            },
            WindowEvent::RedrawRequested => state.render(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Baking — decoding raw bytes into GPU-resident atlas content
// ---------------------------------------------------------------------------

/// Pre-bakes all 19 frames of one logo animation set into `main_atlas`,
/// eternal — they must survive the whole idle loop, not just whichever
/// frame is currently referenced (see the eviction-safety note on
/// `TextureRegistry::register_static`), so unlike `bake_pending_text` this
/// can't use a lazy per-entity register/evict path. Shared by both the
/// light set (`logo_frame_path`, feeds `Demo::set_logo_frames`) and the
/// dark set (`logo_frame_dark_path`, feeds `Demo::set_loading_logo_frames_
/// dark`) — `path_fn`/`label` are the only difference between the two.
/// Missing/unreadable frames degrade gracefully — same convention as any
/// other image asset.
fn bake_logo_frames(
    app: &mut Proteus,
    queue: &wgpu::Queue,
    path_fn: impl Fn(usize) -> std::path::PathBuf,
    label: &str,
) -> Vec<TextureHandle> {
    let mut frames = Vec::with_capacity(LOGO_FRAME_COUNT);
    for n in 1..=LOGO_FRAME_COUNT {
        let path = path_fn(n);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                log::warn!("{label} frame {n}: could not read {path:?}: {e}");
                continue;
            }
        };
        let decoded = match proteus_render::decode_image(&bytes) {
            Ok(decoded) => decoded,
            Err(e) => {
                log::warn!("{label} frame {n}: could not decode {path:?}: {e}");
                continue;
            }
        };
        let decoded = proteus_render::resize_to_fit(decoded, LOGO_FRAME_MAX_SIDE);

        let texture_id = {
            let Some(mut pipeline) = app.world_mut().get_resource_mut::<QuadPipeline>() else {
                return frames;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(decoded.width, decoded.height, true)
            else {
                log::warn!(
                    "{label} frame {n}: main_atlas full — could not register {}x{}",
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
            texture_id
        };
        frames.push(app.texture(texture_id));
    }
    if frames.is_empty() {
        log::warn!(
            "{label} animation: no frames loaded from {:?} — renders as a blank transparent quad",
            path_fn(1).parent().unwrap()
        );
    }
    frames
}

/// Rasterizes and uploads any `Text` component that doesn't have a
/// `BakedText` yet. `proteus-sdk`/`Demo` don't do this themselves (see
/// `proteus-demo`'s crate-root doc: baking stays a shell concern), so a
/// host application that wants to actually *see* text has to do it.
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
/// `BakedImage` yet, at up to `max_side` pixels — the shared logic behind
/// both [`bake_pending_images`] (the 400px-capped generic pass) and
/// [`bake_gallery_hires_image`] (a dedicated, bigger-capped pass for one
/// specific entity).
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
/// instead of `MAX_IMAGE_SIDE` — the shared 400px cap is sized for 12
/// simultaneous grid tiles; only one hires image is ever resident at a
/// time, so it can be bigger (900px). Must run before `bake_pending_images`
/// each frame — see that function's doc.
fn bake_gallery_hires_image(demo: &mut Demo, queue: &wgpu::Queue) {
    const GALLERY_LARGE_IMAGE_MAX_SIDE: u32 = 900;
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

/// Registers each of this tick's Texture Churn updates (see
/// `Demo::take_pending_texture_churn`'s doc) into `main_atlas` and swaps it
/// onto its slot — the GPU-touching half of the churn cycle `Demo` itself
/// can't do (it's headless). Skips a cycle rather than erroring if the
/// atlas is full even after eviction.
fn apply_texture_churn(demo: &mut Demo, queue: &wgpu::Queue) {
    for update in demo.take_pending_texture_churn() {
        let app = demo.app_mut();
        let texture_id = {
            let Some(mut pipeline) = app.world_mut().get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(update.width, update.height, false)
            else {
                log::warn!(
                    "apply_texture_churn: main_atlas full — could not register {}x{}",
                    update.width,
                    update.height,
                );
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &update.rgba);
            texture_id
        };
        let texture = app.texture(texture_id);
        update.handle.set_texture(app, texture);
    }
}

// ---------------------------------------------------------------------------
// Video playback
// ---------------------------------------------------------------------------

/// The currently-playing tile's decode thread + its GPU video texture id
/// (needed by `QuadPipeline::suspend_video` on teardown — see
/// `apply_video_actions`).
struct PlayingVideo {
    texture_id: TextureId,
    handle: mp4_player::PlaybackHandle,
}

/// Diagnostic: measures the actual wall-clock gap between successive
/// `frame.present()` calls while a video is playing, to check whether GPU
/// presentation itself is landing at even intervals — independent of
/// `mp4_player`'s decode-thread pacing (verified separately). Only
/// accumulates while a video is playing; logs and resets its own window
/// every `LOG_INTERVAL` presents.
#[derive(Default)]
struct PresentTiming {
    last_present: Option<std::time::Instant>,
    count: u32,
    sum: std::time::Duration,
    max: std::time::Duration,
}

impl PresentTiming {
    const LOG_INTERVAL: u32 = 90;

    /// Call once per `frame.present()` while a video is playing. Logs and
    /// resets its window every `LOG_INTERVAL` calls.
    fn record(&mut self) {
        let now = std::time::Instant::now();
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

    /// Call when playback stops so the next playback session starts a
    /// fresh window instead of measuring the gap across the idle period in
    /// between.
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Starts/stops real `.mp4` decode in response to `Demo`'s
/// `take_pending_video_*` injection points — the GPU/subprocess-touching
/// half of video playback `Demo` itself can't do (it's headless), using
/// [`mp4_player`] and `QuadPipeline::init_video`/`suspend_video`.
fn apply_video_actions(
    demo: &mut Demo,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    playing: &mut Option<PlayingVideo>,
    present_timing: &mut PresentTiming,
) {
    if let Some(idx) = demo.take_pending_video_start() {
        if let Some(old) = playing.take() {
            old.handle.stop();
            let mut pipeline = demo.app_mut().world_mut().resource_mut::<QuadPipeline>();
            pipeline.suspend_video(device, old.texture_id);
            present_timing.reset();
        }
        let path = std::path::Path::new(TILE_VIDEO_PATHS[idx]);
        match mp4_player::probe(path) {
            Ok(dims) => {
                let (texture_id, sender) = {
                    let mut pipeline = demo.app_mut().world_mut().resource_mut::<QuadPipeline>();
                    pipeline.init_video(device, queue, dims.width, dims.height)
                };
                let handle = mp4_player::spawn(path.to_path_buf(), sender, dims.width, dims.height);
                *playing = Some(PlayingVideo { texture_id, handle });
            }
            Err(e) => log::warn!("video {idx}: could not probe {path:?}: {e}"),
        }
    }
    if demo.take_pending_video_stop() {
        if let Some(old) = playing.take() {
            old.handle.stop();
            let mut pipeline = demo.app_mut().world_mut().resource_mut::<QuadPipeline>();
            pipeline.suspend_video(device, old.texture_id);
            present_timing.reset();
        }
    }
}

// ---------------------------------------------------------------------------
// Gallery fetch
// ---------------------------------------------------------------------------

/// 4×3 — matches `screens::gallery::TILE_COUNT` (not itself part of
/// `proteus-demo`'s public API, so re-stated here; `Demo` never needs the
/// count communicated to it, only individual `set_gallery_tile_image`
/// calls by index).
const GALLERY_TILE_COUNT: usize = 12;

/// Which tile a hires fetch is for, plus the channel it'll arrive on.
type GalleryHiresRx = (usize, Receiver<Result<Vec<u8>, String>>);

/// Starts a fresh batch fetch in response to `Demo`'s
/// `take_pending_gallery_fetch` injection point, and drains whichever
/// fetch is currently running — the network-touching half of the gallery
/// `Demo` itself can't do (it's headless), using [`gallery_fetch`].
///
/// `request.tile_side_px` is logical, uncapped (see `GalleryFetchRequest`'s
/// doc) — scaled by `scale_factor` and capped at `MAX_TILE_IMAGE_SIDE_PX`
/// here: without this, tiles fetch at half their intended resolution (or
/// worse) on any HiDPI display.
fn apply_gallery_fetch(
    demo: &mut Demo,
    gallery_fetch_rx: &mut Option<Receiver<gallery_fetch::FetchResult>>,
    tile_photo_id: &mut [Option<u32>; GALLERY_TILE_COUNT],
    scale_factor: f32,
) {
    const MAX_TILE_IMAGE_SIDE_PX: f32 = 400.0;
    if let Some(request) = demo.take_pending_gallery_fetch() {
        let side_px = (request.tile_side_px as f32 * scale_factor)
            .min(MAX_TILE_IMAGE_SIDE_PX)
            .round()
            .max(1.0) as u32;
        *gallery_fetch_rx = Some(gallery_fetch::spawn(GALLERY_TILE_COUNT, side_px));
    }
    let Some(rx) = gallery_fetch_rx else {
        return;
    };
    while let Ok((idx, result)) = rx.try_recv() {
        match result {
            Ok(tile) => {
                // Stashed so a later hires fetch for this same tile (see
                // `apply_gallery_hires_fetch`) re-fetches the *same* photo
                // bigger, not a different random one.
                tile_photo_id[idx] = Some(tile.photo_id);
                let aspect = Vec2::new(tile.aspect.0, tile.aspect.1);
                demo.set_gallery_tile_image(idx, tile.bytes, aspect);
            }
            Err(e) => log::warn!("gallery tile {idx}: fetch failed: {e}"),
        }
    }
}

/// Starts a hires fetch in response to `Demo`'s
/// `take_pending_gallery_hires_fetch` injection point — reusing whichever
/// photo id `apply_gallery_fetch` already stashed for that tile, at
/// `width_px`×`height_px` (already the correctly aspect-preserving size —
/// see `GalleryHiresFetchRequest`'s doc, this function doesn't need to know
/// the photo's aspect ratio itself), scaled/capped to physical pixels (see
/// below) — drains whichever hires fetch is currently running, and drops
/// the receiver the instant `Demo` asks to cancel
/// (`take_pending_gallery_hires_cancel`). The network-touching half of the
/// enlarged view `Demo` itself can't do.
///
/// `request.width_px`/`height_px` are logical, uncapped (see
/// `GalleryHiresFetchRequest`'s doc) — scaled by `scale_factor` and capped
/// proportionally (the *larger* axis against the cap, both scaled by the
/// same factor, never clamped independently — see `Demo::
/// start_gallery_to_image`'s doc for why that one extra degree of freedom
/// would drift the fetched aspect away from the box's) at
/// `GALLERY_LARGE_IMAGE_MAX_SIDE_PX`.
fn apply_gallery_hires_fetch(
    demo: &mut Demo,
    gallery_hires_rx: &mut Option<GalleryHiresRx>,
    tile_photo_id: &[Option<u32>; GALLERY_TILE_COUNT],
    scale_factor: f32,
) {
    const GALLERY_LARGE_IMAGE_MAX_SIDE_PX: f32 = 900.0;
    if let Some(request) = demo.take_pending_gallery_hires_fetch() {
        match tile_photo_id[request.idx] {
            Some(photo_id) => {
                let physical_w = request.width_px as f32 * scale_factor;
                let physical_h = request.height_px as f32 * scale_factor;
                let cap_scale =
                    (GALLERY_LARGE_IMAGE_MAX_SIDE_PX / physical_w.max(physical_h)).min(1.0);
                let width = (physical_w * cap_scale).round().max(1.0) as u32;
                let height = (physical_h * cap_scale).round().max(1.0) as u32;
                let rx = gallery_fetch::spawn_hires(width, height, photo_id);
                *gallery_hires_rx = Some((request.idx, rx));
            }
            None => log::warn!(
                "gallery hires fetch: tile {} has no known photo id yet",
                request.idx
            ),
        }
    }
    if demo.take_pending_gallery_hires_cancel() {
        *gallery_hires_rx = None;
    }
    let Some((idx, rx)) = gallery_hires_rx else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(bytes)) => {
            demo.set_gallery_hires_image(*idx, bytes);
            *gallery_hires_rx = None;
        }
        Ok(Err(e)) => {
            log::warn!("gallery hires fetch: tile {idx}: {e}");
            *gallery_hires_rx = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            *gallery_hires_rx = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Render state
// ---------------------------------------------------------------------------

struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    demo: Demo,
    font_atlas: FontAtlas,
    last_frame: std::time::Instant,
    playing_video: Option<PlayingVideo>,
    present_timing: PresentTiming,
    gallery_fetch_rx: Option<Receiver<gallery_fetch::FetchResult>>,
    /// Which picsum photo id each tile's current low-res fetch landed on —
    /// `apply_gallery_hires_fetch` reuses this so the hires upgrade is the
    /// *same* photo, bigger, not a different random one.
    tile_photo_id: [Option<u32>; GALLERY_TILE_COUNT],
    gallery_hires_rx: Option<GalleryHiresRx>,
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
        // Explicitly avoid an sRGB-tagged surface format: every color in
        // this app (QuadState::color, Text::color, etc.) is authored as a
        // flat, already-gamma-space value. The fragment shader passes these
        // through with no linear/gamma conversion of its own, so an sRGB
        // swapchain format would make the GPU apply an unwanted *second*
        // gamma encode on top of values that are already gamma-encoded —
        // washing out colors and lightening blacks.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
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

        let atlas_config = AtlasConfig::default();
        validate_atlas_config(&device, &atlas_config)
            .expect("AtlasConfig must fit this device's real reported limits");
        let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096, atlas_config);

        // Window is created at a *logical* size but inner_size() returns
        // *physical* pixels on HiDPI displays — divide by scale_factor so
        // world units stay 1:1 with logical pixels.
        let scale_factor = window.scale_factor() as f32;
        pipeline.set_view_projection(
            &queue,
            QuadPipeline::ortho(
                size.width as f32 / scale_factor,
                size.height as f32 / scale_factor,
            ),
        );

        let mut demo = Demo::new();
        demo.app_mut().world_mut().insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        demo.app_mut().world_mut().insert_resource(pipeline);

        let logo_frames = bake_logo_frames(demo.app_mut(), &queue, logo_frame_path, "logo");
        demo.set_logo_frames(logo_frames);
        let loading_logo_frames_dark =
            bake_logo_frames(demo.app_mut(), &queue, logo_frame_dark_path, "logo dark");
        demo.set_loading_logo_frames_dark(loading_logo_frames_dark);

        match std::fs::read(BG_IMAGE_PATH) {
            Ok(bytes) => demo.set_background_image(bytes),
            Err(e) => log::warn!("background: could not read {BG_IMAGE_PATH:?}: {e}"),
        }
        match std::fs::read(BG_IMAGE_DARK_PATH) {
            Ok(bytes) => demo.set_background_image_dark(bytes),
            Err(e) => log::warn!("background: could not read {BG_IMAGE_DARK_PATH:?}: {e}"),
        }
        for (idx, path) in TILE_IMAGE_PATHS.iter().enumerate() {
            match std::fs::read(path) {
                Ok(bytes) => demo.set_tile_image(idx, bytes),
                Err(e) => log::warn!("tile {idx}: could not read box-cover image {path:?}: {e}"),
            }
        }
        // Nav icons/lockup + the theme sun/moon toggle — light/dark pairs.
        // Same read-or-warn-and-skip graceful degradation as every asset
        // above; a path that fails just leaves that one quad blank/
        // transparent.
        macro_rules! set_asset {
            ($path:expr, $setter:ident) => {
                match std::fs::read($path) {
                    Ok(bytes) => demo.$setter(bytes),
                    Err(e) => log::warn!(
                        concat!(stringify!($setter), ": could not read {:?}: {}"),
                        $path,
                        e
                    ),
                }
            };
        }
        set_asset!(NAV_HOME_ICON_PATH, set_nav_home_icon);
        set_asset!(NAV_HOME_ICON_DARK_PATH, set_nav_home_icon_dark);
        set_asset!(NAV_HOME_ICON_SELECTED_PATH, set_nav_home_icon_selected);
        set_asset!(
            NAV_HOME_ICON_SELECTED_DARK_PATH,
            set_nav_home_icon_selected_dark
        );
        set_asset!(NAV_BACK_ICON_PATH, set_nav_back_icon);
        set_asset!(NAV_BACK_ICON_DARK_PATH, set_nav_back_icon_dark);
        set_asset!(NAV_LOGO_LOCKUP_PATH, set_nav_logo_lockup);
        set_asset!(NAV_LOGO_LOCKUP_DARK_PATH, set_nav_logo_lockup_dark);
        set_asset!(THEME_SUN_ICON_PATH, set_theme_sun_icon);
        set_asset!(THEME_SUN_ICON_DARK_PATH, set_theme_sun_icon_dark);
        set_asset!(THEME_MOON_ICON_PATH, set_theme_moon_icon);
        set_asset!(THEME_MOON_ICON_DARK_PATH, set_theme_moon_icon_dark);
        demo.set_viewport_size(Vec2::new(
            size.width as f32 / scale_factor,
            size.height as f32 / scale_factor,
        ));

        log::info!(
            "Render state ready — {}×{} px, format {:?}",
            size.width,
            size.height,
            surface_format,
        );

        Self {
            window,
            surface,
            device,
            queue,
            surface_config,
            demo,
            font_atlas: FontAtlas::with_embedded_font(),
            last_frame: std::time::Instant::now(),
            playing_video: None,
            present_timing: PresentTiming::default(),
            gallery_fetch_rx: None,
            tile_photo_id: [None; GALLERY_TILE_COUNT],
            gallery_hires_rx: None,
        }
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);

        let scale_factor = self.window.scale_factor() as f32;
        let logical_size = Vec2::new(
            size.width as f32 / scale_factor,
            size.height as f32 / scale_factor,
        );
        let pipeline = self.demo.app_mut().world_mut().resource::<QuadPipeline>();
        pipeline.set_view_projection(
            &self.queue,
            QuadPipeline::ortho(logical_size.x, logical_size.y),
        );
        self.demo.set_viewport_size(logical_size);
    }

    fn render(&mut self) {
        // Clamped to 20fps-equivalent. Without this, any real stall between
        // frames (most visibly: the first frame ever, after asset baking
        // plus whatever GPU pipeline/shader warm-up the very first draw
        // triggers) gets fed straight into `Demo::tick` as one giant `dt`,
        // which is more than enough to blow through Splash's entire
        // delay+fade+hold budget (~3.1s) in a single tick — the intro/logo
        // animation never gets a chance to render a single visible frame,
        // and the app appears to open straight into the Splash→Home
        // transition already in flight.
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = std::time::Instant::now();
        self.demo.tick(dt); // Demo::tick already calls refresh_cascades internally.

        bake_pending_text(
            self.demo.app_mut().world_mut(),
            &mut self.font_atlas,
            &self.queue,
        );
        bake_gallery_hires_image(&mut self.demo, &self.queue);
        bake_pending_images(self.demo.app_mut().world_mut(), &self.queue);
        apply_texture_churn(&mut self.demo, &self.queue);
        apply_video_actions(
            &mut self.demo,
            &self.device,
            &self.queue,
            &mut self.playing_video,
            &mut self.present_timing,
        );
        // `Demo`'s own gallery-fetch requests are logical-pixel/uncapped —
        // see `GalleryFetchRequest`/`GalleryHiresFetchRequest`'s docs for
        // why scaling by the window's own `scale_factor` (for HiDPI
        // sharpness) and capping the physical result is this shell's job,
        // not `Demo`'s.
        let scale_factor = self.window.scale_factor() as f32;
        apply_gallery_fetch(
            &mut self.demo,
            &mut self.gallery_fetch_rx,
            &mut self.tile_photo_id,
            scale_factor,
        );
        apply_gallery_hires_fetch(
            &mut self.demo,
            &mut self.gallery_hires_rx,
            &self.tile_photo_id,
            scale_factor,
        );

        let instances = proteus_ui::collect_instances(self.demo.app_mut().world_mut());

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
                return;
            }
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

        let mut pipeline = self
            .demo
            .app_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>();
        // Drains the decode thread's channel and uploads the latest queued
        // frame — a no-op when nothing's playing (`consume_video_frame`'s
        // own doc: it bails immediately once the video texture is back to
        // its 1×1 suspended placeholder). The return value latches once a
        // *real* decoded frame actually lands — decode/buffering can take a
        // second or more, so `Demo::advance_video_loading`'s loading dots
        // need this signal to know when to stop waiting (`Demo` has no way
        // to see the GPU texture itself).
        let frame_landed = pipeline.consume_video_frame(&self.queue);
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
        // `pipeline`'s borrow of `self.demo` ends with the render-pass block
        // above — only now can `self.demo` be borrowed mutably again.
        if frame_landed {
            self.demo.set_video_first_frame_shown();
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        if self.playing_video.is_some() {
            self.present_timing.record();
        }
    }
}
