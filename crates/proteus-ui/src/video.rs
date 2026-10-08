//! Video: drawing the playing video on an entity.
//!
//! An entity with [`VideoPlayer`] shows the whole video texture instead of its
//! color. `QuadState::color` still tints it; use `Vec4::ONE` for none.
//!
//! **Experimental.** There is one video at a time, and every entity with
//! `VideoPlayer` shows it. Proteus doesn't play video: the app's own player
//! decodes it, and the app uploads its frames with `proteus-sdk`'s
//! `VideoHandle`.
//!
//! ## Fading between an image and the video
//!
//! An entity with both `VideoPlayer` and [`crate::BakedImage`], such as a
//! thumbnail that starts playing in place, can fade between the two with
//! [`VideoCrossfade`]: `video_t` goes from `0.0`, the image, to `1.0`, the
//! video. The video keeps playing throughout. The app sets `video_t` each
//! frame, since only it knows which way the fade is going. Without
//! `VideoCrossfade`, the entity shows only the video.

use bevy_ecs::prelude::Component;

/// Draws the playing video on this entity. See the [module docs](self).
#[derive(Component, Clone, Debug, Default)]
pub struct VideoPlayer;

/// Fades between a [`VideoPlayer`] entity's image and its video. See the
/// [module docs](self).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct VideoCrossfade {
    /// From `0.0`, only the image, to `1.0`, only the video.
    pub video_t: f32,
}
