//! [`HostServices`]: the platform services Proteus's own APIs need, provided
//! by the host.
//!
//! - **Loading an asset** ([`HostServices::load_asset`]) returns its bytes
//!   immediately.
//! - **Fetching** ([`HostServices::fetch_async`]) starts a request and
//!   delivers the result later, for anything that can't be loaded
//!   immediately, such as an image from another site.
//! - **Video** ([`HostServices::open_video`]) opens a stream of decoded frames.
//!
//! None of them touch the GPU. Turning bytes or frames into textures is done
//! by [`Frame`](crate::Frame).
//!
//! This isn't a list of everything an app can use. An app is ordinary code,
//! and uses any other platform feature, such as the clipboard or a file
//! picker, directly. For a feature that works differently on each platform,
//! give the app its own trait with an implementation per platform, and pass
//! the right one in where the app is created.

use std::sync::Arc;

/// Identifies a [`HostServices::fetch_async`] request, so its result from
/// [`HostServices::poll_fetches`] can be matched to it. IDs only mean
/// something to the `HostServices` that issued them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FetchId(pub u64);

/// One result from [`HostServices::poll_fetches`]: the request's ID, and its
/// bytes, or `None` if it failed.
pub type FetchResult = (FetchId, Option<Arc<[u8]>>);

/// Asset loading, fetching and video playback, provided by a host to the
/// running [`App`](crate::App).
///
/// [`load_asset`](HostServices::load_asset) returns immediately. On native it
/// reads a file from the host's asset directory; on the web it looks the asset
/// up in a set the host downloaded before starting, since a browser can't fetch
/// synchronously. For anything else, such as a URL,
/// [`fetch_async`](HostServices::fetch_async) starts a request and
/// [`poll_fetches`](HostServices::poll_fetches) delivers the result.
pub trait HostServices {
    /// Returns an asset's bytes by key, such as `"nav/home-idle.png"`, or
    /// `None` if it isn't found. The bytes are shared rather than copied.
    fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>>;

    /// Starts fetching `key_or_url` and returns immediately. The result
    /// arrives through [`poll_fetches`](Self::poll_fetches).
    ///
    /// A plain key means the same as in [`load_asset`](Self::load_asset). Both
    /// hosts also accept a full `http://` or `https://` URL.
    fn fetch_async(&mut self, key_or_url: &str) -> FetchId;

    /// Returns the fetches that completed since the last call. A result of
    /// `None` means the fetch failed or found nothing; the host logs why. Call
    /// once per frame.
    fn poll_fetches(&mut self) -> Vec<FetchResult>;

    /// Cancels a fetch that is no longer wanted. After this call,
    /// [`poll_fetches`](Self::poll_fetches) never returns its result, even if
    /// the fetch had already finished.
    ///
    /// The web host also aborts the request. The native host can't interrupt
    /// a request already in progress, so it lets it finish and discards the
    /// result.
    fn cancel_fetch(&mut self, id: FetchId);

    /// Starts decoding the video `key`. Returns `None`, and logs why, if the
    /// player can't open it.
    ///
    /// Proteus doesn't play video: an app brings its own player and hands
    /// Proteus the frames. Implementing this method is how a player is plugged
    /// in: start it here, and return its frames as a [`VideoStream`]. Both
    /// hosts include a reference player, `ffmpeg` natively and HLS on the web.
    /// The default supports no video.
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        let _ = key;
        None
    }
}

/// One decoded frame from a [`VideoStream`].
pub struct VideoFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA.
    pub rgba: Arc<[u8]>,
}

/// A video being decoded, from [`HostServices::open_video`].
/// [`Frame::poll_video`](crate::Frame::poll_video) reads it once per frame.
pub trait VideoStream {
    /// Returns the next decoded frame, or `None` if none is ready. Doesn't
    /// block.
    fn poll_frame(&mut self) -> Option<VideoFrame>;

    /// Cancels the initial load, for when the app gives up waiting for the
    /// first frame. Unlike [`stop`](Self::stop), a frame may still arrive.
    ///
    /// Only a stream that loads over the network, such as the web host's HLS
    /// stream, can do this. The default does nothing.
    fn cancel_load(&mut self) {}

    /// Stops decoding and releases the stream's resources, such as a decoder
    /// process or a network request.
    fn stop(self: Box<Self>);
}
