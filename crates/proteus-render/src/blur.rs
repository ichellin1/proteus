//! M8.5 — Blur pipeline skeleton (UNSCHEDULED — not compiled).
//!
//! This file is an early planning skeleton. It is intentionally NOT included via
//! `mod blur` in `lib.rs` and therefore does not affect compilation.
//!
//! **Do not add `mod blur;` to `lib.rs` until M8.5 is scheduled.** The companion
//! shader (`shaders/blur.wgsl`) uses `@group(1) @binding(3)` which collides with
//! `video_atlas` (M9). Both bindings must be reconciled before M8.5 begins.
//!
//! When M8.5 is ready:
//! 1. Resolve the binding-3 collision (renumber blur or video).
//! 2. Implement `BlurPipeline` properly (see PLANNING.md §M8.5 for the DoD).
//! 3. Add `pub mod blur;` to `lib.rs`.
//! 4. Delete this comment block.
