//! [`DemoApp`]: the reference demo as a [`proteus_runtime::App`].
//!
//! [`Demo`] holds the demo's logic, and has no access to the GPU, the network
//! or assets, so it can be tested without them. `DemoApp` wraps it and does
//! that work in `update` through [`Frame`]: it loads assets with
//! [`Frame::load_asset`] and [`Frame::load_texture`], bakes the textures `Demo`
//! generates with [`Frame::bake_texture`], fetches gallery photos with
//! [`Frame::fetch_async`] (see [`crate::gallery_fetch`]), and plays video with
//! [`Frame::play_video`].
//!
//! Video works the same on both hosts, through `HostServices::open_video`:
//! natively an `ffmpeg`-backed MP4 stream, on the web an HLS stream in a
//! `<video>` element. Only the video keys differ; see [`DemoApp::new`].

use std::collections::HashMap;

use proteus_runtime::{App, FetchId, Frame, TextureRequest};

use crate::screens::gallery::TILE_COUNT as GALLERY_TILE_COUNT;
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
    /// The video keys for the left, center and right tiles, passed to
    /// `Frame::play_video`. Their format is the host's: file paths natively,
    /// an HLS directory and codec string on the web.
    ///
    /// `None` runs the demo without video: its video requests are ignored.
    /// Both shells pass `Some`.
    video_keys: Option<[String; 3]>,
    playing_video: Option<proteus_runtime::PlayingVideo>,
    /// The viewport size last given to `Demo::set_viewport_size`, so that
    /// `update` can tell when it changes. See `advance_viewport`.
    last_viewport: Option<glam::Vec2>,
}

impl DemoApp {
    /// `video_keys`: see the field's own doc.
    pub fn new(video_keys: Option<[String; 3]>) -> Self {
        Self {
            demo: None,
            tile_photo_id: [None; GALLERY_TILE_COUNT],
            gallery_fetches: HashMap::new(),
            hires_fetch: None,
            video_keys,
            playing_video: None,
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

    /// Starts this frame's requested videos and uploads their frames. Does
    /// nothing without video keys.
    fn advance_video(&mut self, demo: &mut Demo, f: &mut Frame) {
        let Some(video_keys) = &self.video_keys else {
            return;
        };

        if let Some(idx) = demo.take_pending_video_start() {
            if let Some(old) = self.playing_video.take() {
                f.stop_video(old);
            }
            self.playing_video = f.play_video(&video_keys[idx]);
        }

        if demo.take_pending_video_stop() {
            if let Some(playing) = self.playing_video.take() {
                f.stop_video(playing);
            }
        }

        if demo.take_pending_video_cancel() {
            if let Some(playing) = self.playing_video.as_mut() {
                f.cancel_video_load(playing);
            }
        }

        if let Some(playing) = self.playing_video.as_mut() {
            if f.poll_video(playing) {
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

impl Default for DemoApp {
    /// A demo without video. Use [`Self::new`] to give it video keys.
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

/// One image asset's bytes, or `None` if the host can't find it, in which
/// case that part of the demo stays blank.
fn asset_bytes(f: &mut Frame, key: &str) -> Option<Vec<u8>> {
    f.load_asset(key).map(|b| b.to_vec())
}

fn load_assets(demo: &mut Demo, f: &mut Frame) {
    macro_rules! set_img {
        ($key:expr, $setter:ident) => {
            if let Some(bytes) = asset_bytes(f, $key) {
                demo.$setter(f.proteus, bytes);
            }
        };
    }

    set_img!("bg/ocean-blur.jpg", set_background_image);
    set_img!("bg/ocean-blur-dark.jpg", set_background_image_dark);

    for (idx, key) in ["tiger.jpg", "sintel.jpg", "jellyfish.jpg"]
        .iter()
        .enumerate()
    {
        if let Some(bytes) = asset_bytes(f, key) {
            demo.set_tile_image(f.proteus, idx, bytes);
        }
    }

    set_img!("icons/home-idle.png", set_nav_home_icon);
    set_img!("icons/home-idle-dark.png", set_nav_home_icon_dark);
    set_img!("icons/home-selected.png", set_nav_home_icon_selected);
    set_img!(
        "icons/home-selected-dark.png",
        set_nav_home_icon_selected_dark
    );
    set_img!("icons/back-idle.png", set_nav_back_icon);
    set_img!("icons/back-idle-dark.png", set_nav_back_icon_dark);
    set_img!("logo/lockup.png", set_nav_logo_lockup);
    set_img!("logo/lockup-dark.png", set_nav_logo_lockup_dark);

    // The sun icon's light and dark assets are swapped on purpose; see
    // `proteus_demo::screens::theme`.
    set_img!("icons/sun-idle-dark.png", set_theme_sun_icon);
    set_img!("icons/sun-selected.png", set_theme_sun_icon_dark);
    set_img!("icons/moon-idle.png", set_theme_moon_icon);
    set_img!("icons/moon-selected-dark.png", set_theme_moon_icon_dark);

    let logo_req = TextureRequest {
        max_side: Some(LOGO_FRAME_MAX_SIDE),
        eternal: true,
    };
    let light = (1..=LOGO_FRAME_COUNT)
        .map(|n| f.load_texture(&format!("logo/frame-{n:02}.png"), logo_req))
        .collect();
    demo.set_logo_frames(f.proteus, light);

    let dark = (1..=LOGO_FRAME_COUNT)
        .map(|n| f.load_texture(&format!("logo/frame-{n:02}-dark.png"), logo_req))
        .collect();
    demo.set_loading_logo_frames_dark(dark);
}
