//! Per-screen entity spawning. Each module owns one screen's layout and
//! returns a small struct of `Handle`s the state machine (`lib.rs`) uses to
//! wire up transitions between screens.

pub mod background;
pub mod example_detail;
pub mod examples_home;
pub mod gallery;
pub mod home;
pub mod loading;
pub mod nav;
pub mod splash;
pub mod theme;
pub mod video_tiles;
