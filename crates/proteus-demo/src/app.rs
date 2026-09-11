//! [`DemoApp`] — the reference demo as a [`proteus_runtime::App`] (M13.1).
//!
//! Wraps [`Demo`] (which no longer owns its `Proteus`) and owns the asset
//! loading the M12 shells used to hand-roll: every `set_*` call now pulls
//! bytes from the host via [`Frame::load_asset`] / [`Frame::load_texture`].
//!
//! The native video (`.mp4` / ffmpeg) and gallery (`picsum` fetch) flows are
//! **not** here yet — they stay a thin shell shim (`Demo::take_pending_*`)
//! until M13.4 makes them host services.

use proteus_runtime::{App, Frame, TextureRequest};

use crate::Demo;

/// Source frame art (208×288) is larger than the mark's on-screen footprint;
/// downscale before packing. Matches the M12 shells' `LOGO_FRAME_MAX_SIDE`.
const LOGO_FRAME_MAX_SIDE: u32 = 220;

/// Number of frames in each (light / dark) logo hatch-sweep set.
const LOGO_FRAME_COUNT: u32 = 19;

/// The reference demo, ready to hand to a host's `run()`.
pub struct DemoApp {
    demo: Option<Demo>,
}

impl DemoApp {
    pub fn new() -> Self {
        Self { demo: None }
    }

    /// The wrapped [`Demo`] once `setup` has run — for a host still driving
    /// the M13.4-debt video / gallery / texture-churn shims (`take_pending_*`)
    /// outside the `App` contract. `None` before the first frame.
    pub fn demo_mut(&mut self) -> Option<&mut Demo> {
        self.demo.as_mut()
    }
}

impl Default for DemoApp {
    fn default() -> Self {
        Self::new()
    }
}

impl App for DemoApp {
    fn setup(&mut self, f: &mut Frame) {
        let mut demo = Demo::new(f.proteus);
        demo.set_viewport_size(f.proteus, f.viewport.logical_size);
        load_assets(&mut demo, f);
        self.demo = Some(demo);
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        if let Some(demo) = self.demo.as_mut() {
            demo.advance(f.proteus, dt);
        }
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
