//! [`DemoApp`]: the reference demo as a [`proteus_runtime::App`].
//!
//! [`Demo`] holds the demo's logic, and has no access to the GPU, the network
//! or assets, so it can be tested without them. `DemoApp` wraps it and does
//! that work in `update` through [`Frame`]: it loads assets with
//! [`Frame::load_asset`] and [`Frame::load_texture`], bakes the textures `Demo`
//! generates with [`Frame::bake_texture`], fetches gallery photos with
//! [`Frame::fetch_async`] (see [`crate::gallery_fetch`]), and uploads video
//! frames from the shell's player (see [`crate::video`]) with
//! `VideoHandle::upload_frame`.

use std::collections::HashMap;

use proteus_runtime::{App, FetchId, Frame, ProteusConfig, TextureRequest};
use proteus_sdk::Proteus;

use crate::screens::gallery::TILE_COUNT as GALLERY_TILE_COUNT;
use crate::video::{VideoSource, VideoStream};
use crate::{gallery_fetch, Demo};

/// The logo frames (208×288) are larger than the logo is drawn, so they are
/// scaled down to this before packing.
const LOGO_FRAME_MAX_SIDE: u32 = 220;

/// Number of frames in each (light / dark) logo hatch-sweep set.
const LOGO_FRAME_COUNT: u32 = 19;

/// The largest side, in pixels, of a gallery tile's photo. A large window
/// shouldn't fetch much more than a tile of about 186 pixels can show.
const MAX_TILE_IMAGE_SIDE_PX: f32 = 400.0;

/// The largest side, in pixels, of an enlarged gallery photo, where a bigger
/// image is the point.
const GALLERY_LARGE_IMAGE_MAX_SIDE_PX: f32 = 900.0;

/// What a completed [`Frame::fetch_async`] is for, looked up by its
/// [`FetchId`]. A fetch that isn't in the map is ignored.
enum PendingGalleryFetch {
    Tile(usize, glam::Vec2),
    Hires(usize),
}

/// The reference demo, ready to hand to a host's `run()`.
pub struct DemoApp {
    demo: Option<Demo>,
    /// The picsum photo ID each tile shows, so an enlarged tile can fetch the
    /// same photo at a larger size. `Demo` only knows the photos' shapes.
    tile_photo_id: [Option<u32>; GALLERY_TILE_COUNT],
    gallery_fetches: HashMap<FetchId, PendingGalleryFetch>,
    /// The large-photo fetch in progress, if any; there is at most one. It is
    /// also in `gallery_fetches`, but kept here so it can be cancelled without
    /// searching the map.
    hires_fetch: Option<FetchId>,
    /// The shell's video player. `None` runs the demo without video: its
    /// video requests are ignored. Both shells pass `Some`.
    video_source: Option<Box<dyn VideoSource>>,
    /// The video playing, if any.
    video_stream: Option<Box<dyn VideoStream>>,
    /// The viewport size last given to `Demo::set_viewport_size`, so that
    /// `update` can tell when it changes. See `advance_viewport`.
    last_viewport: Option<glam::Vec2>,
}

impl DemoApp {
    /// `video_source` is the shell's video player, or `None` for no video.
    pub fn new(video_source: Option<Box<dyn VideoSource>>) -> Self {
        Self {
            demo: None,
            tile_photo_id: [None; GALLERY_TILE_COUNT],
            gallery_fetches: HashMap::new(),
            hires_fetch: None,
            video_source,
            video_stream: None,
            last_viewport: None,
        }
    }

    /// The wrapped [`Demo`], once `setup` has run. For tests.
    #[cfg(test)]
    pub(crate) fn demo_mut(&mut self) -> Option<&mut Demo> {
        self.demo.as_mut()
    }

    /// Passes a changed viewport size to [`Demo::set_viewport_size`], so the
    /// background, the gallery layout and the corner icons follow the window.
    ///
    /// Only when the size changes: `set_viewport_size` recomputes every
    /// gallery tile's declared geometry, which would disturb a tile that is
    /// transitioning.
    fn advance_viewport(&mut self, demo: &mut Demo, f: &mut Frame) {
        let size = f.viewport.logical_size;
        if self.last_viewport == Some(size) {
            return;
        }
        self.last_viewport = Some(size);
        demo.set_viewport_size(f.proteus, size);
    }

    /// Starts and stops the player as `Demo` asks, and uploads its frames to
    /// `Demo`'s video. Does nothing without a video source.
    fn advance_video(&mut self, demo: &mut Demo, f: &mut Frame) {
        let Some(source) = self.video_source.as_mut() else {
            return;
        };

        if let Some(idx) = demo.take_pending_video_start() {
            if let Some(old) = self.video_stream.take() {
                old.stop();
            }
            self.video_stream = source.open(idx);
        }

        if demo.take_pending_video_stop() {
            if let Some(stream) = self.video_stream.take() {
                stream.stop();
            }
        }

        if demo.take_pending_video_cancel() {
            if let Some(stream) = self.video_stream.as_mut() {
                stream.cancel_load();
            }
        }

        let (Some(stream), Some(video)) = (self.video_stream.as_mut(), demo.video()) else {
            return;
        };
        if let Some(frame) = stream.poll_frame() {
            if video.upload_frame(f.proteus, frame.width, frame.height, &frame.rgba) {
                demo.set_video_first_frame_shown();
            }
        }
    }

    /// Starts this frame's requested gallery fetches and handles the ones that
    /// completed.
    fn advance_gallery(&mut self, demo: &mut Demo, f: &mut Frame) {
        let scale = f.viewport.scale_factor;

        if let Some(request) = demo.take_pending_gallery_fetch() {
            let nonce = gallery_fetch::fresh_nonce();
            let side_px = ((request.tile_side_px as f32 * scale).min(MAX_TILE_IMAGE_SIDE_PX))
                .round()
                .max(1.0) as u32;
            for idx in 0..GALLERY_TILE_COUNT {
                let (url, photo_id, aspect) = gallery_fetch::tile_fetch_url(nonce, idx, side_px);
                self.tile_photo_id[idx] = Some(photo_id);
                let id = f.fetch_async(&url);
                self.gallery_fetches
                    .insert(id, PendingGalleryFetch::Tile(idx, aspect));
            }
        }

        if let Some(request) = demo.take_pending_gallery_hires_fetch() {
            if let Some(old) = self.hires_fetch.take() {
                f.cancel_fetch(old);
                self.gallery_fetches.remove(&old);
            }
            match self.tile_photo_id[request.idx] {
                Some(photo_id) => {
                    let physical_w = request.width_px as f32 * scale;
                    let physical_h = request.height_px as f32 * scale;
                    let cap_scale =
                        (GALLERY_LARGE_IMAGE_MAX_SIDE_PX / physical_w.max(physical_h)).min(1.0);
                    let width = (physical_w * cap_scale).round().max(1.0) as u32;
                    let height = (physical_h * cap_scale).round().max(1.0) as u32;
                    let url = gallery_fetch::hires_fetch_url(photo_id, width, height);
                    let id = f.fetch_async(&url);
                    self.gallery_fetches
                        .insert(id, PendingGalleryFetch::Hires(request.idx));
                    self.hires_fetch = Some(id);
                }
                None => log::warn!("gallery hires: tile {} has no known photo id", request.idx),
            }
        }

        if demo.take_pending_gallery_hires_cancel() {
            if let Some(old) = self.hires_fetch.take() {
                f.cancel_fetch(old);
                self.gallery_fetches.remove(&old);
            }
        }

        for (id, bytes) in f.poll_fetches() {
            let Some(kind) = self.gallery_fetches.remove(&id) else {
                continue;
            };
            match kind {
                PendingGalleryFetch::Tile(idx, aspect) => match bytes {
                    Some(bytes) => {
                        demo.set_gallery_tile_image(f.proteus, idx, bytes.to_vec(), aspect)
                    }
                    None => log::warn!("gallery tile {idx}: fetch failed"),
                },
                PendingGalleryFetch::Hires(idx) => {
                    if self.hires_fetch == Some(id) {
                        self.hires_fetch = None;
                    }
                    match bytes {
                        Some(bytes) => demo.set_gallery_hires_image(f.proteus, idx, bytes.to_vec()),
                        None => log::warn!("gallery hires {idx}: fetch failed"),
                    }
                }
            }
        }
    }
}

/// The resting page color, shown briefly before the background image loads
/// and behind any component transparency. A light lavender, not black.
const CLEAR_COLOR: [f64; 4] = [
    0xCD as f64 / 255.0,
    0xC7 as f64 / 255.0,
    0xED as f64 / 255.0,
    1.0,
];

/// The largest side, in pixels, the renderer bakes an `Image` at. The
/// enlarged gallery photo sets its own larger cap.
const IMAGE_MAX_SIDE: u32 = 400;

impl DemoApp {
    /// The demo's settings over a platform's `base` preset: its page color
    /// and image size cap.
    pub fn config(base: ProteusConfig) -> ProteusConfig {
        let mut config = base;
        config.render.clear_color = CLEAR_COLOR;
        config.resources.image_max_side = Some(IMAGE_MAX_SIDE);
        config
    }

    /// Every asset key the demo loads in `setup`, for a host that must
    /// download its assets before the app starts, as the web host does.
    pub fn asset_keys() -> Vec<String> {
        image_assets()
            .iter()
            .map(|(key, _)| key.to_string())
            .chain(logo_frame_keys(false))
            .chain(logo_frame_keys(true))
            .collect()
    }
}

impl Default for DemoApp {
    /// A demo without video. Use [`Self::new`] to give it a video player.
    fn default() -> Self {
        Self::new(None)
    }
}

impl App for DemoApp {
    fn setup(&mut self, f: &mut Frame) {
        let mut demo = Demo::new(f.proteus);
        self.advance_viewport(&mut demo, f);
        load_assets(&mut demo, f);
        self.demo = Some(demo);
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        let Some(mut demo) = self.demo.take() else {
            return;
        };
        self.advance_viewport(&mut demo, f);
        demo.advance(f.proteus, dt);
        for update in demo.take_pending_texture_churn() {
            let texture = f.bake_texture(
                update.width,
                update.height,
                update.rgba,
                TextureRequest::default(),
            );
            let _ = update.handle.set_texture(f.proteus, texture);
        }
        self.advance_gallery(&mut demo, f);
        self.advance_video(&mut demo, f);
        self.demo = Some(demo);
    }
}

/// Sets one image asset on the demo, once its bytes are loaded.
type ImageSetter = fn(&mut Demo, &mut Proteus, Vec<u8>);

/// Every image asset the demo loads in `setup`, and where it goes. The logo
/// frames are separate: see [`logo_frame_keys`].
fn image_assets() -> [(&'static str, ImageSetter); 17] {
    [
        ("bg/ocean-blur.jpg", Demo::set_background_image),
        ("bg/ocean-blur-dark.jpg", Demo::set_background_image_dark),
        ("tiger.jpg", |d, p, b| d.set_tile_image(p, 0, b)),
        ("sintel.jpg", |d, p, b| d.set_tile_image(p, 1, b)),
        ("jellyfish.jpg", |d, p, b| d.set_tile_image(p, 2, b)),
        ("icons/home-idle.png", Demo::set_nav_home_icon),
        ("icons/home-idle-dark.png", Demo::set_nav_home_icon_dark),
        ("icons/home-selected.png", Demo::set_nav_home_icon_selected),
        (
            "icons/home-selected-dark.png",
            Demo::set_nav_home_icon_selected_dark,
        ),
        ("icons/back-idle.png", Demo::set_nav_back_icon),
        ("icons/back-idle-dark.png", Demo::set_nav_back_icon_dark),
        ("logo/lockup.png", Demo::set_nav_logo_lockup),
        ("logo/lockup-dark.png", Demo::set_nav_logo_lockup_dark),
        // The sun icon's light and dark assets are swapped on purpose; see
        // `proteus_demo::screens::theme`.
        ("icons/sun-idle-dark.png", Demo::set_theme_sun_icon),
        ("icons/sun-selected.png", Demo::set_theme_sun_icon_dark),
        ("icons/moon-idle.png", Demo::set_theme_moon_icon),
        (
            "icons/moon-selected-dark.png",
            Demo::set_theme_moon_icon_dark,
        ),
    ]
}

/// The keys of the light or dark logo frames, in order.
fn logo_frame_keys(dark: bool) -> impl Iterator<Item = String> {
    let suffix = if dark { "-dark" } else { "" };
    (1..=LOGO_FRAME_COUNT).map(move |n| format!("logo/frame-{n:02}{suffix}.png"))
}

fn load_assets(demo: &mut Demo, f: &mut Frame) {
    for (key, set) in image_assets() {
        // A missing asset leaves that part of the demo blank.
        if let Some(bytes) = f.load_asset(key) {
            set(demo, f.proteus, bytes.to_vec());
        }
    }

    let logo_req = TextureRequest {
        max_side: Some(LOGO_FRAME_MAX_SIDE),
        eternal: true,
    };
    let light = logo_frame_keys(false)
        .map(|key| f.load_texture(&key, logo_req))
        .collect();
    demo.set_logo_frames(f.proteus, light);
    let dark = logo_frame_keys(true)
        .map(|key| f.load_texture(&key, logo_req))
        .collect();
    demo.set_loading_logo_frames_dark(dark);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proteus_runtime::{FetchResult, HostServices, Viewport};

    use super::*;

    // Records every asset key requested, and has none of them.
    #[derive(Default)]
    struct RecordingServices {
        requested: Vec<String>,
    }

    impl HostServices for RecordingServices {
        fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>> {
            self.requested.push(key.to_string());
            None
        }
        fn fetch_async(&mut self, _key_or_url: &str) -> FetchId {
            FetchId(0)
        }
        fn poll_fetches(&mut self) -> Vec<FetchResult> {
            Vec::new()
        }
        fn cancel_fetch(&mut self, _id: FetchId) {}
    }

    // The web shell downloads `asset_keys()` before the demo starts, so a key
    // `setup` loads but the list leaves out would silently not load there.
    #[test]
    fn asset_keys_lists_every_asset_setup_loads() {
        let mut proteus = Proteus::new();
        let mut services = RecordingServices::default();
        let mut frame = Frame {
            proteus: &mut proteus,
            services: &mut services,
            viewport: Viewport::new(glam::Vec2::new(1280.0, 800.0), 1.0),
        };
        DemoApp::default().setup(&mut frame);

        let mut requested = services.requested;
        let mut listed = DemoApp::asset_keys();
        requested.sort();
        listed.sort();
        assert_eq!(requested, listed);
    }
}
