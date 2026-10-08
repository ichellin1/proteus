//! A player for the desktop that runs the `ffmpeg` command to decode the
//! video. `ffmpeg` and `ffprobe` must be installed and on `PATH`.
//!
//! `ffmpeg` decodes on a background thread, as fast as it can, into a queue
//! that holds two frames; when the queue is full, it waits. The app takes
//! frames from the queue at the video's frame rate in [`Player::next_frame`],
//! so pausing is just not taking any, and resuming carries on from the same
//! frame. The video loops.
//!
//! Audio isn't played.

use std::io::{ErrorKind, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError};

use crate::{Picture, Player};

/// Plays a video file with `ffmpeg`.
pub struct FfmpegPlayer {
    frames: Receiver<Vec<u8>>,
    width: u32,
    height: u32,
    /// Seconds per frame.
    frame_time: f32,
    /// Playback time not yet used by a frame.
    clock: f32,
    paused: bool,
}

impl FfmpegPlayer {
    /// Starts decoding the video at `path`.
    ///
    /// # Errors
    ///
    /// If `ffprobe` can't read the video's size and frame rate, such as when
    /// the file doesn't exist or `ffmpeg` isn't installed.
    pub fn open(path: &str) -> Result<Self, String> {
        let (width, height, frame_rate) = probe(path)?;
        let (sender, frames) = sync_channel(2);
        let path = path.to_string();
        std::thread::spawn(move || decode(&path, width, height, &sender));
        Ok(Self::from_frames(frames, width, height, frame_rate))
    }

    fn from_frames(frames: Receiver<Vec<u8>>, width: u32, height: u32, frame_rate: f32) -> Self {
        Self {
            frames,
            width,
            height,
            frame_time: 1.0 / frame_rate,
            clock: 0.0,
            paused: false,
        }
    }
}

impl Player for FfmpegPlayer {
    fn next_frame(&mut self, dt: f32) -> Option<Picture> {
        if self.paused {
            return None;
        }
        self.clock += dt;
        // Take a frame for each frame time that has passed, and show the last.
        let mut newest = None;
        while self.clock >= self.frame_time {
            match self.frames.try_recv() {
                Ok(rgba) => {
                    newest = Some(rgba);
                    self.clock -= self.frame_time;
                }
                // `ffmpeg` is behind. Don't let time build up, or the video
                // would rush to catch up when it can.
                Err(TryRecvError::Empty) => {
                    self.clock = self.clock.min(self.frame_time);
                    break;
                }
                Err(TryRecvError::Disconnected) => break,
            }
        }
        newest.map(|rgba| Picture {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }
}

/// Reads the video's size and frame rate with `ffprobe`.
fn probe(path: &str) -> Result<(u32, u32, f32), String> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height,r_frame_rate"])
        .args(["-of", "csv=p=0", path])
        .output()
        .map_err(|e| format!("can't run ffprobe ({e}); is ffmpeg installed?"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

/// Parses `ffprobe`'s `width,height,rate` line, where the rate is a fraction
/// such as `24000/1001`.
fn parse_probe(line: &str) -> Result<(u32, u32, f32), String> {
    let bad = || format!("unexpected ffprobe output: {line:?}");
    let mut fields = line.trim().split(',');
    let mut number = || fields.next().ok_or_else(bad);
    let width = number()?.parse().map_err(|_| bad())?;
    let height = number()?.parse().map_err(|_| bad())?;
    let (top, bottom) = number()?.split_once('/').ok_or_else(bad)?;
    let rate = top.parse::<f32>().map_err(|_| bad())? / bottom.parse::<f32>().map_err(|_| bad())?;
    if rate > 0.0 && rate.is_finite() {
        Ok((width, height, rate))
    } else {
        Err(bad())
    }
}

/// Decodes the video over and over, sending each frame, until the player is
/// dropped. Runs on its own thread.
///
/// When the player is dropped, its end of the queue goes with it, the next
/// `send` fails, and this stops `ffmpeg` and returns.
fn decode(path: &str, width: u32, height: u32, sender: &SyncSender<Vec<u8>>) {
    loop {
        let child = Command::new("ffmpeg")
            .args(["-v", "error", "-i", path])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                log::warn!("can't run ffmpeg: {e}");
                return;
            }
        };
        let Some(mut stdout) = child.stdout.take() else {
            return;
        };
        let mut frame = vec![0u8; (width * height * 4) as usize];
        loop {
            match stdout.read_exact(&mut frame) {
                Ok(()) => {
                    if sender.send(frame.clone()).is_err() {
                        // The player is gone.
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
                // The end of the video: play it again.
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    log::warn!("reading from ffmpeg: {e}");
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_size_and_a_fractional_frame_rate() {
        let (width, height, rate) = parse_probe("1280,720,24000/1001\n").unwrap();
        assert_eq!((width, height), (1280, 720));
        assert!((rate - 23.976).abs() < 0.001);
        assert!(parse_probe("1280,720,0/0").is_err());
        assert!(parse_probe("garbage").is_err());
    }

    /// A player at `rate` frames a second, with `queued` frames waiting, each
    /// filled with its own number.
    fn player(rate: f32, queued: u8) -> (FfmpegPlayer, SyncSender<Vec<u8>>) {
        let (sender, frames) = sync_channel(64);
        for i in 0..queued {
            sender.send(vec![i]).unwrap();
        }
        (FfmpegPlayer::from_frames(frames, 1, 1, rate), sender)
    }

    // At 10 frames a second and 60 app frames a second, a new frame is shown
    // every sixth app frame.
    #[test]
    fn frames_are_taken_at_the_videos_frame_rate() {
        let (mut player, _sender) = player(10.0, 20);
        let shown: Vec<u8> = (0..60)
            .filter_map(|_| player.next_frame(1.0 / 60.0))
            .map(|picture| picture.rgba[0])
            .collect();
        assert_eq!(shown, (0..10).collect::<Vec<u8>>());
    }

    #[test]
    fn a_paused_player_takes_no_frames_and_resumes_where_it_was() {
        let (mut player, _sender) = player(10.0, 20);
        assert_eq!(player.next_frame(0.1).unwrap().rgba, [0]);
        player.toggle_pause();
        assert!(player.next_frame(5.0).is_none());
        player.toggle_pause();
        assert_eq!(player.next_frame(0.1).unwrap().rgba, [1]);
    }

    // When `ffmpeg` falls behind, the time it was behind isn't made up later
    // by rushing through frames.
    #[test]
    fn time_spent_waiting_for_ffmpeg_doesnt_build_up() {
        let (mut player, sender) = player(10.0, 0);
        assert!(player.next_frame(2.0).is_none());
        for i in 0..5 {
            sender.send(vec![i]).unwrap();
        }
        assert_eq!(player.next_frame(0.0).unwrap().rgba, [0]);
        assert!(player.next_frame(0.0).is_none());
    }
}
