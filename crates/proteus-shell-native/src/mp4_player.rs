//! Reference "bring your own player" example: decodes an `.mp4` file by
//! shelling out to `ffmpeg`/`ffprobe` on a background thread, and feeds the
//! resulting RGBA frames into [`proteus_render::QuadPipeline`]'s generic
//! `VideoFrameSender` channel.
//!
//! `proteus-render`/`proteus-ui` know nothing about MP4, ffmpeg, or any
//! codec — they only see [`VideoFrameSender`], a plain `Vec<u8>` channel
//! (see M9 in PLANNING.md). This module is *one* way to produce frames for
//! that channel. Swapping in an HLS player, a different decoder, or a
//! hardware path means writing a different module with this same shape —
//! spawn a thread (or task), decode, call `sender.send(rgba)` — not
//! touching the framework.
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

use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use proteus_render::VideoFrameSender;

/// Coded dimensions of an mp4 file's video stream.
#[derive(Copy, Clone, Debug)]
pub struct VideoDimensions {
    pub width: u32,
    pub height: u32,
}

/// Reads `path`'s video stream dimensions via `ffprobe` — container metadata
/// only, no frame data decoded. Call this before `QuadPipeline::init_video`
/// so the texture is sized correctly, then pass the same dimensions to
/// [`spawn`].
pub fn probe(path: &Path) -> Result<VideoDimensions, String> {
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

/// Handle to a running decode thread.
pub struct PlaybackHandle {
    stop_flag: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    join_handle: Option<JoinHandle<()>>,
}

impl PlaybackHandle {
    /// Signal the decode thread to stop, kill the `ffmpeg` child process
    /// immediately (rather than waiting for it to notice the stop flag
    /// between frames), and block until the thread exits.
    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Decode `path`'s video stream on a background thread via `ffmpeg`, sending
/// RGBA frames to `sender` paced at the source's native frame rate (`ffmpeg
/// -re`). Loops back to the start at end-of-file — playback only stops when
/// [`PlaybackHandle::stop`] is called.
///
/// `width`/`height` must be the same [`VideoDimensions`] `probe` returned for
/// this file (i.e. whatever `QuadPipeline::init_video` was called with) —
/// `ffmpeg` is asked to scale its output to exactly this size.
pub fn spawn(path: PathBuf, sender: VideoFrameSender, width: u32, height: u32) -> PlaybackHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let child_slot: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let thread_stop = stop_flag.clone();
    let thread_child_slot = child_slot.clone();
    let join_handle = std::thread::Builder::new()
        .name("mp4-decode".into())
        .spawn(move || {
            decode_loop(
                &path,
                &sender,
                width,
                height,
                &thread_stop,
                &thread_child_slot,
            )
        })
        .expect("failed to spawn mp4 decode thread");
    PlaybackHandle {
        stop_flag,
        child: child_slot,
        join_handle: Some(join_handle),
    }
}

/// Replays `path` from the start each time `ffmpeg` reaches end-of-file,
/// until `stop` is set or a hard error occurs (logged, then the thread exits).
fn decode_loop(
    path: &Path,
    sender: &VideoFrameSender,
    width: u32,
    height: u32,
    stop: &AtomicBool,
    child_slot: &Mutex<Option<Child>>,
) {
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = decode_once(path, sender, width, height, stop, child_slot) {
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
    sender: &VideoFrameSender,
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
    let mut stderr = child.stderr.take();
    let stderr_thread = stderr.take().map(|mut s| {
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
                if !sender.send(buf.clone()) {
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
