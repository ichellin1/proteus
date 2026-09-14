//! `proteus-shell-web` — WebGL2/WebGPU WASM shell (reference demo).
//!
//! ## The full collapse M13.2's own module doc predicted
//!
//! Canvas/wgpu setup, DPI, the `requestAnimationFrame` loop, `ResizeObserver`,
//! Pointer Events, visibility-pause, and context-loss logging all moved to
//! `proteus-host-web` at M13.2. Texture churn, the photo gallery, and video
//! all moved to `DemoApp` itself at M13.4 (steps 1, 3, and 4b — the last of
//! these needing `proteus-host-web`'s own `hls_video` module, real
//! `<video>`/`MediaSource` playback with no JS at all). With all three gone,
//! there's no shim left needing a wasm-bindgen handle back to JS — [`start`]
//! just fetches assets and hands off to `proteus_host_web::run`, matching
//! `proteus-shell-native`'s own equivalent collapse at M13.4 step 4a.

use wasm_bindgen::prelude::*;

use proteus_demo::DemoApp;
use proteus_host_web::{run, PreloadedHostServices};
use proteus_runtime::config::{RenderConfig, ResourceConfig};
use proteus_runtime::ProteusConfig;

/// The resting page colour, shown briefly before the background image loads
/// and behind any component transparency. Matches every other shell's
/// identical constant.
const CLEAR_COLOR: [f64; 4] = [
    0xCD as f64 / 255.0,
    0xC7 as f64 / 255.0,
    0xED as f64 / 255.0,
    1.0,
];

/// The generic renderer bakes every `Image` at this cap — matches
/// `proteus-shell-native::IMAGE_MAX_SIDE`.
const IMAGE_MAX_SIDE: u32 = 400;

/// Every key [`DemoApp::setup`] asks a [`proteus_runtime::HostServices`] for
/// (see `proteus-demo/src/app.rs::load_assets`) — fetched up front by
/// [`PreloadedHostServices::fetch`] before `run()`'s `Engine::new` (and so
/// `DemoApp::setup`) runs. Kept in lockstep with that function by hand; a
/// key missing here just means that asset silently doesn't load (same
/// graceful degradation `HostServices::load_asset` already has for a 404).
fn asset_keys() -> Vec<String> {
    let mut keys: Vec<String> = [
        "bg/ocean-blur.jpg",
        "bg/ocean-blur-dark.jpg",
        "tiger.jpg",
        "sintel.jpg",
        "jellyfish.jpg",
        "icons/home-idle.png",
        "icons/home-idle-dark.png",
        "icons/home-selected.png",
        "icons/home-selected-dark.png",
        "icons/back-idle.png",
        "icons/back-idle-dark.png",
        "logo/lockup.png",
        "logo/lockup-dark.png",
        "icons/sun-idle-dark.png",
        "icons/sun-selected.png",
        "icons/moon-idle.png",
        "icons/moon-selected-dark.png",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for n in 1..=19 {
        keys.push(format!("logo/frame-{n:02}.png"));
        keys.push(format!("logo/frame-{n:02}-dark.png"));
    }
    keys
}

/// `"{dir}|{codecs}"` per tile, left/center/right — see
/// `proteus_host_web::PreloadedHostServices::open_video`'s own doc for why
/// video keys carry this extra, web-only piece of information. Matches
/// `proteus-shell-native::TILE_VIDEO_PATHS`' index order
/// (`screens::video_tiles`).
fn video_keys() -> [String; 3] {
    [
        "videos/hls/tiger|avc1.64001F,mp4a.40.2".to_string(),
        "videos/hls/sintel|avc1.64001F".to_string(),
        "videos/hls/jellyfish|avc1.64001F".to_string(),
    ]
}

/// Mount the reference demo on the `<canvas>` element with the given `id`.
/// Fetches every asset [`DemoApp`] needs (relative to `images/`), then hands
/// off to `proteus_host_web::run` — nothing further to do here once that
/// returns; the browser's own event loop drives every frame after this.
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let keys = asset_keys();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let services = PreloadedHostServices::fetch("images", &key_refs).await;

    let config = ProteusConfig {
        render: RenderConfig {
            clear_color: CLEAR_COLOR,
            ..Default::default()
        },
        resources: ResourceConfig {
            image_max_side: Some(IMAGE_MAX_SIDE),
            ..Default::default()
        },
        ..ProteusConfig::web()
    };

    run(
        DemoApp::new(Some(video_keys())),
        &canvas_id,
        config,
        services,
    )
    .await?;
    Ok(())
}
