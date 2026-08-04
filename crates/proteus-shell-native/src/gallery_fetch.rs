//! Reference "fetch photos from the internet" example: fetches a batch of
//! square nature-themed images from loremflickr.com, sending each
//! `(tile_idx, Result<bytes, err>)` back over a plain channel as it
//! completes — the same "spawn a thread, do the slow work off the main
//! loop, send results through a channel" shape as [`crate::mp4_player`], for
//! a one-shot batch fetch instead of a continuous decode stream.
//!
//! LoremFlickr (not picsum.photos, used before this feature) is the source
//! specifically because it supports keyword-filtered results with no API
//! key — `https://loremflickr.com/{w}/{h}/{keyword}` — where picsum has no
//! content filtering at all, just arbitrary stock photos by id/seed. Its
//! `lock` query parameter deterministically selects a specific match for a
//! given keyword+size+lock combination, which doubles as the "get 12
//! different photos, and a different 12 on the next fetch" mechanism: each
//! tile gets a distinct lock value, and every fetch's values are offset by
//! the current time so a re-fetch doesn't reproduce the same batch.
//!
//! The per-image fetches run one thread each, concurrently, rather than one
//! thread looping over all of them — with a blocking client and no
//! connection-sharing between them, a single-threaded loop pays each
//! image's full network round-trip back to back (12 images at ~300ms each
//! is ~3.6s), where the equivalent JS `fetch()`-per-image on the web shell
//! has the browser dispatch all of them at once. Concurrent threads close
//! that gap: total wall time becomes roughly the slowest single fetch, not
//! the sum of all of them.
//!
//! `ureq` is blocking/synchronous, which is exactly what plain
//! `std::thread`s want — this app has no async runtime at all otherwise, so
//! reaching for one just for this would be a much bigger dependency than
//! the feature needs.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};

/// One fetch outcome: which tile it's for, and either the raw (still
/// encoded, e.g. JPEG) image bytes or an error description.
pub type FetchResult = (usize, Result<Vec<u8>, String>);

/// Every gallery fetch is nature-themed — see the module doc for why this
/// demo doesn't (and, on picsum, couldn't) offer other categories.
const KEYWORD: &str = "nature";

/// Spawns a coordinator thread that fans out one fetch thread per image
/// (all `side_px`×`side_px`, nature-themed), each sending its `(idx,
/// result)` back over the returned channel as soon as it completes — so
/// results can arrive in any order, not tile order. Drop the receiver (e.g.
/// the caller navigated away before the fetch finished) to make each worker
/// give up on send instead of it mattering that nobody's listening — see
/// the `tx.send(..)` call below.
pub fn spawn(count: usize, side_px: u32) -> Receiver<FetchResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gallery-fetch".into())
        .spawn(move || fetch_all(count, side_px, tx))
        .expect("failed to spawn gallery-fetch thread");
    rx
}

fn fetch_all(count: usize, side_px: u32, tx: Sender<FetchResult>) {
    // Offsets every lock value so this fetch's batch differs from the last
    // one's, even though the tile-local part (`idx`) repeats every fetch.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    for idx in 0..count {
        let tx = tx.clone();
        std::thread::Builder::new()
            .name(format!("gallery-fetch-{idx}"))
            .spawn(move || {
                let lock = nonce.wrapping_add(idx as u128);
                let url =
                    format!("https://loremflickr.com/{side_px}/{side_px}/{KEYWORD}?lock={lock}");
                let result = fetch_bytes(&url);
                let _ = tx.send((idx, result));
            })
            .expect("failed to spawn gallery-fetch worker thread");
    }
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
