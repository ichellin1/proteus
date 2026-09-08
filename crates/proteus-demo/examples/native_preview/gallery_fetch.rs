//! Reference "fetch photos from the internet" example: fetches a batch of
//! nature-themed images from picsum.photos, sending each `(tile_idx,
//! Result<FetchedTile, err>)` back over a plain channel as it completes —
//! the same "spawn a thread, do the slow work off the main loop, send
//! results through a channel" shape as `mp4_player`, for a one-shot batch
//! fetch instead of a continuous decode stream. Also provides
//! [`spawn_hires`], the single-image counterpart used to fetch a bigger
//! version of one already-fetched tile once it's enlarged
//! (`Demo::take_pending_gallery_hires_fetch`).
//!
//! Adapted from `proteus-shell-native/src/gallery_fetch.rs` — same curated
//! [`NATURE_PHOTOS`] list, concurrency model, [`fetch_dimensions`] proportional
//! sizing, `aspect` tracking, and `spawn_hires` shape. The low-res fetch
//! itself requests the photo at its own real (width, height) *ratio*, not a
//! forced square — `fetch_dimensions` caps whichever axis is larger at
//! `side_px` and scales the other down to match, so the fetched bytes'
//! decoded shape is never distorted at the network layer. `screens::gallery`
//! does no client-side center-crop of its own yet (a separate, later
//! fidelity item — see that module's doc), so a tile's square grid cell
//! ends up showing this proportional image *stretched* to fill it; the
//! `Handle::copy_baked_image_from` a click uses is this exact same baked
//! image, undistorted, contain-fit into the enlarged view's own
//! correctly-proportioned box (`gallery::large_image_quad`) — matching
//! shapes is what actually matters there, not whether the grid cell itself
//! stretches. Once M12.5's cutover (Step 9) actually wires
//! `proteus-shell-native` to this crate, one of these two copies goes away
//! — same eventual fate as `mp4_player.rs`, see that module's doc.
//!
//! `ureq` is blocking/synchronous, which is exactly what plain
//! `std::thread`s want — this harness has no async runtime otherwise, so
//! reaching for one just for this would be a much bigger dependency than
//! the feature needs.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};

/// A successfully fetched tile: the raw (still encoded, e.g. JPEG) bytes —
/// at the photo's own real proportions, see `fetch_dimensions` — the
/// picsum photo id used to fetch it (reused by `spawn_hires` to fetch the
/// *same* photo bigger once the tile is enlarged, rather than a different
/// random one), and that photo's real (width, height) ratio (redundant
/// with `bytes`' own decoded shape once baked, but available immediately,
/// before baking happens).
pub struct FetchedTile {
    pub bytes: Vec<u8>,
    pub photo_id: u32,
    pub aspect: (f32, f32),
}

/// One fetch outcome: which tile it's for, and either the fetched tile or
/// an error description.
pub type FetchResult = (usize, Result<FetchedTile, String>);

/// Hand-picked, hand-verified picsum.photos photos that are actually
/// nature/landscape (mountains, forests, water, sky, wildlife, plants) —
/// picsum's catalog is unfiltered stock photography with no keyword search
/// at all, so this list is what stands in for "nature theme". Each entry is
/// `(id, width, height)`, the photo's *real* pixel dimensions — copied
/// verbatim from `proteus-shell-native::gallery_fetch::NATURE_PHOTOS`.
const NATURE_PHOTOS: [(u32, u32, u32); 72] = [
    (12, 2500, 1667),
    (18, 2500, 1667),
    (54, 3264, 2176),
    (66, 3264, 2448),
    (108, 2000, 1333),
    (114, 3264, 2448),
    (132, 1600, 1066),
    (162, 1500, 998),
    (168, 1920, 1280),
    (174, 1600, 589),
    (198, 3456, 2304),
    (216, 2500, 1667),
    (222, 1800, 977),
    (228, 4608, 3456),
    (282, 5000, 3333),
    (294, 3753, 2309),
    (300, 4272, 2848),
    (324, 3888, 2592),
    (330, 4272, 2848),
    (378, 5000, 3333),
    (384, 5000, 3333),
    (426, 4272, 2848),
    (432, 5000, 3333),
    (450, 4288, 2848),
    (456, 3823, 2549),
    (468, 5000, 3337),
    (474, 4288, 2848),
    (480, 3888, 2592),
    (492, 5000, 3324),
    (498, 5000, 3333),
    (516, 3008, 2000),
    (558, 4928, 3264),
    (564, 2000, 1333),
    (570, 2509, 1673),
    (582, 2509, 1673),
    (588, 2509, 1673),
    (600, 2509, 1673),
    (606, 2513, 1670),
    (612, 2731, 1536),
    (630, 2517, 1667),
    (648, 2517, 1667),
    (654, 2509, 1673),
    (666, 5000, 3333),
    (678, 4896, 3264),
    (684, 3872, 2178),
    (702, 5000, 3333),
    (732, 5000, 3333),
    (738, 2005, 3000),
    (774, 5000, 3333),
    (780, 3264, 1830),
    (798, 4592, 3448),
    (810, 5000, 3333),
    (846, 4000, 3000),
    (852, 3212, 2409),
    (876, 5000, 3338),
    (888, 5000, 2813),
    (894, 5000, 3333),
    (906, 3840, 2880),
    (912, 5000, 3333),
    (918, 3747, 2489),
    (924, 3000, 2250),
    (930, 3264, 4912),
    (960, 3264, 1832),
    (966, 5000, 3333),
    (984, 4000, 2248),
    (990, 5000, 3334),
    (1002, 4312, 2868),
    (1020, 4288, 2848),
    (1044, 4032, 2268),
    (1050, 5000, 3333),
    (1056, 3988, 2720),
    (1074, 5000, 3333),
];

/// Spawns a coordinator thread that fans out one fetch thread per image
/// (nature-themed — see `NATURE_PHOTOS`, requested square at `side_px`),
/// each sending its `(idx, result)` back over the returned channel as soon
/// as it completes — so results can arrive in any order, not tile order.
/// Drop the receiver (e.g. the caller navigated away before the fetch
/// finished) to make each worker give up on send instead of it mattering
/// that nobody's listening.
pub fn spawn(count: usize, side_px: u32) -> Receiver<FetchResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gallery-fetch".into())
        .spawn(move || fetch_all(count, side_px, tx))
        .expect("failed to spawn gallery-fetch thread");
    rx
}

/// Spawns a single background thread fetching one photo (`photo_id`,
/// already assigned to the tile being enlarged — see
/// `main.rs::apply_gallery_hires_fetch`) at `width`×`height` instead of the
/// grid's smaller square. Drop the receiver to cancel: the same "give up on
/// send, nobody's listening" pattern `spawn` uses, since a blocking `ureq`
/// call can't be interrupted mid-flight — this doesn't stop the network
/// request, just discards its result.
pub fn spawn_hires(width: u32, height: u32, photo_id: u32) -> Receiver<Result<Vec<u8>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gallery-hires-fetch".into())
        .spawn(move || {
            let url = format!("https://picsum.photos/id/{photo_id}/{width}/{height}");
            let result = fetch_bytes(&url);
            let _ = tx.send(result);
        })
        .expect("failed to spawn gallery-hires-fetch thread");
    rx
}

/// The actual fetch dimensions for `aspect` (width, height ratio) at a
/// `side_px` cap: square is `side_px`×`side_px`; otherwise the larger ratio
/// axis is held at `side_px` and the other scaled down to match, so a
/// portrait photo is height-constrained and a landscape one
/// width-constrained. Mirrors
/// `proteus-shell-native::gallery_fetch::fetch_dimensions` exactly.
fn fetch_dimensions(aspect: (f32, f32), side_px: u32) -> (u32, u32) {
    let (aw, ah) = aspect;
    if aw >= ah {
        (side_px, (side_px as f32 * ah / aw).round() as u32)
    } else {
        ((side_px as f32 * aw / ah).round() as u32, side_px)
    }
}

fn fetch_all(count: usize, side_px: u32, tx: Sender<FetchResult>) {
    // Offsets which NATURE_PHOTOS entry this fetch's batch lands on so it
    // differs from the last one's, even though the tile-local part (`idx`)
    // repeats every fetch.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0);
    for idx in 0..count {
        let tx = tx.clone();
        std::thread::Builder::new()
            .name(format!("gallery-fetch-{idx}"))
            .spawn(move || {
                let offset = nonce.wrapping_add(idx as u32);
                let (photo_id, real_w, real_h) =
                    NATURE_PHOTOS[(offset as usize) % NATURE_PHOTOS.len()];
                let aspect = (real_w as f32, real_h as f32);
                let (fw, fh) = fetch_dimensions(aspect, side_px);
                let url = format!("https://picsum.photos/id/{photo_id}/{fw}/{fh}");
                let result = fetch_bytes(&url).map(|bytes| FetchedTile {
                    bytes,
                    photo_id,
                    aspect,
                });
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
