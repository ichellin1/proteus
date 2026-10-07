//! The native host: runs a Proteus [`App`] in a desktop
//! window, using winit and wgpu.
//!
//! [`run`] opens the window, sets up the GPU, and drives an [`Engine`] from
//! winit's event loop:
//!
//! ```no_run
//! # struct MyApp;
//! # impl proteus_runtime::App for MyApp { fn setup(&mut self, _: &mut proteus_runtime::Frame) {} }
//! # let my_app = MyApp;
//! proteus_host_winit::run(my_app, proteus_host_winit::RunConfig::default());
//! ```
//!
//! It handles the window, the frame loop, pointer input, resizing and changes of
//! display scale. Assets are read from a directory ([`DirHostServices`]).
//! Keyboard input is not handled yet, and the surface is not recreated after
//! the app is suspended, which only happens on mobile platforms.

#![warn(missing_docs)]

mod mp4_player;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use proteus_runtime::config::RenderConfig;
use proteus_runtime::glam::Vec2;
use proteus_runtime::wgpu;
use proteus_runtime::{
    App, Engine, FetchId, FetchResult, FetchTracker, GpuSurface, HostServices, ProteusConfig,
    SurfaceRequest, VideoStream, Viewport,
};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

// ---------------------------------------------------------------------------
// DirHostServices
// ---------------------------------------------------------------------------

/// [`HostServices`] that read assets from a directory.
///
/// `load_asset("nav/home-idle.png")` reads `base/nav/home-idle.png`. A missing
/// or unreadable file logs a warning and returns `None`.
///
/// [`fetch_async`](HostServices::fetch_async) also accepts an `http://` or
/// `https://` URL, fetched on a background thread, one thread per request. A
/// plain key is read immediately, since a local file read is fast, but its
/// result is still delivered through
/// [`poll_fetches`](HostServices::poll_fetches) like any other fetch.
///
/// A URL fetch that hasn't finished after [`FETCH_TIMEOUT`] fails, with a
/// warning, and its result is `None`, so a server that never answers doesn't
/// keep a thread forever.
pub struct DirHostServices {
    base: PathBuf,
    next_id: u64,
    fetch_tx: Sender<FetchResult>,
    fetch_rx: Receiver<FetchResult>,
    // A request already running can't be interrupted, so a cancelled one's
    // result is discarded when it arrives.
    fetches: FetchTracker,
}

/// How long [`DirHostServices`] waits for a URL fetch before it fails.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

impl DirHostServices {
    /// Reads assets from the directory `base`.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        let (fetch_tx, fetch_rx) = mpsc::channel();
        Self {
            base: base.into(),
            next_id: 0,
            fetch_tx,
            fetch_rx,
            fetches: FetchTracker::default(),
        }
    }
}

fn fetch_url_bytes(url: &str) -> Option<Arc<[u8]>> {
    use std::io::Read;
    let result = (|| -> Result<Vec<u8>, String> {
        let agent = ureq::AgentBuilder::new().timeout(FETCH_TIMEOUT).build();
        let resp = agent.get(url).call().map_err(|e| e.to_string())?;
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
        self.fetches.started(id);

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
            // A local key: read now, but delivered like any other fetch.
            let bytes = self.load_asset(key_or_url);
            let _ = self.fetch_tx.send((id, bytes));
        }
        id
    }

    fn poll_fetches(&mut self) -> Vec<FetchResult> {
        while let Ok((id, bytes)) = self.fetch_rx.try_recv() {
            self.fetches.finished(id, bytes);
        }
        self.fetches.take()
    }

    fn cancel_fetch(&mut self, id: FetchId) {
        self.fetches.cancel(id);
    }

    // other assets? Today they are file paths.
    // Opens an MP4 file for playback, decoded with `ffmpeg`. Unlike
    // [`Self::load_asset`], `key` is a file path, not a path under the asset
    // directory.
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        mp4_player::open(PathBuf::from(key)).map(|stream| Box::new(stream) as Box<dyn VideoStream>)
    }
}

// ---------------------------------------------------------------------------
// RunConfig
// ---------------------------------------------------------------------------

/// Window, asset and engine settings for [`run`].
pub struct RunConfig {
    /// Window title.
    pub title: String,
    /// The window's initial content size, in logical pixels.
    pub initial_size: (u32, u32),
    /// The directory [`DirHostServices`] reads assets from.
    pub asset_dir: PathBuf,
    /// Engine settings.
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

/// The GPU limits this host requests: `wgpu::Limits::default()`, the full
/// defaults, which larger settings such as `ProteusConfig::desktop()` need.
/// The shaders use nothing beyond WebGL2, so they run on both hosts. Pass
/// them to `ProteusConfig::check` to test a config for this host.
pub fn limits() -> wgpu::Limits {
    wgpu::Limits::default()
}

/// Opens a window and runs `app` until the window is closed.
///
/// # Panics
///
/// If `config.proteus` doesn't fit [`limits`], before the window opens, with
/// the `ConfigError`'s message; call `ProteusConfig::check` first to handle
/// it yourself. Also if the window or the GPU can't be set up.
pub fn run<A: App>(app: A, config: RunConfig) {
    if let Err(e) = config.proteus.check(&limits()) {
        panic!("{e}");
    }
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
    gpu: GpuSurface,
    engine: Engine,
    services: DirHostServices,
    last_frame: Instant,
}

impl Running {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.gpu.resize(width, height);
        self.engine
            .resize(viewport_for(&self.window, width, height));
    }

    fn render(&mut self, app: &mut dyn App) {
        // `Engine::frame` clamps this; the host only measures it.
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();

        let frame = match self.gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.gpu.reconfigure();
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
            // Already set up: `resumed` fires once on desktop, but may fire
            // again elsewhere. The surface isn't recreated after a real
            // suspend yet.
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
        let gpu = pollster::block_on(init_gpu(window.clone(), render_cfg));
        log::info!("GPU adapter: {}", gpu.adapter_description());

        let mut services = DirHostServices::new(self.config.asset_dir.clone());
        let viewport = viewport_for(&window, gpu.config.width, gpu.config.height);
        let engine = Engine::new(
            &gpu.device,
            &gpu.queue,
            gpu.format(),
            viewport,
            self.config.proteus.clone(),
            &mut self.app,
            &mut services,
        );

        let running = Running {
            window,
            gpu,
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
                // `position` is in physical pixels from the top-left; the
                // engine wants world units: logical, origin at the center, y
                // up.
                let scale = running.window.scale_factor() as f32;
                let w = running.gpu.config.width as f32 / scale;
                let h = running.gpu.config.height as f32 / scale;
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

/// The viewport for a window's scale factor and physical size.
fn viewport_for(window: &Window, physical_w: u32, physical_h: u32) -> Viewport {
    let scale = window.scale_factor() as f32;
    Viewport::new(
        Vec2::new(physical_w as f32 / scale, physical_h as f32 / scale),
        scale,
    )
}

/// Creates the window and sets up its GPU surface with `GpuSurface::create`.
async fn init_gpu(window: Arc<Window>, render: RenderConfig) -> GpuSurface {
    let size = window.inner_size();
    GpuSurface::create(
        window,
        SurfaceRequest {
            label: "proteus-host-winit",
            size: (size.width, size.height),
            power_preference: render.power_preference,
            present_mode: render.present_mode,
            limits: limits(),
        },
    )
    .await
    .expect("GPU setup failed")
}

#[cfg(test)]
mod limits_tests {
    use super::*;

    // The native host requests enough for every preset, `desktop()` included.
    #[test]
    fn every_preset_fits_the_native_host() {
        for config in [
            ProteusConfig::web(),
            ProteusConfig::desktop(),
            ProteusConfig::constrained(),
        ] {
            assert_eq!(config.check(&limits()), Ok(()));
        }
    }
}
