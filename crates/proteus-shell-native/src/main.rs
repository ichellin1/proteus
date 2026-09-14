//! `proteus-shell-native` — native desktop entry point.
//!
//! ## M13.4 (step 4a) — the collapse the M13.1 module doc predicted
//!
//! Video is now a real `HostServices` seam — [`proteus_host_winit`]'s
//! `DirHostServices::open_video`, backed by `ffmpeg`/`ffprobe` (see that
//! crate's own `mp4_player` module, moved there from this crate) — instead
//! of a shell-side shim. That was the one thing still keeping this file
//! more than a one-line `main()`: texture churn and the photo gallery
//! already collapsed the same way at M13.4 steps 1 and 3. What's left is
//! exactly `proteus_host_winit::run(DemoApp::new(...), RunConfig { .. })`.
//!
//! The web shell hasn't made this same video move yet (M13.4 step 4b — HLS
//! decode needs `<video>`/`MediaSource`, tracked as its own unit of work
//! given its size) and so keeps its own shell-side video shim for now.

use std::path::PathBuf;

use proteus_demo::DemoApp;
use proteus_host_winit::RunConfig;
use proteus_runtime::config::{RenderConfig, ResourceConfig};
use proteus_runtime::ProteusConfig;

/// Base directory for `DemoApp`'s image asset keys (`bg/…`, `icons/…`,
/// `logo/…`, `tiger.jpg`, …) — resolved by `DirHostServices::load_asset`.
const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/images");

/// `assets/videos/{tiger,sintel_fixed,jellyfish_fixed}.mp4` — index order
/// matches `screens::video_tiles`' left/center/right tiles. Unlike
/// `ASSET_DIR`, these are handed to `DemoApp` as literal filesystem paths —
/// video keys don't resolve against `DirHostServices`' own `base` (see
/// `DirHostServices::open_video`'s own doc for why).
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

fn main() {
    env_logger::init();
    log::info!("Proteus reference demo — native shell");

    let video_keys = TILE_VIDEO_PATHS.map(|p| p.to_string());
    let demo_app = DemoApp::new(Some(video_keys));

    let config = ProteusConfig {
        render: RenderConfig {
            clear_color: CLEAR_COLOR,
            ..Default::default()
        },
        resources: ResourceConfig {
            image_max_side: Some(IMAGE_MAX_SIDE),
            ..Default::default()
        },
        ..ProteusConfig::default()
    };

    proteus_host_winit::run(
        demo_app,
        RunConfig {
            title: "Proteus — Reference Demo".to_string(),
            initial_size: (1280, 800),
            asset_dir: PathBuf::from(ASSET_DIR),
            proteus: config,
        },
    );
}
