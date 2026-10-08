//! Plays a video in a desktop window, decoded by `ffmpeg`. In a browser,
//! `start` in `lib.rs` runs the app instead, so this is empty there.
//!
//! `cargo run -p video` plays the reference demo's tiger video;
//! `cargo run -p video -- path/to/video.mp4` plays another.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../crates/proteus-shell-native/assets/videos/tiger.mp4"
        )
        .to_string()
    });
    let player = match video::ffmpeg::FfmpegPlayer::open(&path) {
        Ok(player) => player,
        Err(e) => {
            eprintln!("can't play {path}: {e}");
            std::process::exit(1);
        }
    };
    proteus_host_winit::run(
        video::VideoApp::new(player),
        proteus_host_winit::RunConfig {
            title: "Video".to_string(),
            initial_size: (1100, 720),
            proteus: video::config(),
            ..Default::default()
        },
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}
