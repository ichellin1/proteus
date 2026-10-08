//! Video from your own player, the same app as the TypeScript video example.
//!
//! Proteus shows video but doesn't play it. A player decodes the video, and
//! the app uploads each new frame to a Proteus video, which a component
//! shows. The component grows when the pointer is over it, the way any
//! component can. Clicking it pauses and resumes the player: playback is the
//! player's, not Proteus's.
//!
//! Techniques worth copying:
//!   - The player is a trait of the app's own, [`Player`], with an
//!     implementation for each platform: `ffmpeg` on the desktop, in
//!     `ffmpeg.rs`, and the browser's `<video>` element on the web, in
//!     `browser.rs`. Each entry point passes its own in, and the app never
//!     needs to know which it has.
//!   - Upload in [`App::update`], which the host calls every frame: ask the
//!     player for a new frame, and upload it if there is one. Asking never
//!     waits for the player.
//!
//! Video is experimental in V1: one video at a time, with frames supplied by
//! the app.

use std::cell::Cell;
use std::rc::Rc;

use proteus_runtime::{App, Frame};
use proteus_sdk::glam::{Vec2, Vec4};
use proteus_sdk::{ComponentSpec, QuadState, StyleOverride, VideoHandle};

#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(not(target_arch = "wasm32"))]
pub mod ffmpeg;

/// One decoded frame, as RGBA pixels: `width * height * 4` bytes, row by row
/// from the top-left.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A video player: it decodes the video and hands over its frames.
pub trait Player {
    /// The newest frame to show, after `dt` more seconds of playback, or
    /// `None` if it hasn't changed. Never waits for the player.
    fn next_frame(&mut self, dt: f32) -> Option<Picture>;

    /// Pauses playback, or resumes it if it's paused.
    fn toggle_pause(&mut self);
}

const BACKGROUND: Vec4 = Vec4::new(0.08, 0.08, 0.1, 1.0);

/// The engine settings both entry points use.
pub fn config() -> proteus_runtime::ProteusConfig {
    let mut config = proteus_runtime::ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config
}

/// The app: one component showing the player's video.
pub struct VideoApp<P: Player> {
    player: P,
    video: Option<VideoHandle>,
    /// Set by the component's click callback, which can't reach the player.
    clicked: Rc<Cell<bool>>,
}

impl<P: Player> VideoApp<P> {
    pub fn new(player: P) -> Self {
        Self {
            player,
            video: None,
            clicked: Rc::default(),
        }
    }
}

impl<P: Player> App for VideoApp<P> {
    fn setup(&mut self, f: &mut Frame) {
        // A 16:9 screen in the middle of the window.
        let width = (f.viewport.logical_size.x * 0.7).min(960.0);
        let screen = f.proteus.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(width, width * 9.0 / 16.0),
                corner_radius: 16.0,
                ..Default::default()
            })
            .hover(StyleOverride {
                scale: Some(1.03),
                ..Default::default()
            }),
        );
        let video = f.proteus.create_video();
        let _ = screen.show_video(f.proteus, &video);
        self.video = Some(video);

        let clicked = Rc::clone(&self.clicked);
        screen.on_click(f.proteus, move |_| clicked.set(true));
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        if self.clicked.take() {
            self.player.toggle_pause();
        }
        let Some(video) = self.video else {
            return;
        };
        if let Some(picture) = self.player.next_frame(dt) {
            video.upload_frame(f.proteus, picture.width, picture.height, &picture.rgba);
        }
    }
}

/// Runs the video on the `<canvas>` element with the id `canvas_id`, playing
/// `url` in a `<video>` element.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String, url: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    let player = browser::BrowserPlayer::new(&url)?;
    // The app loads no assets through the host; the player loads the video.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(VideoApp::new(player), &canvas_id, config(), services).await
}
