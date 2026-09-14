//! Real HLS playback for the web host (M13.4 step 4b) — `<video>`/
//! `MediaSource`/`SourceBuffer`, with decoded frames read back via an
//! offscreen `<canvas>`. Ported from `proteus-shell-web`'s pre-M13.4
//! `www/index.html` JS implementation into a
//! [`VideoStream`](proteus_runtime::VideoStream) impl — the manifest
//! parsing, segment-fetch sequencing, and buffering logic are unchanged,
//! just moved from JS into web-sys.
//!
//! No `requestVideoFrameCallback` — it isn't in this project's pinned
//! web-sys version's stable bindings. Frames are pumped via
//! `requestAnimationFrame` instead, matching the JS reference's own
//! fallback path for browsers without that (still fairly new) API: draw the
//! video's current frame to the canvas and read it back once per animation
//! frame, rather than only when the decoder actually produced a new one.
//!
//! Every async continuation and event listener here checks a `generation`
//! counter against `Inner`'s current value before touching anything — bumped
//! by [`HlsVideoStream::stop`], it turns a stale callback (e.g. a manifest
//! fetch already in flight when `stop` was called) into a silent no-op
//! instead of a callback reaching into torn-down state. Mirrors the
//! `playbackGeneration` guard the JS reference used for the same reason.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use js_sys::Promise;
use proteus_runtime::{VideoFrame, VideoStream};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement, MediaSource,
    MediaSourceReadyState, SourceBuffer,
};

use crate::request_animation_frame;
use crate::services::fetch_bytes;

/// Shared, `Rc<RefCell<_>>`-held state — every listener and async
/// continuation below reaches it through a clone of the same `Rc`, since
/// they all outlive the single [`HlsVideoStream`] handle returned to `Frame`.
struct Inner {
    video: HtmlVideoElement,
    frame_canvas: HtmlCanvasElement,
    frame_ctx: CanvasRenderingContext2d,
    object_url: Option<String>,
    /// Bumped by `stop` — see the module doc.
    generation: u32,
    abort_controller: AbortController,
    metadata_ready: bool,
    first_segment_ready: bool,
    started: bool,
    /// Written by the `requestAnimationFrame` pump; drained by `poll_frame`.
    latest_frame: Option<VideoFrame>,
}

/// The [`VideoStream`] this module hands back from [`open`].
pub struct HlsVideoStream {
    inner: Rc<RefCell<Inner>>,
}

impl VideoStream for HlsVideoStream {
    fn poll_frame(&mut self) -> Option<VideoFrame> {
        self.inner.borrow_mut().latest_frame.take()
    }

    fn cancel_load(&mut self) {
        self.inner.borrow().abort_controller.abort();
    }

    fn stop(self: Box<Self>) {
        let mut inner = self.inner.borrow_mut();
        inner.generation = inner.generation.wrapping_add(1);
        inner.abort_controller.abort();
        let _ = inner.video.pause();
        if let Some(url) = inner.object_url.take() {
            let _ = web_sys::Url::revoke_object_url(&url);
        }
        if let Some(parent) = inner.video.parent_node() {
            let _ = parent.remove_child(&inner.video);
        }
    }
}

/// `dir`: the manifest's base directory (e.g. `"videos/hls/tiger"`, relative
/// to the page — same convention `fetch_async` URLs use). `codecs`: the
/// exact MP4 container codec string this file needs (differs per video —
/// see [`crate::services::PreloadedHostServices::open_video`]'s own doc for
/// why this isn't just part of the manifest). `None` (logged) if this
/// browser can't play `codecs` at all, or basic DOM setup fails.
pub fn open(dir: String, codecs: String) -> Option<HlsVideoStream> {
    let window = web_sys::window()?;
    let document = window.document()?;

    let mime_type = format!("video/mp4; codecs=\"{codecs}\"");
    if !MediaSource::is_type_supported(&mime_type) {
        log::error!("hls_video: unsupported mime type for {dir}: {mime_type}");
        return None;
    }

    let video: HtmlVideoElement = document.create_element("video").ok()?.dyn_into().ok()?;
    video.set_muted(true);
    video.set_loop(true);
    let _ = video.style().set_property("display", "none");
    document.body()?.append_child(&video).ok()?;

    let frame_canvas: HtmlCanvasElement =
        document.create_element("canvas").ok()?.dyn_into().ok()?;
    let frame_ctx: CanvasRenderingContext2d = frame_canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into()
        .ok()?;

    let media_source = MediaSource::new().ok()?;
    let object_url = web_sys::Url::create_object_url_with_source(&media_source).ok()?;
    video.set_src(&object_url);

    let inner = Rc::new(RefCell::new(Inner {
        video: video.clone(),
        frame_canvas,
        frame_ctx,
        object_url: Some(object_url),
        generation: 0,
        abort_controller: AbortController::new().ok()?,
        metadata_ready: false,
        first_segment_ready: false,
        started: false,
        latest_frame: None,
    }));

    wire_metadata_listeners(&inner);
    wire_source_open(&inner, &media_source, dir, mime_type);

    Some(HlsVideoStream { inner })
}

/// `loadedmetadata` (fires once) and `resize` (may fire again — Safari can
/// report `loadedmetadata` on an MSE-backed `<video>` before
/// `videoWidth`/`videoHeight` are actually known; `resize` re-fires once
/// they land) both just re-check [`maybe_start`]'s own readiness gate —
/// idempotent, so no harm re-registering interest on every call.
fn wire_metadata_listeners(inner: &Rc<RefCell<Inner>>) {
    let video = inner.borrow().video.clone();

    let on_metadata = {
        let inner = inner.clone();
        Closure::<dyn FnMut(_)>::new(move |_evt: web_sys::Event| {
            inner.borrow_mut().metadata_ready = true;
            maybe_start(&inner);
        })
    };
    video
        .add_event_listener_with_callback("loadedmetadata", on_metadata.as_ref().unchecked_ref())
        .expect("addEventListener(loadedmetadata) failed");
    on_metadata.forget();

    let on_resize = {
        let inner = inner.clone();
        Closure::<dyn FnMut(_)>::new(move |_evt: web_sys::Event| {
            maybe_start(&inner);
        })
    };
    video
        .add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref())
        .expect("addEventListener(resize) failed");
    on_resize.forget();
}

/// Starts playback and the frame pump once metadata and the first segment
/// are both ready *and* real dimensions are known (guards the same Safari
/// quirk noted in [`wire_metadata_listeners`]'s doc). Idempotent — safe to
/// call from multiple listeners without an external "have I already fired"
/// check.
fn maybe_start(inner: &Rc<RefCell<Inner>>) {
    let ready = {
        let state = inner.borrow();
        !state.started
            && state.metadata_ready
            && state.first_segment_ready
            && state.video.video_width() > 0
            && state.video.video_height() > 0
    };
    if !ready {
        return;
    }

    let generation = {
        let mut state = inner.borrow_mut();
        state.started = true;
        let (width, height) = (state.video.video_width(), state.video.video_height());
        state.frame_canvas.set_width(width);
        state.frame_canvas.set_height(height);
        state.generation
    };

    if let Err(e) = inner.borrow().video.play() {
        log::error!("hls_video: video.play() failed: {e:?}");
        return;
    }
    start_frame_pump(inner.clone(), generation);
}

/// One `requestAnimationFrame`-paced loop per playback generation — draws
/// the video's current frame to the offscreen canvas and reads it back into
/// `Inner::latest_frame`. Stops rescheduling itself (rather than being
/// explicitly cancelled) once `inner`'s generation has moved on — see the
/// module doc.
fn start_frame_pump(inner: Rc<RefCell<Inner>>, generation: u32) {
    let slot = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let slot_for_closure = slot.clone();
    *slot.borrow_mut() = Some(Closure::new(move |_ts: f64| {
        let still_running = {
            let mut state = inner.borrow_mut();
            if state.generation != generation {
                false
            } else {
                let (width, height) = (state.frame_canvas.width(), state.frame_canvas.height());
                if width > 0 && height > 0 {
                    let video = state.video.clone();
                    let drew = state
                        .frame_ctx
                        .draw_image_with_html_video_element(&video, 0.0, 0.0)
                        .is_ok();
                    if drew {
                        if let Ok(image_data) =
                            state
                                .frame_ctx
                                .get_image_data(0.0, 0.0, width as f64, height as f64)
                        {
                            state.latest_frame = Some(VideoFrame {
                                width,
                                height,
                                rgba: Arc::from(image_data.data().0),
                            });
                        }
                    }
                }
                true
            }
        };
        if still_running {
            request_animation_frame(slot_for_closure.borrow().as_ref().unwrap());
        }
    }));
    request_animation_frame(slot.borrow().as_ref().unwrap());
}

/// Wires the one-shot `sourceopen` event that kicks off the whole manifest/
/// segment fetch sequence — see the module doc for the `generation` guard
/// every step below checks before touching `inner` or `media_source`.
fn wire_source_open(
    inner: &Rc<RefCell<Inner>>,
    media_source: &MediaSource,
    dir: String,
    mime_type: String,
) {
    let inner = inner.clone();
    let media_source_for_closure = media_source.clone();
    let on_source_open = Closure::<dyn FnMut(_)>::new(move |_evt: web_sys::Event| {
        let inner = inner.clone();
        let media_source = media_source_for_closure.clone();
        let dir = dir.clone();
        let mime_type = mime_type.clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = run_source_open(inner, media_source, dir, mime_type).await {
                log::error!("hls_video: {e:?}");
            }
        });
    });
    media_source
        .add_event_listener_with_callback("sourceopen", on_source_open.as_ref().unchecked_ref())
        .expect("addEventListener(sourceopen) failed");
    on_source_open.forget();
}

async fn run_source_open(
    inner: Rc<RefCell<Inner>>,
    media_source: MediaSource,
    dir: String,
    mime_type: String,
) -> Result<(), JsValue> {
    let generation = inner.borrow().generation;
    let source_buffer = media_source.add_source_buffer(&mime_type)?;
    let signal = inner.borrow().abort_controller.signal();

    let manifest_url = format!("{dir}/stream.m3u8");
    let manifest_bytes = fetch_bytes(&manifest_url, Some(&signal)).await?;
    if inner.borrow().generation != generation {
        return Ok(());
    }
    let manifest_text = String::from_utf8_lossy(&manifest_bytes).into_owned();
    let manifest = parse_hls_manifest(&manifest_text)
        .map_err(|e| JsValue::from_str(&format!("{manifest_url}: {e}")))?;

    media_source.set_duration(manifest.duration);

    let init_url = format!("{dir}/{}", manifest.init_uri);
    let init_bytes = fetch_bytes(&init_url, Some(&signal)).await?;
    if inner.borrow().generation != generation {
        return Ok(());
    }
    append_buffer(&source_buffer, init_bytes).await?;
    if inner.borrow().generation != generation {
        return Ok(());
    }

    let first_seg_url = format!("{dir}/{}", manifest.segment_uris[0]);
    let first_seg_bytes = fetch_bytes(&first_seg_url, Some(&signal)).await?;
    if inner.borrow().generation != generation {
        return Ok(());
    }
    append_buffer(&source_buffer, first_seg_bytes).await?;
    if inner.borrow().generation != generation {
        return Ok(());
    }
    inner.borrow_mut().first_segment_ready = true;
    maybe_start(&inner);

    for seg_uri in &manifest.segment_uris[1..] {
        if inner.borrow().generation != generation {
            return Ok(());
        }
        let seg_url = format!("{dir}/{seg_uri}");
        let seg_bytes = fetch_bytes(&seg_url, Some(&signal)).await?;
        if inner.borrow().generation != generation {
            return Ok(());
        }
        append_buffer(&source_buffer, seg_bytes).await?;
    }

    if inner.borrow().generation == generation
        && media_source.ready_state() == MediaSourceReadyState::Open
    {
        media_source.end_of_stream()?;
    }
    Ok(())
}

/// Wraps `SourceBuffer::append_buffer` (which signals completion via the
/// `updateend` event, not a return value) in a `Promise`/`Future`, so the
/// segment-fetch sequence above can simply `.await` each append in turn —
/// `SourceBuffer` only accepts one `appendBuffer` call at a time anyway.
async fn append_buffer(source_buffer: &SourceBuffer, mut bytes: Vec<u8>) -> Result<(), JsValue> {
    let promise = Promise::new(&mut |resolve, _reject| {
        let on_update_end = Closure::once_into_js(move || {
            let _ = resolve.call0(&JsValue::NULL);
        });
        source_buffer.set_onupdateend(Some(on_update_end.unchecked_ref()));
    });
    source_buffer.append_buffer_with_u8_array(&mut bytes)?;
    JsFuture::from(promise).await?;
    Ok(())
}

/// One parsed `#EXT-X-MAP` init segment URI, the flat list of `#EXTINF`
/// media segment URIs, and their summed duration — see
/// [`parse_hls_manifest`].
struct HlsManifest {
    init_uri: String,
    segment_uris: Vec<String>,
    duration: f64,
}

/// Minimal single-variant HLS manifest parser — no adaptive bitrate, so
/// there's no master playlist/variant selection to handle, just one
/// `#EXT-X-MAP` init segment and a flat list of `#EXTINF` segments. Ported
/// verbatim (parsing logic, not just shape) from the JS reference's own
/// `parseHlsManifest`.
fn parse_hls_manifest(text: &str) -> Result<HlsManifest, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut init_uri = None;
    let mut duration = 0.0;
    let mut segment_uris = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(start) = rest.find("URI=\"") {
                let after = &rest[start + "URI=\"".len()..];
                if let Some(end) = after.find('"') {
                    init_uri = Some(after[..end].to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            duration += parse_leading_f64(rest);
            if let Some(next) = lines.get(i + 1) {
                if !next.starts_with('#') {
                    segment_uris.push(next.to_string());
                }
            }
        }
    }

    match init_uri {
        Some(init_uri) if !segment_uris.is_empty() => Ok(HlsManifest {
            init_uri,
            segment_uris,
            duration,
        }),
        _ => Err("HLS manifest: missing init segment or media segments".to_string()),
    }
}

/// `f64::parse` is strict (all-or-nothing); this mimics JS `parseFloat`'s
/// "parse as much of a valid number as possible from the start, ignore the
/// rest" behavior — `#EXTINF:6.006,` has a trailing comma `f64::from_str`
/// would otherwise reject outright.
fn parse_leading_f64(s: &str) -> f64 {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_uri_and_segments_in_order() {
        let manifest = "#EXTM3U\n#EXT-X-MAP:URI=\"init.m4s\"\n#EXTINF:6.006,\nseg_0.m4s\n#EXTINF:2.002,\nseg_1.m4s\n#EXT-X-ENDLIST\n";
        let parsed = parse_hls_manifest(manifest).unwrap();
        assert_eq!(parsed.init_uri, "init.m4s");
        assert_eq!(parsed.segment_uris, vec!["seg_0.m4s", "seg_1.m4s"]);
        assert!((parsed.duration - 8.008).abs() < 1e-9);
    }

    #[test]
    fn missing_init_or_segments_is_an_error() {
        assert!(parse_hls_manifest("#EXTM3U\n#EXTINF:6.006,\nseg_0.m4s\n").is_err());
        assert!(parse_hls_manifest("#EXTM3U\n#EXT-X-MAP:URI=\"init.m4s\"\n").is_err());
    }

    #[test]
    fn parse_leading_f64_stops_at_trailing_comma() {
        assert!((parse_leading_f64("6.006,") - 6.006).abs() < 1e-9);
    }
}
