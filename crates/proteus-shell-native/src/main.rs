//! `proteus-shell-native` — native desktop entry point.
//!
//! ## M13.1 (step 5)
//!
//! The ~8700 lines of hand-rolled demo logic became `proteus_demo::Demo` at
//! M12.5; the per-frame render loop and the ~20 `Demo`-shaped asset setters
//! became `proteus_runtime` (`Engine` / `Renderer`) + `proteus_demo::DemoApp`
//! at M13.1. What is left here is a slim winit `ApplicationHandler` that:
//!
//! - creates the window + wgpu surface/device (M13.3 will fold this into a
//!   shared `proteus-gpu` helper used by the web host too),
//! - drives a [`proteus_runtime::Engine`] with [`DemoApp`],
//! - and runs the video / gallery-fetch / texture-churn **shims** the demo
//!   still needs — real `.mp4` decode via `ffmpeg` ([`mp4_player`]), the
//!   `picsum.photos` fetch ([`gallery_fetch`]), and GPU texture churn — none
//!   of which have a home in the `App` contract until M13.4 turns them into
//!   host services. Until then they poll `Demo::take_pending_*` each frame,
//!   exactly as before.
//!
//! A full collapse to `fn main() { proteus_host_winit::run(DemoApp::new()) }`
//! waits for M13.4.

mod gallery_fetch;
mod mp4_player;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use proteus_demo::DemoApp;
use proteus_host_winit::DirHostServices;
use proteus_render::{QuadPipeline, TextureId};
use proteus_runtime::{wgpu, Engine, ProteusConfig, Viewport};

// ---------------------------------------------------------------------------
// Asset locations
// ---------------------------------------------------------------------------

/// Base directory for [`DemoApp`]'s image asset keys (`bg/…`, `icons/…`,
/// `logo/…`, `tiger.jpg`, …) — resolved by [`DirHostServices`].
const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images");

/// `assets/videos/{tiger,sintel_fixed,jellyfish_fixed}.mp4` — index order
/// matches `screens::video_tiles`' left/center/right tiles.
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

/// 4×3 — matches `screens::gallery::TILE_COUNT`.
const GALLERY_TILE_COUNT: usize = 12;

/// The resting page colour, shown briefly before the background image loads
/// and behind any component transparency. A light lavender, not black.
const CLEAR_COLOR: [f64; 4] = [
    0xCD as f64 / 255.0,
    0xC7 as f64 / 255.0,
    0xED as f64 / 255.0,
    1.0,
];

/// The generic renderer bakes every `Image` at this cap (the M12 shells'
/// `MAX_IMAGE_SIDE`); the one hires gallery overlay overrides it per-entity.
const IMAGE_MAX_SIDE: u32 = 400;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    env_logger::init();
    log::info!("Proteus reference demo — native shell");

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = ShellApp::default();
    event_loop.run_app(&mut app).expect("event loop error");
}

#[derive(Default)]
struct ShellApp {
    state: Option<RenderState>,
}

impl ApplicationHandler for ShellApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_ref() {
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
        state.window.request_redraw();
        self.state = Some(state);
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
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = state.window.inner_size();
                state.resize(size.width, size.height);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = state.window.scale_factor() as f32;
                let w = state.surface_config.width as f32 / scale;
                let h = state.surface_config.height as f32 / scale;
                let wx = (position.x as f32 / scale) - w / 2.0;
                let wy = h / 2.0 - (position.y as f32 / scale);
                state.engine.pointer_moved(Some(Vec2::new(wx, wy)));
            }
            WindowEvent::CursorLeft { .. } => state.engine.pointer_moved(None),
            WindowEvent::MouseInput {
                state: btn_state,
                button: MouseButton::Left,
                ..
            } => match btn_state {
                ElementState::Pressed => state.engine.pointer_pressed(),
                ElementState::Released => state.engine.pointer_released(),
            },
            WindowEvent::RedrawRequested => state.render(),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Video playback
// ---------------------------------------------------------------------------

/// The currently-playing tile's decode thread + GPU video texture id.
struct PlayingVideo {
    texture_id: TextureId,
    handle: mp4_player::PlaybackHandle,
}

/// Diagnostic: wall-clock gap between successive `frame.present()` calls
/// while a video is playing.
#[derive(Default)]
struct PresentTiming {
    last_present: Option<Instant>,
    count: u32,
    sum: std::time::Duration,
    max: std::time::Duration,
}

impl PresentTiming {
    const LOG_INTERVAL: u32 = 90;

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

    fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// Render state
// ---------------------------------------------------------------------------

/// Which picsum photo id a hires fetch is for, plus its result channel.
type GalleryHiresRx = (usize, Receiver<Result<Vec<u8>, String>>);

struct RenderState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,

    engine: Engine,
    demo_app: DemoApp,
    services: DirHostServices,
    last_frame: Instant,

    playing_video: Option<PlayingVideo>,
    present_timing: PresentTiming,
    gallery_fetch_rx: Option<Receiver<gallery_fetch::FetchResult>>,
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
        // Non-sRGB on purpose — colours are authored gamma-space, the shader
        // passes them through untouched.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let scale = window.scale_factor() as f32;
        let viewport = Viewport::new(
            Vec2::new(
                surface_config.width as f32 / scale,
                surface_config.height as f32 / scale,
            ),
            scale,
        );
        let config = ProteusConfig {
            clear_color: CLEAR_COLOR,
            image_max_side: Some(IMAGE_MAX_SIDE),
            ..ProteusConfig::default()
        };

        let mut demo_app = DemoApp::new();
        let mut services = DirHostServices::new(PathBuf::from(ASSET_DIR));
        let engine = Engine::new(
            &device,
            &queue,
            surface_format,
            viewport,
            config,
            &mut demo_app,
            &mut services,
        );

        log::info!(
            "Render state ready — {}×{} px, format {surface_format:?}",
            size.width,
            size.height,
        );

        Self {
            window,
            surface,
            device,
            queue,
            surface_config,
            engine,
            demo_app,
            services,
            last_frame: Instant::now(),
            playing_video: None,
            present_timing: PresentTiming::default(),
            gallery_fetch_rx: None,
            tile_photo_id: [None; GALLERY_TILE_COUNT],
            gallery_hires_rx: None,
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);

        let scale = self.window.scale_factor() as f32;
        self.engine.resize(Viewport::new(
            Vec2::new(width as f32 / scale, height as f32 / scale),
            scale,
        ));
    }

    fn render(&mut self) {
        // Clamp to a 20fps floor — see the M12 shell's own note.
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = Instant::now();

        // ── Pre-render: upload the latest decoded video frame, if any. ──
        let scale = self.window.scale_factor() as f32;
        let frame_landed = self
            .engine
            .proteus_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>()
            .consume_video_frame(&self.queue);

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

        self.engine
            .frame(dt, &view, &mut self.demo_app, &mut self.services);

        if frame_landed {
            if let Some(demo) = self.demo_app.demo_mut() {
                demo.set_video_first_frame_shown();
            }
        }

        // ── Post-frame shims (M13.4 debt): drain Demo::take_pending_*. ──
        self.apply_video_actions();
        self.apply_texture_churn();
        self.apply_gallery_fetch(scale);
        self.apply_gallery_hires_fetch(scale);

        frame.present();
        if self.playing_video.is_some() {
            self.present_timing.record();
        }
    }

    // ── video / gallery / churn shims — adapted from the M12 native shell ──

    fn apply_texture_churn(&mut self) {
        let Some(demo) = self.demo_app.demo_mut() else {
            return;
        };
        let updates = demo.take_pending_texture_churn();
        if updates.is_empty() {
            return;
        }
        let proteus = self.engine.proteus_mut();
        for update in updates {
            let texture_id = {
                let mut pipeline = proteus.world_mut().resource_mut::<QuadPipeline>();
                let Some(id) =
                    pipeline
                        .texture_registry
                        .register_static(update.width, update.height, false)
                else {
                    log::warn!("texture churn: main_atlas full");
                    continue;
                };
                let placement = pipeline
                    .texture_registry
                    .main_atlas_region(id)
                    .expect("just registered");
                pipeline.write_to_main_atlas(&self.queue, placement, &update.rgba);
                id
            };
            update.handle.set_texture(
                proteus,
                proteus_sdk::TextureHandle::from_texture_id(texture_id),
            );
        }
    }

    fn apply_video_actions(&mut self) {
        let Some(demo) = self.demo_app.demo_mut() else {
            return;
        };
        let start = demo.take_pending_video_start();
        let stop = demo.take_pending_video_stop();

        if let Some(idx) = start {
            if let Some(old) = self.playing_video.take() {
                old.handle.stop();
                self.engine
                    .proteus_mut()
                    .world_mut()
                    .resource_mut::<QuadPipeline>()
                    .suspend_video(&self.device, old.texture_id);
                self.present_timing.reset();
            }
            let path = std::path::Path::new(TILE_VIDEO_PATHS[idx]);
            match mp4_player::probe(path) {
                Ok(dims) => {
                    let (texture_id, sender) = self
                        .engine
                        .proteus_mut()
                        .world_mut()
                        .resource_mut::<QuadPipeline>()
                        .init_video(&self.device, &self.queue, dims.width, dims.height);
                    let handle =
                        mp4_player::spawn(path.to_path_buf(), sender, dims.width, dims.height);
                    self.playing_video = Some(PlayingVideo { texture_id, handle });
                }
                Err(e) => log::warn!("video {idx}: could not probe {path:?}: {e}"),
            }
        }

        if stop {
            if let Some(old) = self.playing_video.take() {
                old.handle.stop();
                self.engine
                    .proteus_mut()
                    .world_mut()
                    .resource_mut::<QuadPipeline>()
                    .suspend_video(&self.device, old.texture_id);
                self.present_timing.reset();
            }
        }
    }

    fn apply_gallery_fetch(&mut self, scale_factor: f32) {
        const MAX_TILE_IMAGE_SIDE_PX: f32 = 400.0;
        if let Some(request) = self
            .demo_app
            .demo_mut()
            .and_then(|d| d.take_pending_gallery_fetch())
        {
            let side_px = (request.tile_side_px as f32 * scale_factor)
                .min(MAX_TILE_IMAGE_SIDE_PX)
                .round()
                .max(1.0) as u32;
            self.gallery_fetch_rx = Some(gallery_fetch::spawn(GALLERY_TILE_COUNT, side_px));
        }
        let Some(rx) = self.gallery_fetch_rx.as_ref() else {
            return;
        };
        let mut received = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            received.push(msg);
        }
        for (idx, result) in received {
            match result {
                Ok(tile) => {
                    self.tile_photo_id[idx] = Some(tile.photo_id);
                    let aspect = Vec2::new(tile.aspect.0, tile.aspect.1);
                    if let Some(demo) = self.demo_app.demo_mut() {
                        demo.set_gallery_tile_image(
                            self.engine.proteus_mut(),
                            idx,
                            tile.bytes,
                            aspect,
                        );
                    }
                }
                Err(e) => log::warn!("gallery tile {idx}: fetch failed: {e}"),
            }
        }
    }

    fn apply_gallery_hires_fetch(&mut self, scale_factor: f32) {
        const GALLERY_LARGE_IMAGE_MAX_SIDE_PX: f32 = 900.0;
        if let Some(request) = self
            .demo_app
            .demo_mut()
            .and_then(|d| d.take_pending_gallery_hires_fetch())
        {
            match self.tile_photo_id[request.idx] {
                Some(photo_id) => {
                    let physical_w = request.width_px as f32 * scale_factor;
                    let physical_h = request.height_px as f32 * scale_factor;
                    let cap_scale =
                        (GALLERY_LARGE_IMAGE_MAX_SIDE_PX / physical_w.max(physical_h)).min(1.0);
                    let width = (physical_w * cap_scale).round().max(1.0) as u32;
                    let height = (physical_h * cap_scale).round().max(1.0) as u32;
                    self.gallery_hires_rx = Some((
                        request.idx,
                        gallery_fetch::spawn_hires(width, height, photo_id),
                    ));
                }
                None => log::warn!("gallery hires: tile {} has no known photo id", request.idx),
            }
        }
        if self
            .demo_app
            .demo_mut()
            .map(|d| d.take_pending_gallery_hires_cancel())
            .unwrap_or(false)
        {
            self.gallery_hires_rx = None;
        }
        let Some((idx, rx)) = self.gallery_hires_rx.as_ref() else {
            return;
        };
        let idx = *idx;
        match rx.try_recv() {
            Ok(Ok(bytes)) => {
                if let Some(demo) = self.demo_app.demo_mut() {
                    demo.set_gallery_hires_image(self.engine.proteus_mut(), idx, bytes);
                }
                self.gallery_hires_rx = None;
            }
            Ok(Err(e)) => {
                log::warn!("gallery hires {idx}: {e}");
                self.gallery_hires_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.gallery_hires_rx = None,
        }
    }
}
