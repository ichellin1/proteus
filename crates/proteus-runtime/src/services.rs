//! [`HostServices`] — per-platform asset fetching, handed to the [`App`]
//! through [`Frame`].
//!
//! M13.1 defined the **synchronous byte-fetch** seam: the app asks for an
//! asset by key, the host returns its bytes immediately. M13.4 adds two more:
//! the **async fetch** seam — [`HostServices::fetch_async`]/[`poll_fetches`]/
//! [`cancel_fetch`] — for work that can't resolve on the spot (an asset not
//! in a web host's prefetched set, or an arbitrary third-party URL — the
//! reference demo's photo gallery); and the **video** seam —
//! [`HostServices::open_video`] / [`VideoStream`] — for a decode that's a
//! long-lived stream, not a single result. Turning either's bytes into a GPU
//! texture is [`Frame`]'s job either way ([`Frame::load_texture`]/
//! [`Frame::bake_texture`]/[`Frame::poll_video`] — they need the world's
//! `QuadPipeline`, which a `HostServices` impl has no handle to —
//! deliberately kept GPU-unaware, matching every impl in this repo).
//!
//! [`App`]: crate::App
//! [`Frame`]: crate::Frame
//! [`Frame::load_texture`]: crate::Frame::load_texture
//! [`Frame::bake_texture`]: crate::Frame::bake_texture
//! [`Frame::poll_video`]: crate::Frame::poll_video
//! [`poll_fetches`]: HostServices::poll_fetches
//! [`cancel_fetch`]: HostServices::cancel_fetch

use std::sync::Arc;

/// Opaque identifier for an in-flight [`HostServices::fetch_async`] request —
/// correlate with its eventual result via [`HostServices::poll_fetches`].
/// Each `HostServices` impl mints its own ids; they're only meaningful
/// against the instance that returned them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FetchId(pub u64);

/// One [`HostServices::poll_fetches`] result: the id it answers and the
/// bytes, or `None` on failure/not-found (see that method's own doc).
pub type FetchResult = (FetchId, Option<Arc<[u8]>>);

/// The asset services a host provides to the running [`App`].
///
/// **Two seams, both "resolves on this host's own terms":**
/// - [`load_asset`](HostServices::load_asset) — synchronous, resolves
///   immediately. On native, a file read under a host-configured base
///   directory; on the web host, a lookup into a set fetched up front (see
///   `proteus-host-web::PreloadedHostServices`) — never a live network call,
///   since `fetch` can't resolve synchronously.
/// - [`fetch_async`](HostServices::fetch_async) — for everything
///   `load_asset` can't serve on the spot: an asset not in the web host's
///   prefetched set, or an arbitrary URL a host is willing to reach (both
///   impls in this repo accept one) — kicked off now, polled for later via
///   [`poll_fetches`](HostServices::poll_fetches).
///
/// [`App`]: crate::App
pub trait HostServices {
    /// Fetch an asset's raw bytes by platform-agnostic key (e.g.
    /// `"nav/home-idle.png"`). `None` = not found. `Arc` so a caller can
    /// hold the bytes (e.g. as an [`Image`](proteus_ui::Image) component)
    /// without copying.
    fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>>;

    /// Start an async fetch for `key_or_url` and return immediately — poll
    /// [`poll_fetches`](Self::poll_fetches) once per frame for the result.
    /// `key_or_url` is the same keyspace [`load_asset`](Self::load_asset)
    /// resolves a plain key against; both `HostServices` impls in this repo
    /// also accept a full `http(s)://` URL, treating it as a fetch outside
    /// their own asset keyspace entirely (M13.4 — one primitive covers both
    /// "lazily load an asset not in the prefetched set" and "fetch an
    /// arbitrary third-party URL").
    fn fetch_async(&mut self, key_or_url: &str) -> FetchId;

    /// Drain fetches that completed since the last call. `None` = the fetch
    /// failed or the resource wasn't found — same "quiet" convention as
    /// [`load_asset`](Self::load_asset); a host logs its own error detail.
    /// Call once per frame.
    fn poll_fetches(&mut self) -> Vec<FetchResult>;

    /// Best-effort: stop delivering — and, where the host can actually
    /// interrupt the underlying operation (the web host aborts the
    /// in-flight `fetch`; native can't cut short a blocking request already
    /// running on its own thread, so it just discards the result), abort —
    /// a fetch no longer wanted. A cancelled id is never expected to surface
    /// from [`poll_fetches`](Self::poll_fetches) afterward in either impl
    /// here, but a caller should tolerate one arriving anyway (e.g. by
    /// forgetting its own bookkeeping for the id first) rather than assume
    /// every `HostServices` impl can guarantee it.
    fn cancel_fetch(&mut self, id: FetchId);

    /// Start decoding video `key` (this host's own keyspace/URL scheme —
    /// same idea as [`load_asset`](Self::load_asset)/
    /// [`fetch_async`](Self::fetch_async), just for a stream instead of a
    /// single result). `None` if the host couldn't even start (bad path, no
    /// decoder available, video unsupported on this host, …) — logged by
    /// the host. Per-codec decode (`.mp4` via `ffmpeg` on native, HLS via
    /// `<video>`/`MediaSource` on the web host) lives entirely inside each
    /// host's own implementation of this method; nothing above it knows or
    /// cares. Defaults to "unsupported" so a `HostServices` impl that never
    /// needs video isn't forced to write a decoder.
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        let _ = key;
        None
    }
}

/// One decoded frame from an open [`VideoStream`].
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, RGBA8.
    pub rgba: Arc<[u8]>,
}

/// A live video decode, opened by [`HostServices::open_video`] — polled by
/// [`Frame::poll_video`](crate::Frame::poll_video) once per frame.
/// Deliberately the only GPU-adjacent-shaped type `HostServices` deals with,
/// and even this carries no GPU handle: turning a [`VideoFrame`] into a
/// texture is `Frame`'s job (it has the world's `QuadPipeline`; a
/// `HostServices` impl never does), matching every other seam in this file.
pub trait VideoStream {
    /// Non-blocking: `None` if no new frame has decoded since the last call.
    fn poll_frame(&mut self) -> Option<VideoFrame>;

    /// Best-effort: abort the *initial* load without fully stopping —
    /// unlike [`stop`](Self::stop), the host may still eventually produce a
    /// first frame if a response was already close. Used when the app's own
    /// load-timeout fires before `poll_frame` has ever returned anything.
    /// Default no-op: most decoders (e.g. a local `ffmpeg` process) have no
    /// meaningful "abort just the load, not playback" state — this exists
    /// for hosts whose decode is itself a cancellable network fetch (the
    /// web host's HLS manifest/segment requests).
    fn cancel_load(&mut self) {}

    /// Stop decoding and release whatever resources this stream holds
    /// (kill a decode process/thread, abort any in-flight fetch, …).
    fn stop(self: Box<Self>);
}
