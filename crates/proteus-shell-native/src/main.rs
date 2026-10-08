//! `proteus-shell-native`: runs the reference demo in a desktop window.
//!
//! [`DemoApp`] holds the demo and its settings, and
//! [`proteus_host_winit::run`] owns the window, the frame loop and the GPU.
//! This binary supplies what is per-platform: the asset directory, the window,
//! and a video player, [`video_player`], which runs `ffmpeg`. `ffmpeg` and
//! `ffprobe` must be on `PATH`.

use std::path::PathBuf;

use proteus_demo::DemoApp;
use proteus_host_winit::RunConfig;
use proteus_runtime::ProteusConfig;

mod video_player;

/// Base directory for `DemoApp`'s image asset keys (`bg/…`, `icons/…`,
/// `logo/…`, `tiger.jpg`, …), resolved by `DirHostServices::load_asset`.
const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images");

/// The left, center and right video tiles' files, in
/// `screens::video_tiles`' order.
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

fn main() {
    env_logger::init();
    log::info!("Proteus reference demo — native shell");

    let player = video_player::Mp4Videos {
        paths: TILE_VIDEO_PATHS.map(PathBuf::from),
    };
    proteus_host_winit::run(
        DemoApp::new(Some(Box::new(player))),
        RunConfig {
            title: "Proteus — Reference Demo".to_string(),
            initial_size: (1280, 800),
            asset_dir: PathBuf::from(ASSET_DIR),
            proteus: DemoApp::config(ProteusConfig::default()),
        },
    );
}
