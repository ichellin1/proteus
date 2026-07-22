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
//!
//! ## Live crossfade into/out of a base image (M9.8)
//!
//! An entity that carries *both* [`VideoPlayer`] and [`crate::BakedImage`] —
//! e.g. a component that permanently shows a poster/thumbnail and, once
//! activated, plays video in the same spot — can crossfade smoothly between
//! the two instead of cutting instantly. Attach [`VideoCrossfade`] and set
//! `video_t` each frame: `0.0` shows the base image fully, `1.0` shows video
//! fully, anything between blends live — the video keeps streaming and
//! updating throughout, never frozen into a static snapshot (unlike the
//! baked-slice crossfade transitions use).
//!
//! `collect_instances` reads `video_t` — it doesn't decide *when* to move it;
//! only the caller knows whether a transition is running forward or in
//! reverse, so the caller (not the framework) owns the ramp. Absent
//! `VideoCrossfade`, an entity with `VideoPlayer` shows video at full opacity
//! with no blending, same as before this component existed.

use bevy_ecs::prelude::Component;

/// Marker component: render this entity from the pipeline's streaming video
/// texture (`video_atlas`, `atlas_page = 2`) instead of the solid-colour white
/// pixel in `main_atlas`.
///
/// See the [module documentation](self) for setup instructions.
#[derive(Component, Clone, Debug, Default)]
pub struct VideoPlayer;

/// Blends a [`VideoPlayer`] entity's live video against its [`crate::BakedImage`]
/// base — see the [module documentation](self) for the full picture.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct VideoCrossfade {
    /// `0.0` = fully the base image, `1.0` = fully video.
    pub video_t: f32,
}
