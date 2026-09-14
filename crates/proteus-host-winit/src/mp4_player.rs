//! Reference "bring your own player" example: decodes an `.mp4` file by
//! shelling out to `ffmpeg`/`ffprobe` on a background thread, delivering
//! RGBA frames through a [`proteus_runtime::VideoStream`] impl (M13.4 step
//! 4a — moved here from `proteus-shell-native`'s own former copy of this
//! file now that video is a real `HostServices` seam instead of a
//! shell-side shim).
//!
//! `proteus-render`/`proteus-ui`/`proteus-runtime` know nothing about MP4,
//! ffmpeg, or any codec — they only see [`VideoStream`], a small trait with
//! one non-blocking `poll_frame`. Swapping in a different decoder or a
//! hardware path means writing a different [`HostServices::open_video`]
//! implementation with this same shape — spawn a thread (or task), decode,
//! deliver frames — not touching the framework.
//!
//! Decoding, container demuxing, B-frame reordering, and real-time pacing
//! are all delegated to `ffmpeg` itself (`-re` reads the input at its native
//! frame rate) rather than reimplemented here — it's a known-correct,
//! extremely battle-tested decoder, which sidesteps an entire class of bugs
//! (reordering edge cases, chroma conversion, GOP-boundary quirks) a
//! from-scratch decoder would need to get right. Requires `ffmpeg` and
//! `ffprobe` on `PATH`.
//!
//! Audio is not decoded; this is video-only playback, matching what the
//! reference demo's video screen needs.
//!
//! [`HostServices::open_video`]: proteus_runtime::HostServices::open_video

use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use proteus_runtime::{VideoFrame, VideoStream};

/// Coded dimensions of an mp4 file's video stream.
#[derive(Copy, Clone, Debug)]
struct VideoDimensions {
    width: u32,
    height: u32,
}

/// Reads `path`'s video stream dimensions via `ffprobe` — container metadata
/// only, no frame data decoded.
fn probe(path: &Path) -> Result<VideoDimensions, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|e| format!("run ffprobe: {e} (is ffmpeg/ffprobe installed and on PATH?)"))?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe {path:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fields = stdout.trim().split(',');
    let width: u32 = fields
        .next()
        .ok_or("ffprobe: no width in output")?
        .parse()
        .map_err(|e| format!("ffprobe: bad width: {e}"))?;
    let height: u32 = fields
        .next()
        .ok_or("ffprobe: no height in output")?
        .parse()
        .map_err(|e| format!("ffprobe: bad height: {e}"))?;
    log::info!("mp4_player: probed {path:?} — {width}×{height} px (via ffprobe)");
    Ok(VideoDimensions { width, height })
}

/// A running `.mp4` decode — the [`VideoStream`] this module hands back
/// from [`open`].
pub struct Mp4Stream {
    stop_flag: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    join_handle: Option<JoinHandle<()>>,
    rx: Receiver<Vec<u8>>,
    width: u32,
    height: u32,
}

impl VideoStream for Mp4Stream {
    fn poll_frame(&mut self) -> Option<VideoFrame> {
        // Drain to the latest frame — if more than one arrived since the
        // last call (e.g. the render loop fell behind the decoder for a
        // tick), everything but the freshest is discarded. Mirrors
        // `QuadPipeline::consume_video_frame`'s identical pre-M13.4 policy.
        let mut latest: Option<Vec<u8>> = None;
        let mut drained = 0u32;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
            drained += 1;
        }
        if drained > 1 {
            log::debug!(
                "mp4_player: {} stale frame(s) discarded (render loop behind decoder)",
                drained - 1
            );
        }
        latest.map(|rgba| VideoFrame {
            width: self.width,
            height: self.height,
            rgba: Arc::from(rgba),
        })
    }

    /// Kills the `ffmpeg` child immediately (rather than waiting for it to
    /// notice the stop flag between frames) and blocks until the decode
    /// thread exits.
    fn stop(mut self: Box<Self>) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Probe `path` and, if that succeeds, spawn a background thread decoding it
/// via `ffmpeg` — looping back to the start at end-of-file; playback only
/// stops when [`VideoStream::stop`] is called. `None` (logged) if `ffprobe`
/// fails, matching [`HostServices::open_video`](proteus_runtime::HostServices::open_video)'s
/// own "couldn't even start" convention.
pub fn open(path: PathBuf) -> Option<Mp4Stream> {
    let dims = match probe(&path) {
        Ok(dims) => dims,
        Err(e) => {
            log::warn!("mp4_player: {path:?}: {e}");
            return None;
        }
    };

    // Bounded to 2 frames: one frame of lookahead; `send` blocks when the
    // decode loop is ahead, providing natural backpressure — same shape
    // `QuadPipeline::VideoFrameSender` used before this seam existed.
    let (tx, rx) = sync_channel(2);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let thread_stop = stop_flag.clone();
    let thread_child_slot = child_slot.clone();
    let (width, height) = (dims.width, dims.height);
    let join_handle = std::thread::Builder::new()
        .name("mp4-decode".into())
        .spawn(move || decode_loop(&path, &tx, width, height, &thread_stop, &thread_child_slot))
        .expect("failed to spawn mp4 decode thread");

    Some(Mp4Stream {
        stop_flag,
        child: child_slot,
        join_handle: Some(join_handle),
        rx,
        width,
        height,
    })
}

/// Replays `path` from the start each time `ffmpeg` reaches end-of-file,
/// until `stop` is set or a hard error occurs (logged, then the thread exits).
fn decode_loop(
    path: &Path,
    tx: &SyncSender<Vec<u8>>,
    width: u32,
    height: u32,
    stop: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) {
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = decode_once(path, tx, width, height, stop, child_slot) {
            log::warn!("mp4_player: {path:?}: {e}");
            return;
        }
    }
}

/// Runs one `ffmpeg` decode pass over `path` from start to end (or until
/// `stop` is set / the receiver is dropped), sending one RGBA frame per
/// `width`×`height`×4-byte chunk read from its stdout.
fn decode_once(
    path: &Path,
    tx: &SyncSender<Vec<u8>>,
    width: u32,
    height: u32,
    stop: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) -> Result<(), String> {
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-re"]) // -re: read input at native frame rate — ffmpeg paces output for us
        .arg("-i")
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-vf"])
        .arg(format!("scale={width}:{height}"))
        .arg("-") // raw frames to stdout
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn ffmpeg: {e} (is it installed and on PATH?)"))?;

    let mut stdout = child.stdout.take().ok_or("ffmpeg: no stdout pipe")?;
    // Drain stderr on its own thread so ffmpeg never blocks trying to write
    // warnings into a pipe nobody's reading; logged (at debug) only if
    // decode_once exits abnormally, to avoid spamming a normal run.
    let stderr_thread = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });

    *child_slot.lock().unwrap() = Some(child);

    let frame_size = (width * height * 4) as usize;
    let mut buf = vec![0u8; frame_size];
    let mut frames_sent = 0u32;
    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        match stdout.read_exact(&mut buf) {
            Ok(()) => {
                frames_sent += 1;
                if tx.send(buf.clone()).is_err() {
                    break Ok(()); // receiver dropped — pipeline is gone
                }
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break Ok(()), // natural end of stream
            Err(e) => break Err(format!("ffmpeg stdout read: {e}")),
        }
    };

    // Reap the child (it may already have exited on its own at EOF) so it
    // doesn't linger as a zombie process.
    if let Some(mut child) = child_slot.lock().unwrap().take() {
        let _ = child.wait();
    }

    if result.is_ok() {
        log::info!("mp4_player: {path:?}: {frames_sent} frames sent");
    } else if let Some(handle) = stderr_thread {
        if let Ok(err_output) = handle.join() {
            if !err_output.trim().is_empty() {
                log::warn!(
                    "mp4_player: {path:?}: ffmpeg stderr:\n{}",
                    err_output.trim()
                );
            }
        }
    }
    result
}
