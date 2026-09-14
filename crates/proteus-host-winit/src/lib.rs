//! `proteus-host-winit` — Layer 3: a winit + wgpu native host for
//! [`proteus_runtime`] (M13.1 minimal build / M13.3).
//!
//! Implements [`proteus_runtime::Host`] on a winit window and drives an
//! [`Engine`] from winit's event loop. A native app is then just:
//!
//! ```no_run
//! # struct MyApp;
//! # impl proteus_runtime::App for MyApp { fn setup(&mut self, _: &mut proteus_runtime::Frame) {} }
//! # let my_app = MyApp;
//! proteus_host_winit::run(my_app, proteus_host_winit::RunConfig::default());
//! ```
//!
//! ## M13.1 scope
//!
//! Window, surface, frame loop, pointer input, resize + `ScaleFactorChanged`.
//! `suspended` / `resumed` surface recreation (real only on winit's mobile
//! backends), the `proteus-gpu` GPU-init consolidation, and keyboard →
//! navigation plumbing are M13.3 proper. Asset loading is a synchronous
//! directory read ([`DirHostServices`]) — the async story is M13.2 / M13.4.

mod mp4_player;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use proteus_runtime::config::RenderConfig;
use proteus_runtime::glam::Vec2;
use proteus_runtime::wgpu;
use proteus_runtime::{
    App, Engine, FetchId, FetchResult, Host, HostServices, ProteusConfig, VideoStream, Viewport,
};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

// ---------------------------------------------------------------------------
// DirHostServices
// ---------------------------------------------------------------------------

/// Resolves asset keys as paths under a base directory, read synchronously.
///
/// `load_asset("nav/home-idle.png")` → `std::fs::read(base/nav/home-idle.png)`.
/// A missing / unreadable file logs a warning and returns `None` — the same
/// graceful degradation the M12 shells had (that quad just stays blank).
///
/// [`fetch_async`](HostServices::fetch_async) (M13.4) additionally accepts a
/// full `http(s)://` URL — fetched on its own background thread via `ureq`
/// (blocking/sync, exactly what a plain `std::thread` wants; this crate has
/// no async runtime otherwise), one thread per fetch for concurrency, same
/// shape the reference demo's own former `gallery_fetch.rs` used before this
/// became a real `HostServices` primitive. A plain key still resolves via
/// [`load_asset`] inline — local disk reads are fast enough that a
/// background thread would only add latency, not remove it — but the result
/// still arrives through the same [`poll_fetches`](HostServices::poll_fetches)
/// channel, so callers see one uniform async story regardless of which path
/// a given request took.
///
/// [`load_asset`]: HostServices::load_asset
pub struct DirHostServices {
    base: PathBuf,
    next_id: u64,
    fetch_tx: Sender<FetchResult>,
    fetch_rx: Receiver<FetchResult>,
    /// Ids [`cancel_fetch`](HostServices::cancel_fetch) has been told to
    /// drop — a blocking `ureq` call already running on its own thread can't
    /// be interrupted, so this just discards the result in
    /// [`poll_fetches`](HostServices::poll_fetches) instead of delivering it.
    cancelled: HashSet<FetchId>,
}

impl DirHostServices {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        let (fetch_tx, fetch_rx) = mpsc::channel();
        Self {
            base: base.into(),
            next_id: 0,
            fetch_tx,
            fetch_rx,
            cancelled: HashSet::new(),
        }
    }
}

fn fetch_url_bytes(url: &str) -> Option<Arc<[u8]>> {
    use std::io::Read;
    let result = (|| -> Result<Vec<u8>, String> {
        let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        resp.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => Some(Arc::from(bytes)),
        Err(e) => {
            log::warn!("fetch_async: {url}: {e}");
            None
        }
    }
}

impl HostServices for DirHostServices {
    fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>> {
        let path = self.base.join(key);
        match std::fs::read(&path) {
            Ok(bytes) => Some(Arc::from(bytes)),
            Err(e) => {
                log::warn!("load_asset: {key}: {}: {e}", path.display());
                None
            }
        }
    }

    fn fetch_async(&mut self, key_or_url: &str) -> FetchId {
        let id = FetchId(self.next_id);
        self.next_id += 1;

        if key_or_url.starts_with("http://") || key_or_url.starts_with("https://") {
            let url = key_or_url.to_string();
            let tx = self.fetch_tx.clone();
            std::thread::Builder::new()
                .name(format!("fetch-{}", id.0))
                .spawn(move || {
                    let _ = tx.send((id, fetch_url_bytes(&url)));
                })
                .expect("failed to spawn fetch thread");
        } else {
            // Local key: resolved inline (see this type's own doc for why),
            // but still delivered through the same channel as the URL case.
            let bytes = self.load_asset(key_or_url);
            let _ = self.fetch_tx.send((id, bytes));
        }
        id
    }

    fn poll_fetches(&mut self) -> Vec<FetchResult> {
        let mut results = Vec::new();
        while let Ok((id, bytes)) = self.fetch_rx.try_recv() {
            if self.cancelled.remove(&id) {
                continue;
            }
            results.push((id, bytes));
        }
        results
    }

    fn cancel_fetch(&mut self, id: FetchId) {
        self.cancelled.insert(id);
    }

    /// `key` is a literal filesystem path here, unlike [`Self::load_asset`]
    /// — video files don't live under `self.base` in the reference demo
    /// (`assets/videos/`, separate from the image `base`), and there's no
    /// established "video keyspace" convention yet to resolve a bare key
    /// against. `.mp4` decode via `ffmpeg`/`ffprobe` — see [`mp4_player`].
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        mp4_player::open(PathBuf::from(key)).map(|stream| Box::new(stream) as Box<dyn VideoStream>)
    }
}

// ---------------------------------------------------------------------------
// RunConfig
// ---------------------------------------------------------------------------

/// Window + asset configuration for [`run`].
pub struct RunConfig {
    /// Window title.
    pub title: String,
    /// Initial inner size in logical pixels.
    pub initial_size: (u32, u32),
    /// Base directory for [`DirHostServices`] asset resolution.
    pub asset_dir: PathBuf,
    /// Renderer / atlas configuration (clear colour, atlas sizing).
    pub proteus: ProteusConfig,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            title: "Proteus".to_string(),
            initial_size: (1280, 800),
            asset_dir: PathBuf::from("."),
            proteus: ProteusConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

/// Open a window and run `app` until it closes. Panics on GPU or window
/// setup failure — the same "fatal, print and abort" posture the M12 native
/// shell had.
pub fn run<A: App>(app: A, config: RunConfig) {
    let event_loop = EventLoop::new().expect("failed to create winit event loop");
    let mut host = WinitHostApp {
        app,
        config,
        running: None,
    };
    event_loop
        .run_app(&mut host)
        .expect("winit event loop error");
}

// ---------------------------------------------------------------------------
// WinitHostApp — the ApplicationHandler
// ---------------------------------------------------------------------------

struct WinitHostApp<A: App> {
    app: A,
    config: RunConfig,
    running: Option<Running>,
}

/// Everything that exists only once a surface is live.
struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    engine: Engine,
    services: DirHostServices,
    last_frame: Instant,
}

impl Host for Running {
    fn device(&self) -> &wgpu::Device {
        &self.device
    }
    fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }
    fn viewport(&self) -> Viewport {
        viewport_for(
            &self.window,
            self.surface_config.width,
            self.surface_config.height,
        )
    }
}

impl Running {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.engine
            .resize(viewport_for(&self.window, width, height));
    }

    fn render(&mut self, app: &mut dyn App) {
        // M13.5: the dt clamp itself now lives in `Engine::frame`
        // (`ProteusConfig.frame.dt_clamp_secs`) — this host just measures.
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();

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
                log::error!("surface error: {e:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        self.engine.frame(dt, &view, app, &mut self.services);
        frame.present();
    }
}

impl<A: App> ApplicationHandler for WinitHostApp<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(running) = self.running.as_ref() {
            // Already initialised (desktop: resumed fires once; other
            // backends may re-fire) — just keep drawing. Surface recreation
            // on a genuine suspend/resume is M13.3 proper.
            running.window.request_redraw();
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title(self.config.title.clone())
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            self.config.initial_size.0,
                            self.config.initial_size.1,
                        )),
                )
                .expect("failed to create window"),
        );

        let render_cfg = self.config.proteus.render;
        let (surface, device, queue, surface_config) =
            pollster::block_on(init_gpu(window.clone(), render_cfg));

        let mut services = DirHostServices::new(self.config.asset_dir.clone());
        let viewport = viewport_for(&window, surface_config.width, surface_config.height);
        let engine = Engine::new(
            &device,
            &queue,
            surface_config.format,
            viewport,
            self.config.proteus.clone(),
            &mut self.app,
            &mut services,
        );

        let running = Running {
            window,
            surface,
            surface_config,
            device,
            queue,
            engine,
            services,
            last_frame: Instant::now(),
        };
        running.window.request_redraw();
        self.running = Some(running);
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(running) = self.running.as_ref() {
            running.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => running.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = running.window.inner_size();
                running.resize(size.width, size.height);
            }
            WindowEvent::CursorMoved { position, .. } => {
                // `position` is physical px; the engine wants world-space,
                // logical, centre-origin, Y-up.
                let scale = running.window.scale_factor() as f32;
                let w = running.surface_config.width as f32 / scale;
                let h = running.surface_config.height as f32 / scale;
                let wx = (position.x as f32 / scale) - w / 2.0;
                let wy = h / 2.0 - (position.y as f32 / scale);
                running.engine.pointer_moved(Some(Vec2::new(wx, wy)));
            }
            WindowEvent::CursorLeft { .. } => running.engine.pointer_moved(None),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => running.engine.pointer_pressed(),
                ElementState::Released => running.engine.pointer_released(),
            },
            WindowEvent::RedrawRequested => running.render(&mut self.app),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Logical viewport from a window's scale factor and a physical surface size.
fn viewport_for(window: &Window, physical_w: u32, physical_h: u32) -> Viewport {
    let scale = window.scale_factor() as f32;
    Viewport::new(
        Vec2::new(physical_w as f32 / scale, physical_h as f32 / scale),
        scale,
    )
}

/// wgpu instance / surface / adapter / device setup. Lifted from the M12
/// native shell; `render`'s `power_preference` / `present_mode` are M13.5's
/// wiring (were hardcoded before). The `proteus-gpu` consolidation (shared
/// with the web host) is M13.3 proper.
async fn init_gpu(
    window: Arc<Window>,
    render: RenderConfig,
) -> (
    wgpu::Surface<'static>,
    wgpu::Device,
    wgpu::Queue,
    wgpu::SurfaceConfiguration,
) {
    let size = window.inner_size();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let surface = instance
        .create_surface(window)
        .expect("failed to create surface");

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: render.power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .expect("no suitable GPU adapter found");

    log::info!("GPU adapter: {}", adapter.get_info().name);

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("proteus-host-winit"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            ..Default::default()
        })
        .await
        .expect("failed to create GPU device");

    let surface_caps = surface.get_capabilities(&adapter);
    // Avoid an sRGB-tagged surface format on purpose — every colour in
    // Proteus is authored as a flat, already-gamma-space value and the
    // fragment shader passes it through untouched, so an sRGB swapchain
    // would double-encode. Lifted from the M12 shells.
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
        present_mode: render.present_mode,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &surface_config);

    (surface, device, queue, surface_config)
}
