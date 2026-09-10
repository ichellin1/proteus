//! [`HostServices`] — per-platform asset fulfilment, handed to the [`App`]
//! through [`Frame`].
//!
//! M13.1 defines **only** the texture path — the one thing the M13.8
//! TypeScript POC needs. Fonts, runtime image-loading policy, and video as a
//! host service are M13.4.
//!
//! [`App`]: crate::App
//! [`Frame`]: crate::Frame

use proteus_sdk::TextureHandle;

/// Options for a [`HostServices::load_texture`] request.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextureRequest {
    /// Downscale cap (longest side, pixels) before packing into `main_atlas`.
    /// `None` = the host's platform default.
    pub max_side: Option<u32>,
}

/// The asset services a [`Host`] provides to the running [`App`].
///
/// **Resolution model (M13.1 decision): placeholder that resolves
/// immediately.** [`load_texture`](HostServices::load_texture) returns a live
/// [`TextureHandle`] synchronously; the host fulfils the bytes on its own
/// schedule (`std::fs::read` on native, `fetch` on web), and the renderer's
/// pending-`Image` bake pass picks them up on a later frame. Until then the
/// component renders as a blank / transparent quad — exactly today's
/// "asset not loaded yet" behaviour. The app writes no `async` and does no
/// polling; the `take_pending_*` inversion in the M12 shells disappears.
///
/// A fuller async / loading-state contract (progress, failure surfacing,
/// lazy-on-visible) is M13.4.
///
/// [`Host`]: crate::Host
/// [`App`]: crate::App
pub trait HostServices {
    /// Request a texture by platform-agnostic key (e.g. `"nav/home-idle.png"`).
    ///
    /// The key → disk-path / URL mapping is host configuration, not app code.
    /// Returns immediately; see the trait-level note on the resolution model.
    fn load_texture(&mut self, key: &str, req: TextureRequest) -> TextureHandle;
}
