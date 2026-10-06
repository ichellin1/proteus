//! A reference video player for the native host, showing how to bring your
//! own. It decodes an MP4 file by running `ffmpeg` on a background thread, and
//! delivers RGBA frames through a [`proteus_runtime::VideoStream`].
//!
//! Proteus doesn't play video. It shows the frames a player hands it, and
//! knows nothing about MP4 or `ffmpeg`. Any other player, such as a hardware
//! decoder, plugs in the same way: start it in
//! [`HostServices::open_video`], and return its frames as a [`VideoStream`].
//!
//! `ffmpeg` does all the decoding and plays at the file's own frame rate
//! (`-re`). It and `ffprobe` must be on `PATH`.
//!
//! Audio is not decoded: this player shows video only.
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

/// The size of an MP4 file's video, in pixels.
#[derive(Copy, Clone, Debug)]
struct VideoDimensions {
    width: u32,
    height: u32,
}

/// Reads the size of `path`'s video with `ffprobe`, without decoding any
/// frames.
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

/// An MP4 file being decoded, returned by [`open`].
pub struct Mp4Stream {
    stop_flag: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    join_handle: Option<JoinHandle<()>>,
    // An `Option` only so that `stop` can drop the receiver before joining the
    // decode thread; see `stop`.
    rx: Option<Receiver<Vec<u8>>>,
    width: u32,
    height: u32,
}

impl VideoStream for Mp4Stream {
    fn poll_frame(&mut self) -> Option<VideoFrame> {
        // Keep only the newest frame: if the render loop fell behind, older
        // frames are dropped.
        let mut latest: Option<Vec<u8>> = None;
        let mut drained = 0u32;
        let rx = self.rx.as_ref()?;
        while let Ok(frame) = rx.try_recv() {
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

    /// Kills `ffmpeg` and waits for the decode thread to exit.
    ///
    /// The receiver is dropped first. The frame channel holds two frames, so
    /// the decode thread is usually waiting in `send`, and killing `ffmpeg`
    /// doesn't wake it: only dropping the receiver does. Joining first would
    /// block the calling thread, the render thread, forever.
    fn stop(mut self: Box<Self>) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
        }
        // Before the join; see this method's doc.
        drop(self.rx.take());
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

/// Starts decoding `path` on a background thread. Playback loops until
/// [`VideoStream::stop`] is called. Returns `None`, and logs why, if `ffprobe`
/// can't read the file.
pub fn open(path: PathBuf) -> Option<Mp4Stream> {
    let dims = match probe(&path) {
        Ok(dims) => dims,
        Err(e) => {
            log::warn!("mp4_player: {path:?}: {e}");
            return None;
        }
    };

    // Two frames: one frame of lookahead. `send` blocks when the decoder is
    // ahead, which paces it.
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
        rx: Some(rx),
        width,
        height,
    })
}

/// Plays `path` from the start each time `ffmpeg` reaches the end, until `stop`
/// is set or an error occurs, which is logged.
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

/// Decodes `path` once, from start to end, sending a frame for every
/// `width * height * 4` bytes `ffmpeg` writes. Stops early if `stop` is set or
/// the receiver is dropped.
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
    // Read stderr on its own thread, or ffmpeg blocks when the pipe fills. It
    // is only logged if decoding fails.
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

    // Wait for the process, which may already have exited, so it doesn't
    // linger as a zombie.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    // `stop` must return even when the decode thread is waiting in `send`,
    // which is usual whenever the app stops polling for a moment. Uses a
    // stand-in producer, so it needs no `ffmpeg`. Checked with a timeout,
    // because the failure is a hang rather than an error.
    #[test]
    fn stop_returns_even_when_the_decode_thread_is_blocked_on_a_full_channel() {
        let (tx, rx) = sync_channel::<Vec<u8>>(2);
        let stop_flag = Arc::new(AtomicBool::new(false));

        // Stands in for the decode thread: sends frames as fast as it can and
        // exits when the receiver is dropped. With nothing reading, it is soon
        // waiting in `send`.
        let join_handle = std::thread::spawn(move || while tx.send(vec![0u8; 16]).is_ok() {});

        // Let it fill the channel and block.
        std::thread::sleep(Duration::from_millis(50));

        let stream = Mp4Stream {
            stop_flag,
            child: Arc::new(Mutex::new(None)), // no real ffmpeg child in this test
            join_handle: Some(join_handle),
            rx: Some(rx),
            width: 4,
            height: 4,
        };

        // `stop` blocks on the join, so run it off-thread and watch for it to
        // finish rather than hanging the test runner if it regresses.
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            Box::new(stream).stop();
            let _ = done_tx.send(());
        });

        match done_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => panic!(
                "Mp4Stream::stop deadlocked: the decode thread was blocked in `send`, and \
                 joining it without first dropping the receiver never returns"
            ),
            Err(RecvTimeoutError::Disconnected) => panic!("stop() panicked"),
        }
    }
}
