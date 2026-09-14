//! `proteus-shell-web` — WebGL2/WebGPU WASM shell (reference demo).
//!
//! ## M13.2 collapse
//!
//! Everything platform-generic (canvas/wgpu setup, DPI, the `requestAnimationFrame`
//! loop, `ResizeObserver`, Pointer Events, visibility-pause, context-loss
//! logging) moved to `proteus-host-web` at M13.2. What's left here mirrors
//! `proteus-shell-native`'s own M13.1 shape exactly: a thin wasm entry point
//! ([`start`]) that fetches assets and calls `proteus_host_web::run`, plus
//! the same **M13.4-debt shim** the native shell carries — real HLS video
//! playback, the `picsum.photos` gallery fetch, and GPU texture churn, none
//! of which have a home in the `App`/`HostServices` contract until M13.4
//! turns them into host services. Unlike native, this shim can't just poll
//! `Demo::take_pending_*` from a Rust-owned per-frame hook (the network/decode
//! work — `fetch`, `<video>`/`MediaSource` — is JS's job, not Rust's); it's
//! exposed instead as [`WebDemoHandle`], a small wasm-bindgen struct
//! `www/index.html`'s own `requestAnimationFrame` loop polls once per frame,
//! same "polled take" shape `Demo`'s own API already uses. `proteus-host-web`
//! grew `WebLoop::driver_mut` / `RustDriver::app_mut`/`engine_mut`
//! specifically to make this possible without changing the generic `App`
//! contract — see that crate's `rust_app.rs` doc.
//!
//! A full collapse (no shim at all) waits for M13.4.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use proteus_demo::DemoApp;
use proteus_host_web::{run, PreloadedHostServices, RustDriver, WebLoop};
use proteus_render::{QuadPipeline, TextureId};
use proteus_runtime::config::{RenderConfig, ResourceConfig};
use proteus_runtime::glam::Vec2;
use proteus_runtime::ProteusConfig;

/// 4×3 — matches `proteus_demo::screens::gallery::TILE_COUNT`.
const GALLERY_TILE_COUNT: usize = 12;

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

/// Mount the reference demo on the `<canvas>` element with the given `id`.
/// Fetches every asset [`DemoApp`] needs (relative to `images/`), then hands
/// off to `proteus_host_web::run`. Returns a [`WebDemoHandle`] — see the
/// crate doc — for `www/index.html`'s own rAF loop to poll for video/
/// gallery/texture-churn.
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<WebDemoHandle, JsValue> {
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

    let web_loop = run(DemoApp::new(), &canvas_id, config, services).await?;
    Ok(WebDemoHandle {
        web_loop,
        playing_video: None,
        tile_photo_id: [None; GALLERY_TILE_COUNT],
    })
}

// ---------------------------------------------------------------------------
// M13.4-debt shim — video / gallery / texture churn
// ---------------------------------------------------------------------------

/// The currently-playing tile's GPU video texture id (needed by
/// `QuadPipeline::suspend_video` on teardown).
struct PlayingVideo {
    texture_id: TextureId,
}

/// A pending hires fetch for the enlarged gallery image — returned by
/// [`WebDemoHandle::take_pending_gallery_hires_fetch`]. `width`/`height` are
/// the actual on-screen fitted pixel dimensions `Demo` computed, already
/// proportionally capped; `photo_id` requests the same picsum.photos photo
/// the tile's low-res image already shows — `Demo` itself never tracks a
/// photo id, so this shim stitches it back in from `tile_photo_id`.
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct GalleryHiresFetchRequest {
    pub tile_idx: u32,
    pub width: u32,
    pub height: u32,
    pub photo_id: u32,
}

/// See the crate doc. Wraps the [`WebLoop`] handle [`start`]'s `run()` call
/// returned, plus the extra bits of state (playing video's texture id, which
/// picsum photo each gallery tile currently shows) the M12 shells also kept
/// on their own equivalent struct — `Demo` itself is display-agnostic and
/// never tracks either.
#[wasm_bindgen]
pub struct WebDemoHandle {
    web_loop: Rc<RefCell<WebLoop<RustDriver<DemoApp, PreloadedHostServices>>>>,
    playing_video: Option<PlayingVideo>,
    tile_photo_id: [Option<u32>; GALLERY_TILE_COUNT],
}

#[wasm_bindgen]
impl WebDemoHandle {
    // ── Video (real HLS via <video>/MediaSource — see www/index.html) ──────

    /// Returns the tile index playback should start for, once, or
    /// `undefined` if nothing changed since the last call.
    #[wasm_bindgen(js_name = takePendingVideoStart)]
    pub fn take_pending_video_start(&mut self) -> Option<u32> {
        let mut state = self.web_loop.borrow_mut();
        let demo = state.driver_mut().app_mut().demo_mut()?;
        demo.take_pending_video_start().map(|i| i as u32)
    }

    /// Returns `true` once, the first tick after the screen was clicked to
    /// stop playback. Also releases the GPU video texture.
    #[wasm_bindgen(js_name = takePendingVideoStop)]
    pub fn take_pending_video_stop(&mut self) -> bool {
        let mut state = self.web_loop.borrow_mut();
        let stopped = state
            .driver_mut()
            .app_mut()
            .demo_mut()
            .map(|d| d.take_pending_video_stop())
            .unwrap_or(false);
        if stopped {
            if let Some(playing) = self.playing_video.take() {
                let device = state.device().clone();
                state
                    .driver_mut()
                    .engine_mut()
                    .proteus_mut()
                    .world_mut()
                    .resource_mut::<QuadPipeline>()
                    .suspend_video(&device, playing.texture_id);
            }
        }
        stopped
    }

    /// Returns `true` once, the first tick after a video load timed out —
    /// tells JS to abort whatever HLS segment fetch is still in flight.
    #[wasm_bindgen(js_name = takePendingVideoCancel)]
    pub fn take_pending_video_cancel(&mut self) -> bool {
        let mut state = self.web_loop.borrow_mut();
        state
            .driver_mut()
            .app_mut()
            .demo_mut()
            .map(|d| d.take_pending_video_cancel())
            .unwrap_or(false)
    }

    /// Sizes the pipeline's video texture. Call once `<video>`'s
    /// `loadedmetadata` has fired, passing its `videoWidth`/`videoHeight`.
    /// Rejects `0×0` outright — see the M12 shell's identical doc for why.
    #[wasm_bindgen(js_name = startVideo)]
    pub fn start_video(&mut self, tile_idx: u32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            log::warn!("start_video: rejecting 0×0 dimensions for tile {tile_idx}");
            return;
        }
        let mut state = self.web_loop.borrow_mut();
        let device = state.device().clone();
        let queue = state.queue().clone();
        let (texture_id, _sender) = state
            .driver_mut()
            .engine_mut()
            .proteus_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>()
            .init_video(&device, &queue, width, height);
        // `_sender` (the BYOV channel's sending half) goes unused — see
        // `push_video_frame`'s doc.
        self.playing_video = Some(PlayingVideo { texture_id });
    }

    /// Uploads one decoded RGBA frame straight to the video texture — no
    /// channel (unlike native's background-thread decode, JS already runs on
    /// its own turn of the event loop, so there's no second thread to
    /// hand off to or deadlock with) — and latches `Demo::
    /// set_video_first_frame_shown`. Call once per `<video>`
    /// `requestVideoFrameCallback`.
    #[wasm_bindgen(js_name = pushVideoFrame)]
    pub fn push_video_frame(&mut self, rgba: &[u8]) {
        if self.playing_video.is_none() {
            return;
        }
        let mut state = self.web_loop.borrow_mut();
        let queue = state.queue().clone();
        let driver = state.driver_mut();
        driver
            .engine_mut()
            .proteus_mut()
            .world_mut()
            .resource::<QuadPipeline>()
            .upload_video_frame(&queue, rgba);
        if let Some(demo) = driver.app_mut().demo_mut() {
            demo.set_video_first_frame_shown();
        }
    }

    // ── Photo gallery ────────────────────────────────────────────────────

    /// Returns `Some(side_px)` exactly once per `Loading` entry — the square
    /// pixel size JS should request each of the 12 gallery images at,
    /// capped at [`IMAGE_MAX_SIDE`] (the same cap the generic bake pass
    /// applies, so a big viewport doesn't fetch far more bytes than any
    /// tile can ever show).
    #[wasm_bindgen(js_name = takePendingGalleryFetch)]
    pub fn take_pending_gallery_fetch(&mut self) -> Option<u32> {
        let mut state = self.web_loop.borrow_mut();
        let demo = state.driver_mut().app_mut().demo_mut()?;
        demo.take_pending_gallery_fetch()
            .map(|r| r.tile_side_px.min(IMAGE_MAX_SIDE))
    }

    /// Attaches a fetched gallery image. `photo_id` is stashed for later
    /// reuse if this tile gets enlarged.
    #[wasm_bindgen(js_name = setGalleryTileImage)]
    pub fn set_gallery_tile_image(
        &mut self,
        tile_idx: u32,
        bytes: &[u8],
        photo_id: u32,
        aspect_w: f32,
        aspect_h: f32,
    ) {
        self.tile_photo_id[tile_idx as usize] = Some(photo_id);
        let mut state = self.web_loop.borrow_mut();
        let (engine, app) = state.driver_mut().split_mut();
        if let Some(demo) = app.demo_mut() {
            demo.set_gallery_tile_image(
                engine.proteus_mut(),
                tile_idx as usize,
                bytes.to_vec(),
                Vec2::new(aspect_w, aspect_h),
            );
        }
    }

    /// Returns the pending hires fetch for the enlarged gallery image, once
    /// per `GalleryImage` entry. No cap applied here — unlike the low-res
    /// tile fetch, the hires overlay is the one place a bigger image is the
    /// whole point; `Demo`'s own fitted size is already reasonable.
    #[wasm_bindgen(js_name = takePendingGalleryHiresFetch)]
    pub fn take_pending_gallery_hires_fetch(&mut self) -> Option<GalleryHiresFetchRequest> {
        let mut state = self.web_loop.borrow_mut();
        let demo = state.driver_mut().app_mut().demo_mut()?;
        let request = demo.take_pending_gallery_hires_fetch()?;
        Some(GalleryHiresFetchRequest {
            tile_idx: request.idx as u32,
            width: request.width_px,
            height: request.height_px,
            photo_id: self.tile_photo_id[request.idx].unwrap_or(0),
        })
    }

    /// `true` exactly once whenever the hires fetch was just cancelled — JS
    /// should `.abort()` its in-flight fetch's `AbortController`, if any.
    #[wasm_bindgen(js_name = takePendingGalleryHiresCancel)]
    pub fn take_pending_gallery_hires_cancel(&mut self) -> bool {
        let mut state = self.web_loop.borrow_mut();
        state
            .driver_mut()
            .app_mut()
            .demo_mut()
            .map(|d| d.take_pending_gallery_hires_cancel())
            .unwrap_or(false)
    }

    /// Attaches the fetched hires bytes to the enlarged gallery view — the
    /// generic bake pass (`Engine::frame` → `Renderer::render`) picks up the
    /// `Image` component this sets and uploads it, same as any other image.
    #[wasm_bindgen(js_name = setGalleryHiresImage)]
    pub fn set_gallery_hires_image(&mut self, tile_idx: u32, bytes: &[u8]) {
        let mut state = self.web_loop.borrow_mut();
        let (engine, app) = state.driver_mut().split_mut();
        if let Some(demo) = app.demo_mut() {
            demo.set_gallery_hires_image(engine.proteus_mut(), tile_idx as usize, bytes.to_vec());
        }
    }
}
