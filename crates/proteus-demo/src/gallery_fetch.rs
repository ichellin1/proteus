//! Photo selection and URL-building for the reference demo's picsum.photos
//! gallery (M13.4). Moved here — and de-duplicated away from an equivalent
//! hand-written copy in `proteus-shell-web`'s pre-M13.2 `www/index.html` —
//! now that [`proteus_runtime::HostServices::fetch_async`] gives every host
//! one real async-fetch primitive to build on instead of each shell
//! reimplementing "pick a photo, build a URL" itself. `DemoApp` (`app.rs`)
//! is the only caller.
//!
//! picsum.photos, addressed by a *specific photo id*
//! (`https://picsum.photos/id/{id}/{w}/{h}`), not loremflickr.com — see
//! [`NATURE_PHOTOS`]'s own doc for why. `ureq`/browser `fetch()` differences
//! are entirely `HostServices`'s concern now; nothing here knows or cares
//! which host is asking.

use glam::Vec2;
use web_time::{SystemTime, UNIX_EPOCH};

/// A value that changes across separate fetches — including across separate
/// app launches/page loads, not just repeat visits within one running
/// session — so a fresh gallery visit doesn't land on the exact same
/// [`NATURE_PHOTOS`] offset every single time the app starts. `web_time`
/// (not `std::time`) because this crate is shared with the
/// `wasm32-unknown-unknown` target, where plain `SystemTime::now()` panics.
pub(crate) fn fresh_nonce() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

/// Hand-picked, hand-verified picsum.photos photos that are actually
/// nature/landscape (mountains, forests, water, sky, wildlife, plants) —
/// picsum's catalog is unfiltered stock photography with no keyword search
/// at all, so this list is what stands in for "nature theme". Each entry is
/// `(id, width, height)`, the photo's *real* pixel dimensions (from
/// `https://picsum.photos/id/{id}/info`) — used as-is for every fetch of
/// that photo: matching the real ratio makes every request a plain resize,
/// never a crop, so low-res and hires always show the identical framing. 72
/// photos: enough that a re-fetch rarely repeats one a user just saw,
/// without needing an actual live search.
pub(crate) const NATURE_PHOTOS: [(u32, u32, u32); 72] = [
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
/// ratios, but for some it isn't — e.g. picsum id 606 (2513×1670, real ratio
/// 1.50479) requested at a 186px cap: pinning width at exactly 186 gives
/// (186, 124), ratio 1.5 — off by 0.32%. One pixel narrower, (185, 123),
/// gives ratio 1.50407 — off by only 0.05%. That 0.32% doesn't sound like
/// much, but this same real ratio also drives the *hires* fetch's own
/// request (at a much bigger, and so much more precise, target size — see
/// `Demo::start_gallery_to_image`'s doc) — picsum's `/id/{id}/{w}/{h}`
/// endpoint center-crops each request to whatever ratio it's asked for, so
/// the low-res tile fetch (this function) and the hires fetch each land on
/// a *slightly different* crop of the same source photo whenever their two
/// independent roundings of the same real ratio disagree enough.
/// `Demo::advance_gallery_hires_overlay`'s "box never moves" fix turns that
/// disagreement into an imperceptible sub-pixel stretch *in general*, but
/// for a real photo whose ratio happens to round badly at this resolution
/// (confirmed empirically: id 606 and 798 show a visible shift the instant
/// hires swaps in; id 582 and 630, whose naive rounding already lands much
/// closer to their real ratio, don't) it's visible. This search doesn't
/// eliminate the mismatch (the hires fetch's own rounding, at its own much
/// bigger scale, still isn't pixel-exact), but shrinks the low-res side's
/// error enough that it's back under whatever threshold makes the "box
/// never moves" mitigation actually work.
const FETCH_DIMENSIONS_SEARCH_WINDOW: u32 = 6;

pub(crate) fn fetch_dimensions(aspect: Vec2, side_px: u32) -> (u32, u32) {
    let ratio = aspect.x / aspect.y;
    let mut best = (side_px.max(1), 1u32, f32::INFINITY);
    for delta in 0..=FETCH_DIMENSIONS_SEARCH_WINDOW.min(side_px.saturating_sub(1)) {
        let larger = side_px - delta;
        let (w, h) = if aspect.x >= aspect.y {
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

/// The low-res fetch URL, picsum photo id, and that photo's real aspect
/// ratio for gallery tile `idx` in a batch identified by `nonce` — see
/// `DemoApp::gallery_visit` (`app.rs`) for how `nonce` varies across visits.
pub(crate) fn tile_fetch_url(nonce: u32, idx: usize, side_px: u32) -> (String, u32, Vec2) {
    let offset = nonce.wrapping_add(idx as u32);
    let (photo_id, real_w, real_h) = NATURE_PHOTOS[(offset as usize) % NATURE_PHOTOS.len()];
    let aspect = Vec2::new(real_w as f32, real_h as f32);
    let (w, h) = fetch_dimensions(aspect, side_px);
    (
        format!("https://picsum.photos/id/{photo_id}/{w}/{h}"),
        photo_id,
        aspect,
    )
}

/// The hires fetch URL for an already-known `photo_id` (the same photo a
/// tile's low-res fetch already landed on) at `width`×`height`.
pub(crate) fn hires_fetch_url(photo_id: u32, width: u32, height: u32) -> String {
    format!("https://picsum.photos/id/{photo_id}/{width}/{height}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_error(w: u32, h: u32, aspect: Vec2) -> f32 {
        let real_ratio = aspect.x / aspect.y;
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
        let reference = Vec2::new(2517.0, 1667.0);
        let (rw, rh) = fetch_dimensions(reference, side_px);
        let ceiling = relative_error(rw, rh, reference);
        for aspect in [Vec2::new(2513.0, 1670.0), Vec2::new(4592.0, 3448.0)] {
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
        let aspect = Vec2::new(2513.0, 1670.0);
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
            Vec2::new(2513.0, 1670.0),
            Vec2::new(4592.0, 3448.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(589.0, 1600.0),
        ] {
            let (w, h) = fetch_dimensions(aspect, 186);
            assert!(w <= 186 && h <= 186, "({w}, {h}) exceeds the 186px cap");
        }
    }
}
