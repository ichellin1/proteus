//! The demo's video player interface: Proteus shows video but doesn't play
//! it, so each shell brings a player and hands it to [`crate::DemoApp`].
//!
//! The native shell's player runs `ffmpeg`; the web shell's plays HLS in a
//! `<video>` element. Either way, `DemoApp` opens a [`VideoStream`] for the
//! tile that was clicked, and uploads each frame it delivers with
//! `proteus_sdk::VideoHandle::upload_frame`.

use std::sync::Arc;

/// One decoded frame.
pub struct VideoFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA.
    pub rgba: Arc<[u8]>,
}

/// A shell's video player: opens the video of one of the three video tiles.
pub trait VideoSource {
    /// Starts playing the video of tile `tile` (0, 1 or 2: left, center or
    /// right). Returns `None`, and logs why, if it can't.
    fn open(&mut self, tile: usize) -> Option<Box<dyn VideoStream>>;
}

/// A video being played, from [`VideoSource::open`].
pub trait VideoStream {
    /// Returns the newest decoded frame since the last call, or `None` if
    /// there is none. Doesn't block.
    fn poll_frame(&mut self) -> Option<VideoFrame>;

    /// Cancels the initial load, for when the demo gives up waiting for the
    /// first frame. Unlike [`stop`](Self::stop), a frame may still arrive.
    /// Only a stream that loads over the network can do this; the default
    /// does nothing.
    fn cancel_load(&mut self) {}

    /// Stops playing and releases the player's resources.
    fn stop(self: Box<Self>);
}
