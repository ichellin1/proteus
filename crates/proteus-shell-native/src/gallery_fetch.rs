//! Reference "fetch photos from the internet" example: fetches a batch of
//! nature-themed images from picsum.photos, sending each `(tile_idx,
//! Result<FetchedTile, err>)` back over a plain channel as it completes —
//! the same "spawn a thread, do the slow work off the main loop, send
//! results through a channel" shape as [`crate::mp4_player`], for a
//! one-shot batch fetch instead of a continuous decode stream. Also
//! provides [`spawn_hires`], the single-image counterpart used to fetch a
//! bigger version of one already-fetched tile once it's enlarged (see
//! `start_gallery_to_image` in `main.rs`).
//!
//! picsum.photos, addressed by a *specific photo id*
//! (`https://picsum.photos/id/{id}/{w}/{h}`), not loremflickr.com
//! (used before this feature) — loremflickr has no stable notion of "the
//! same photo, bigger": its crop for a given keyword+lock genuinely shifts
//! with the requested pixel size, confirmed empirically. picsum's id-based
//! endpoint is a plain crop of one fixed photo, but a *requested* aspect
//! ratio that doesn't match the photo's own still means picsum has to
//! decide what to crop away — [`NATURE_PHOTOS`] sidesteps that decision
//! entirely by pairing each curated id with its own real (width, height)
//! from picsum's `/id/{id}/info` endpoint, so every fetch (low-res *and*
//! hires) requests exactly that photo's native ratio: a plain resize, never
//! a crop, which is trivially consistent at any output size. picsum has no
//! keyword search at all, so "nature" is enforced by this hand-picked,
//! hand-verified list rather than a live query.
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

/// A successfully fetched tile: the raw (still encoded, e.g. JPEG) bytes,
/// the picsum photo id used to fetch it (reused later to fetch the same
/// photo bigger — see `spawn_hires`), and that photo's real aspect ratio
/// (see `NATURE_PHOTOS`).
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
/// `(id, width, height)`, the photo's *real* pixel dimensions (from
/// `https://picsum.photos/id/{id}/info`) — used as-is for every fetch of
/// that photo (see the module doc for why: matching the real ratio makes
/// every request a plain resize, never a crop, so low-res and hires always
/// show the identical framing). 72 photos: enough that a re-fetch rarely
/// repeats one a user just saw, without needing an actual live search.
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

/// The actual fetch dimensions for `aspect` (width, height ratio) at a
/// `side_px` cap: square is `side_px`×`side_px`; otherwise the larger ratio
/// axis is held at (or just under) `side_px` and the other scaled to match,
/// so a portrait photo is height-constrained and a landscape one
/// width-constrained.
///
/// Doesn't just pin the larger axis at exactly `side_px` and round the
/// other — searches a small window of slightly-smaller candidates for the
/// larger axis too (`FETCH_DIMENSIONS_SEARCH_WINDOW`), picking whichever
/// integer pair's ratio comes closest to the real one. Pinning the larger
/// axis exactly is the *best* approximation for most real photo aspect
/// ratios, but for some it isn't — e.g. picsum id 606 (2513×1670,
/// real ratio 1.50479) requested at a 186px cap: pinning width at exactly
/// 186 gives (186, 124), ratio 1.5 — off by 0.32%. One pixel narrower,
/// (185, 123), gives ratio 1.50407 — off by only 0.05%. That 0.32% doesn't
/// sound like much, but this same real ratio also drives the *hires*
/// fetch's own request (at a much bigger, and so much more precise, target
/// size — see `Demo::start_gallery_to_image`'s doc) — picsum's `/id/{id}/
/// {w}/{h}` endpoint center-crops each request to whatever ratio it's
/// asked for, so the low-res tile fetch (this function) and the hires
/// fetch each land on a *slightly different* crop of the same source photo
/// whenever their two independent roundings of the same real ratio
/// disagree enough. `Demo::advance_gallery_hires_overlay`'s "box never
/// moves" fix turns that disagreement into an imperceptible sub-pixel
/// stretch *in general*, but for a real photo whose ratio happens to round
/// badly at this resolution (confirmed empirically: id 606 and 798 show a
/// visible shift the instant hires swaps in; id 582 and 630, whose naive
/// rounding already lands much closer to their real ratio, don't) it's
/// visible. This search doesn't eliminate the mismatch (the hires fetch's
/// own rounding, at its own much bigger scale, still isn't pixel-exact),
/// but shrinks the low-res side's error enough that it's back under
/// whatever threshold makes the "box never moves" mitigation actually
/// work.
const FETCH_DIMENSIONS_SEARCH_WINDOW: u32 = 6;

fn fetch_dimensions(aspect: (f32, f32), side_px: u32) -> (u32, u32) {
    let (aw, ah) = aspect;
    let ratio = aw / ah;
    let mut best = (side_px.max(1), 1u32, f32::INFINITY);
    for delta in 0..=FETCH_DIMENSIONS_SEARCH_WINDOW.min(side_px.saturating_sub(1)) {
        let larger = side_px - delta;
        let (w, h) = if aw >= ah {
            (larger, ((larger as f32) / ratio).round().max(1.0) as u32)
        } else {
            (((larger as f32) * ratio).round().max(1.0) as u32, larger)
        };
        let error = (w as f32 / h as f32 - ratio).abs();
        if error < best.2 {
            best = (w, h, error);
        }
    }
    (best.0, best.1)
}

/// Spawns a coordinator thread that fans out one fetch thread per image
/// (nature-themed — see `NATURE_PHOTOS`), each sending its `(idx, result)`
/// back over the returned channel as soon as it completes — so results can
/// arrive in any order, not tile order. Drop the receiver (e.g. the caller
/// navigated away before the fetch finished) to make each worker give up on
/// send instead of it mattering that nobody's listening — see the
/// `tx.send(..)` call below.
pub fn spawn(count: usize, side_px: u32) -> Receiver<FetchResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("gallery-fetch".into())
        .spawn(move || fetch_all(count, side_px, tx))
        .expect("failed to spawn gallery-fetch thread");
    rx
}

/// Spawns a single background thread fetching one photo (`photo_id`,
/// already assigned to the tile being enlarged) at `width`×`height` instead
/// of the grid's square crop — the "fetch the hires version" side of
/// `start_gallery_to_image`. Drop the receiver to cancel: the same
/// "give up on send, nobody's listening" pattern `spawn` uses, since a
/// blocking `ureq` call can't be interrupted mid-flight — this doesn't stop
/// the network request, just discards its result.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_error(w: u32, h: u32, aspect: (f32, f32)) -> f32 {
        let real_ratio = aspect.0 / aspect.1;
        (w as f32 / h as f32 - real_ratio).abs() / real_ratio
    }

    /// Reported regression: at the gallery grid's ~186px tile cap, photo
    /// ids 606 (2513×1670) and 798 (4592×3448) visibly shifted content the
    /// instant their hires fetch swapped in over the low-res stand-in;
    /// ids 582 (2509×1673) and 630 (2517×1667) — whose *naive* single-axis
    /// rounding happens to already land close to the real ratio at this
    /// resolution — never showed it. The fix doesn't need to change 582/630
    /// at all; it just needs to pull 606/798 down under roughly the same
    /// relative-error ceiling those two were already living under.
    #[test]
    fn fetch_dimensions_keeps_previously_shifting_photos_as_accurate_as_previously_fine_ones() {
        let side_px = 186;
        let ceiling = relative_error(
            fetch_dimensions((2517.0, 1667.0), side_px).0,
            fetch_dimensions((2517.0, 1667.0), side_px).1,
            (2517.0, 1667.0),
        );
        for aspect in [(2513.0, 1670.0), (4592.0, 3448.0)] {
            let (w, h) = fetch_dimensions(aspect, side_px);
            let err = relative_error(w, h, aspect);
            assert!(
                err <= ceiling,
                "{aspect:?} at {side_px}px: relative error {err} exceeds the \
                 previously-fine-photos' ceiling {ceiling} — ({w}, {h})"
            );
        }
    }

    /// The naive (no-search) approximation for id 606 landed on a ratio
    /// exactly 1.5 (186×124) — 0.32% off its real 1.50479. Confirms the
    /// search actually finds a closer pair, not just an equally-bad one.
    #[test]
    fn fetch_dimensions_finds_a_closer_ratio_than_pinning_the_larger_axis_exactly() {
        let aspect = (2513.0, 1670.0);
        let (w, h) = fetch_dimensions(aspect, 186);
        assert!(
            (w, h) != (186, 124),
            "expected the search to move off the naive (186, 124) pair"
        );
        assert!(relative_error(w, h, aspect) < 0.001);
    }

    #[test]
    fn fetch_dimensions_never_exceeds_the_side_px_cap() {
        for aspect in [
            (2513.0, 1670.0),
            (4592.0, 3448.0),
            (1.0, 1.0),
            (589.0, 1600.0),
        ] {
            let (w, h) = fetch_dimensions(aspect, 186);
            assert!(w <= 186 && h <= 186, "({w}, {h}) exceeds the 186px cap");
        }
    }
}
