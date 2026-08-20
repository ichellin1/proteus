//! Per-screen entity spawning. Each module owns one screen's layout and
//! returns a small struct of `Handle`s the state machine (`lib.rs`) uses to
//! wire up transitions between screens.

pub mod background;
pub mod home;
pub mod splash;
