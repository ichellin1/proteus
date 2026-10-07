//! [`HostServices`]: the platform services Proteus's own APIs need, provided
//! by the host.
//!
//! - **Loading an asset** ([`HostServices::load_asset`]) returns its bytes
//!   immediately.
//! - **Fetching** ([`HostServices::fetch_async`]) starts a request and
//!   delivers the result later, for anything that can't be loaded
//!   immediately, such as an image from another site.
//!
//! Neither touches the GPU. Turning bytes into textures is done by
//! [`Frame`](crate::Frame).
//!
//! Video isn't a host service: Proteus shows video but doesn't play it. The
//! app brings its own player and uploads its frames with
//! `proteus_sdk::Proteus::create_video`.
//!
//! This isn't a list of everything an app can use. An app is ordinary code,
//! and uses any other platform feature, such as the clipboard or a file
//! picker, directly. For a feature that works differently on each platform,
//! give the app its own trait with an implementation per platform, and pass
//! the right one in where the app is created.

use std::collections::HashSet;
use std::sync::Arc;

/// Identifies a [`HostServices::fetch_async`] request, so its result from
/// [`HostServices::poll_fetches`] can be matched to it. IDs only mean
/// something to the `HostServices` that issued them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FetchId(pub u64);

/// One result from [`HostServices::poll_fetches`]: the request's ID, and its
/// bytes, or `None` if it failed.
pub type FetchResult = (FetchId, Option<Arc<[u8]>>);

/// Bookkeeping for a host's [`HostServices::fetch_async`] requests: which are
/// running, which were cancelled, and which results wait for the next
/// [`HostServices::poll_fetches`]. Both hosts use it, so they keep
/// [`HostServices::cancel_fetch`]'s promise the same way.
///
/// Call [`started`](Self::started) when a fetch begins and
/// [`finished`](Self::finished) when it ends, however it ended.
#[derive(Debug, Default)]
pub struct FetchTracker {
    running: HashSet<FetchId>,
    cancelled: HashSet<FetchId>,
    results: Vec<FetchResult>,
}

impl FetchTracker {
    /// Records that the fetch `id` has begun.
    pub fn started(&mut self, id: FetchId) {
        self.running.insert(id);
    }

    /// Records the fetch `id`'s result, which [`take`](Self::take) then
    /// returns, unless the fetch was cancelled.
    pub fn finished(&mut self, id: FetchId, bytes: Option<Arc<[u8]>>) {
        self.running.remove(&id);
        if !self.cancelled.remove(&id) {
            self.results.push((id, bytes));
        }
    }

    /// Cancels the fetch `id`, so [`take`](Self::take) never returns its
    /// result: one still running is discarded when it finishes, and one
    /// already finished is dropped now. Returns `true` if it was still
    /// running, so a host that can abort a request knows to.
    pub fn cancel(&mut self, id: FetchId) -> bool {
        if self.running.contains(&id) {
            self.cancelled.insert(id);
            true
        } else {
            self.results.retain(|(done, _)| *done != id);
            false
        }
    }

    /// The results of fetches that finished since the last call.
    pub fn take(&mut self) -> Vec<FetchResult> {
        std::mem::take(&mut self.results)
    }
}

/// Asset loading and fetching, provided by a host to the running
/// [`App`](crate::App).
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes() -> Option<Arc<[u8]>> {
        Some(Arc::from(vec![1u8]))
    }

    #[test]
    fn a_finished_fetch_is_returned_once() {
        let mut tracker = FetchTracker::default();
        tracker.started(FetchId(1));
        tracker.finished(FetchId(1), bytes());
        assert_eq!(tracker.take().len(), 1);
        assert!(tracker.take().is_empty());
    }

    #[test]
    fn a_fetch_cancelled_while_running_is_discarded_when_it_finishes() {
        let mut tracker = FetchTracker::default();
        tracker.started(FetchId(1));
        assert!(tracker.cancel(FetchId(1)), "still running");
        tracker.finished(FetchId(1), bytes());
        assert!(tracker.take().is_empty());
    }

    // The case the web host got wrong: the result was already waiting for the
    // next poll when the fetch was cancelled.
    #[test]
    fn a_fetch_cancelled_after_it_finished_is_never_returned() {
        let mut tracker = FetchTracker::default();
        tracker.started(FetchId(1));
        tracker.started(FetchId(2));
        tracker.finished(FetchId(1), bytes());
        tracker.finished(FetchId(2), bytes());

        assert!(!tracker.cancel(FetchId(1)), "already finished");

        let ids: Vec<_> = tracker.take().into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, [FetchId(2)]);
    }

    // Nothing is kept for a cancelled fetch once it can't matter any more.
    #[test]
    fn cancelling_keeps_nothing_behind() {
        let mut tracker = FetchTracker::default();
        tracker.started(FetchId(1));
        tracker.cancel(FetchId(1));
        tracker.finished(FetchId(1), None);
        tracker.cancel(FetchId(2)); // never started
        assert!(tracker.running.is_empty());
        assert!(tracker.cancelled.is_empty());
    }
}
