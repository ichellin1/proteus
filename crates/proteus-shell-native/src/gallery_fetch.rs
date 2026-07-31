//! Reference "fetch photos from the internet" example: fetches a batch of
//! square images from picsum.photos on a background thread, sending each
//! `(tile_idx, Result<bytes, err>)` back over a plain channel as it
//! completes — the same "spawn a thread, do the slow work off the main
//! loop, send results through a channel" shape as [`crate::mp4_player`], for
//! a one-shot batch fetch instead of a continuous decode stream.
//!
//! `ureq` is blocking/synchronous, which is exactly what a plain
//! `std::thread` wants — this app has no async runtime at all otherwise, so
//! reaching for one just for this would be a much bigger dependency than
//! the feature needs.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};

/// One fetch outcome: which tile it's for, and either the raw (still
/// encoded, e.g. JPEG) image bytes or an error description.
pub type FetchResult = (usize, Result<Vec<u8>, String>);

#[derive(serde::Deserialize)]
struct PicsumEntry {
    id: String,
}

/// Spawns a background thread that fetches `count` square images at
/// `side_px`×`side_px` from picsum.photos, sending `(idx, result)` back over
/// the returned channel as each completes, in tile order. Drop the receiver
/// (e.g. the caller navigated away before the fetch finished) to make the
/// thread give up early instead of continuing to fetch images nobody will
/// see — see the `tx.send(..).is_err()` check below.
pub fn spawn(count: usize, side_px: u32) -> Receiver<FetchResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gallery-fetch".into())
        .spawn(move || fetch_all(count, side_px, &tx))
        .expect("failed to spawn gallery-fetch thread");
    rx
}

fn fetch_all(count: usize, side_px: u32, tx: &Sender<FetchResult>) {
    let ids = fetch_ids(count).unwrap_or_else(|e| {
        log::warn!("gallery_fetch: list endpoint unavailable ({e}), falling back to seed URLs");
        (0..count).map(|i| format!("seed-{i}")).collect()
    });
    for (idx, id) in ids.into_iter().enumerate().take(count) {
        let url = match id.strip_prefix("seed-") {
            Some(seed) => format!("https://picsum.photos/seed/{seed}/{side_px}/{side_px}"),
            None => format!("https://picsum.photos/id/{id}/{side_px}/{side_px}"),
        };
        let result = fetch_bytes(&url);
        if tx.send((idx, result)).is_err() {
            return;
        }
    }
}

/// Fetches picsum's list endpoint and returns the first `count` image ids.
/// `Err` (network failure, bad JSON, or fewer than `count` entries) tells
/// the caller to fall back to seed-based URLs instead, which need no list
/// lookup at all.
fn fetch_ids(count: usize) -> Result<Vec<String>, String> {
    let resp = ureq::get("https://picsum.photos/v2/list")
        .query("page", "1")
        .query("limit", &count.to_string())
        .call()
        .map_err(|e| e.to_string())?;
    let entries: Vec<PicsumEntry> = resp.into_json().map_err(|e| e.to_string())?;
    if entries.len() < count {
        return Err(format!("list returned only {} entries", entries.len()));
    }
    Ok(entries.into_iter().take(count).map(|e| e.id).collect())
}

fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
