//! `proteus-shell-web`: runs the reference demo on a web page, compiled to
//! WebAssembly.
//!
//! [`DemoApp`] holds the demo, its settings and its asset list, and
//! [`proteus_host_web::run`] owns the canvas, the frame loop and the GPU.
//! [`start`] downloads the demo's assets, then hands off to `run`. This crate
//! supplies what is per-platform: the asset URL, the canvas, and a video
//! player, the `video_player` module, which plays HLS in a `<video>` element.

use wasm_bindgen::prelude::*;

use proteus_demo::DemoApp;
use proteus_host_web::{run, PreloadedHostServices};
use proteus_runtime::ProteusConfig;

mod video_player;

/// The left, center and right video tiles' HLS streams: each manifest's
/// directory, relative to the page, and its codec string. In
/// `screens::video_tiles`' order.
fn tile_streams() -> [(String, String); 3] {
    [
        ("videos/hls/tiger", "avc1.64001F,mp4a.40.2"),
        ("videos/hls/sintel", "avc1.64001F"),
        ("videos/hls/jellyfish", "avc1.64001F"),
    ]
    .map(|(dir, codecs)| (dir.to_string(), codecs.to_string()))
}

/// Runs the reference demo on the `<canvas>` element with the given `id`.
///
/// Downloads every asset [`DemoApp`] needs from `images/`, since `setup` can't
/// wait for a download, then hands off to `proteus_host_web::run`. Returns
/// once the first frame is scheduled; the browser runs every frame after
/// that.
///
/// # Errors
///
/// Returns an error if the canvas isn't found or the GPU can't be set up.
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let keys = DemoApp::asset_keys();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let services = PreloadedHostServices::fetch("images", &key_refs).await;

    let player = video_player::HlsVideos {
        streams: tile_streams(),
    };
    run(
        DemoApp::new(Some(Box::new(player))),
        &canvas_id,
        DemoApp::config(ProteusConfig::web()),
        services,
    )
    .await?;
    Ok(())
}
