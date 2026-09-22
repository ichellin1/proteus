//! [`DemoApp`] — the reference demo as a [`proteus_runtime::App`] (M13.1).
//!
//! Wraps [`Demo`] (which no longer owns its `Proteus`) and owns the asset
//! loading the M12 shells used to hand-roll: every `set_*` call now pulls
//! bytes from the host via [`Frame::load_asset`] / [`Frame::load_texture`].
//!
//! Texture churn (M13.4 step 1), the photo gallery (M13.4 step 3), and video
//! on hosts that have migrated it (M13.4 step 4a, native only so far — see
//! [`DemoApp::new`]'s own doc) are all baked/fetched/played here now too:
//! `Demo` still only generates the synthetic RGBA bytes / requests *that*
//! something happen (it stays headless, with no `Frame`/GPU/network access
//! of its own, on purpose — see its own crate-root doc), but `DemoApp::
//! update` — which does have `Frame` — drains those requests and does the
//! actual work via [`Frame::bake_texture`]/[`Frame::fetch_async`]/
//! [`Frame::play_video`] instead of leaving it to a shell-side shim reaching
//! past `Frame` into the `Engine`. Which photo to fetch and how
//! to build its URL is [`crate::gallery_fetch`] — shared here instead of
//! duplicated per shell, now that there's one real async-fetch primitive
//! both shells can drive identically.
//!
//! Video reaches both hosts the same way, through
//! [`Frame::play_video`]/[`Frame::poll_video`] over
//! `HostServices::open_video` — natively an `ffmpeg`-backed `Mp4Stream`, on
//! the web `proteus-host-web`'s `hls_video` (`<video>`/`MediaSource`, no JS).
//! Only the *keyspace* differs, and that is the host's business: see
//! [`DemoApp::new`]'s `video_keys`.

use std::collections::HashMap;

use proteus_runtime::{App, FetchId, Frame, TextureRequest};

use crate::screens::gallery::TILE_COUNT as GALLERY_TILE_COUNT;
use crate::{gallery_fetch, Demo};

/// Source frame art (208×288) is larger than the mark's on-screen footprint;
/// downscale before packing. Matches the M12 shells' `LOGO_FRAME_MAX_SIDE`.
const LOGO_FRAME_MAX_SIDE: u32 = 220;

/// Number of frames in each (light / dark) logo hatch-sweep set.
const LOGO_FRAME_COUNT: u32 = 19;

/// Matches the M12 shells' own `MAX_TILE_IMAGE_SIDE`/`MAX_IMAGE_SIDE` cap for
/// a gallery grid tile — a big viewport shouldn't fetch far more bytes than
/// a ~186px tile can ever show.
const MAX_TILE_IMAGE_SIDE_PX: f32 = 400.0;

/// Matches the M12 shells' own `GALLERY_LARGE_IMAGE_MAX_SIDE` — the one
/// place a bigger image is the whole point, so its own, higher cap.
const GALLERY_LARGE_IMAGE_MAX_SIDE_PX: f32 = 900.0;

/// What a completed [`Frame::fetch_async`] result routes back to — looked up
/// by [`FetchId`] as fetches complete, since gallery fetches aren't the only
/// possible use of `fetch_async`/`poll_fetches` in principle (any future
/// caller's own ids simply won't be in this map and are ignored).
enum PendingGalleryFetch {
    Tile(usize, glam::Vec2),
    Hires(usize),
}

/// The reference demo, ready to hand to a host's `run()`.
pub struct DemoApp {
    demo: Option<Demo>,
    /// Which picsum photo id each tile's low-res fetch landed on — reused
    /// to fetch the *same* photo bigger when it's enlarged. `Demo` itself
    /// never tracks this (it only knows aspect ratios, not photo ids).
    tile_photo_id: [Option<u32>; GALLERY_TILE_COUNT],
    gallery_fetches: HashMap<FetchId, PendingGalleryFetch>,
    /// The single in-flight hires fetch, if any — at most one at a time,
    /// same invariant the M12 shells' own single-slot channel had. Tracked
    /// separately from `gallery_fetches` (which also holds this same id)
    /// purely so `take_pending_gallery_hires_cancel` has something to look
    /// up without scanning the map for a `Hires` entry.
    hires_fetch: Option<FetchId>,
    /// `Some([left, center, right])` — the keys this module hands
    /// `Frame::play_video`, resolved by whatever keyspace the host's own
    /// `HostServices::open_video` expects (native: literal filesystem paths;
    /// web: an HLS manifest path plus a codec string). Both shells pass
    /// `Some`.
    ///
    /// `None` means "this host has no video at all": `advance_video` becomes
    /// a no-op and the video tiles' pending requests are simply never
    /// drained. No shell needs it today — it's kept so a host without video
    /// support can still run the demo, and it's what the `resize` test
    /// builds against.
    video_keys: Option<[String; 3]>,
    playing_video: Option<proteus_runtime::PlayingVideo>,
    /// Last viewport size handed to `Demo::set_viewport_size`, so `update` can
    /// notice a resize. `Engine::resize` keeps `Frame::viewport` current, but
    /// nothing was reading it after `setup` — see `advance_viewport`.
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

    /// The wrapped [`Demo`] once `setup` has run; `None` before the first
    /// frame.
    ///
    /// Test-only. Every host-side shim that used to reach through this —
    /// texture churn, the gallery, and finally video at M13.4 — now goes
    /// through `Frame` inside `update`, so nothing outside this crate needs
    /// to see the `Demo` itself any more.
    #[cfg(test)]
    pub(crate) fn demo_mut(&mut self) -> Option<&mut Demo> {
        self.demo.as_mut()
    }

    /// Forwards a changed viewport to [`Demo::set_viewport_size`].
    ///
    /// `Demo::set_viewport_size`'s own doc says "call on every resize", and
    /// `Engine::resize` does keep `Frame::viewport` up to date — but nothing
    /// read it after `setup`, so the demo stayed laid out for whatever size the
    /// window happened to open at. The background quad kept its startup size,
    /// the gallery's declared cell geometry went stale, and the nav/theme icons
    /// stayed pinned to the old corners (they position from
    /// `Demo::viewport_size` every frame). A regression from the pre-M13
    /// shells, which called this from their own `Resized` handler.
    ///
    /// Compared rather than called unconditionally: `set_viewport_size` rewrites
    /// every gallery tile's declared geometry, which is real work and — more to
    /// the point — would clobber a tile's declared state mid-transition on every
    /// frame.
    fn advance_viewport(&mut self, demo: &mut Demo, f: &mut Frame) {
        let size = f.viewport.logical_size;
        if self.last_viewport == Some(size) {
            return;
        }
        self.last_viewport = Some(size);
        demo.set_viewport_size(f.proteus, size);
    }

    /// Kicks off / drains this frame's video, on a host that's migrated it
    /// (see `video_keys`'s own doc) — a no-op on a host without video. Split out of
    /// `update` purely for readability — not part of the `App` trait.
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

    /// Kicks off / drains this frame's gallery fetches. Split out of
    /// `update` purely for readability — not part of the `App` trait.
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
    /// Defaults to shell-managed video (`None`) — a host opts into
    /// `HostServices`-driven video explicitly via [`Self::new`].
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

/// One image asset's bytes, or `None` if the host can't find it (the demo
/// degrades to a blank quad — same as the M12 shells).
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

    // The sun icon's light/dark asset roles are inverted on purpose — see
    // `proteus_demo::screens::theme`'s module doc. The keys here match the
    // M12 native shell's own mapping.
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
