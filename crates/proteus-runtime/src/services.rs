//! [`HostServices`] — per-platform asset fetching, handed to the [`App`]
//! through [`Frame`].
//!
//! M13.1 defines the **byte-fetch** seam only: the app asks for an asset by
//! key, the host returns its bytes. Turning bytes into a GPU texture is
//! [`Frame::load_texture`] (it needs the world's `QuadPipeline`, which the
//! host has no handle to). Fonts and video as host services, and a richer
//! return type for assets that arrive later (`fetch` on the web host,
//! M13.2), are M13.4.
//!
//! [`App`]: crate::App
//! [`Frame`]: crate::Frame
//! [`Frame::load_texture`]: crate::Frame::load_texture

use std::sync::Arc;

/// Options for [`Frame::load_texture`](crate::Frame::load_texture).
#[derive(Debug, Clone, Copy, Default)]
pub struct TextureRequest {
    /// Downscale cap (longest side, pixels) before packing into `main_atlas`.
    /// `None` = pack at native resolution.
    pub max_side: Option<u32>,
    /// Pin the texture in the atlas for the app's lifetime — never
    /// LRU-evicted. For assets referenced continuously, e.g. an animation
    /// frame set that must all stay resident.
    pub eternal: bool,
}

/// The asset services a [`Host`] provides to the running [`App`].
///
/// **Resolution model (M13.1 decision): resolves immediately.** On native,
/// [`load_asset`](HostServices::load_asset) is a synchronous file read under
/// a host-configured base directory. The key → path (or URL) mapping is host
/// configuration, not app code.
///
/// The web host (M13.2) cannot read synchronously — `fetch` is async — so it
/// will need a return type that can say "not yet". Generalising this
/// (progress, failure surfacing, lazy-on-visible, video) is M13.4; M13.1 is
/// deliberately the narrow synchronous case the native reference demo needs.
///
/// [`Host`]: crate::Host
/// [`App`]: crate::App
pub trait HostServices {
    /// Fetch an asset's raw bytes by platform-agnostic key (e.g.
    /// `"nav/home-idle.png"`). `None` = not found. `Arc` so a caller can
    /// hold the bytes (e.g. as an [`Image`](proteus_ui::Image) component)
    /// without copying.
    fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>>;
}
