//! `proteus-shell-web`: runs the reference demo on a web page, compiled to
//! WebAssembly.
//!
//! All the work happens in other crates: [`DemoApp`] holds the demo, and
//! [`proteus_host_web::run`] owns the canvas, the frame loop and the GPU.
//! [`start`] downloads the demo's assets, then hands off to `run` with the
//! demo's settings, which match `proteus-shell-native`'s.

use wasm_bindgen::prelude::*;

use proteus_demo::DemoApp;
use proteus_host_web::{run, PreloadedHostServices};
use proteus_runtime::config::{RenderConfig, ResourceConfig};
use proteus_runtime::ProteusConfig;

/// The resting page color, shown briefly before the background image loads
/// and behind any component transparency. Matches `proteus-shell-native`.
const CLEAR_COLOR: [f64; 4] = [
    0xCD as f64 / 255.0,
    0xC7 as f64 / 255.0,
    0xED as f64 / 255.0,
    1.0,
];

/// The largest side, in pixels, the renderer bakes an `Image` at. Matches
/// `proteus-shell-native`.
const IMAGE_MAX_SIDE: u32 = 400;

/// Every asset key `DemoApp` loads in `setup` (see `load_assets` in
/// `proteus-demo`'s `app.rs`). They are downloaded with
/// [`PreloadedHostServices::fetch`] before `run` starts the app, because
/// `setup` can't wait for a download.
///
/// This list is kept in step with `load_assets` by hand. A key missing here
/// means that asset silently doesn't load, as if its download had failed.
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

/// The video key for the left, center and right tiles, in the form
/// `"{dir}|{codecs}"`. See `PreloadedHostServices::open_video` for why a web
/// video key names its codecs. The order matches `proteus-shell-native`'s
/// video paths.
fn video_keys() -> [String; 3] {
    [
        "videos/hls/tiger|avc1.64001F,mp4a.40.2".to_string(),
        "videos/hls/sintel|avc1.64001F".to_string(),
        "videos/hls/jellyfish|avc1.64001F".to_string(),
    ]
}

/// Runs the reference demo on the `<canvas>` element with the given `id`.
///
/// Downloads every asset [`DemoApp`] needs from `images/`, then hands off to
/// `proteus_host_web::run`. Returns once the first frame is scheduled; the
/// browser runs every frame after that.
///
/// # Errors
///
/// Returns an error if the canvas isn't found or the GPU can't be set up.
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
