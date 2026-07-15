//! Video playback component (M9).
//!
//! Attach [`VideoPlayer`] to any entity that has a [`crate::QuadState`] to make
//! it render from the pipeline's streaming `video_atlas` texture instead of the
//! `main_atlas` white-pixel sentinel.
//!
//! ## Rendering
//!
//! [`crate::collect_instances`] detects the `VideoPlayer` marker and:
//! - Sets `atlas_page = 2` so the shader samples from `video_atlas`
//! - Sets `uv_offset = [0, 0]` and `uv_scale = [1, 1]` so the full texture
//!   maps across the quad
//!
//! The entity's `QuadState::color` still applies as a multiplicative tint over
//! the video content.  Set `color = Vec4::ONE` (white) for unfiltered video.
//!
//! ## Limitations (M9)
//!
//! - Only one concurrent video texture is supported.  All entities with
//!   `VideoPlayer` display the same `video_atlas` (the same frame).
//! - The video texture must be initialised by calling
//!   [`proteus_render::QuadPipeline::init_video`] before any frame is displayed.

use bevy_ecs::prelude::Component;

/// Marker component: render this entity from the pipeline's streaming video
/// texture (`video_atlas`, `atlas_page = 2`) instead of the solid-colour white
/// pixel in `main_atlas`.
///
/// See the [module documentation](self) for setup instructions.
#[derive(Component, Clone, Debug, Default)]
pub struct VideoPlayer;
