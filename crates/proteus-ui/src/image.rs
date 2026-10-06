//! Images: a PNG or JPEG drawn on an entity. They work like [`crate::text`],
//! with decoding in place of rasterizing:
//!
//! ```text
//! Image { bytes }                declared on an entity
//!         │  the renderer finds Image without BakedImage
//!         ▼
//! decode_image                   the image, as RGBA pixels
//!         │  added to the main atlas
//!         ▼
//! BakedImage + TextureRef        where the image is in the atlas
//!         │
//!         ▼
//! drawn as the entity's background
//! ```
//!
//! Unlike text, the image fills the entity at the entity's size: an entity
//! with a different shape from its image stretches it. To crop instead, see
//! `Handle::center_crop_to_square`.
//!
//! `QuadState::color` multiplies the image's colors, so use `Vec4::ONE` to show
//! it unchanged.

use bevy_ecs::prelude::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Image component
// ---------------------------------------------------------------------------

/// A PNG or JPEG image to draw on an entity.
///
/// The renderer decodes it into the main atlas and adds a [`BakedImage`].
/// Changing `bytes` afterwards has no effect until the `BakedImage` is
/// removed.
#[derive(Component, Clone, Debug)]
pub struct Image {
    /// The PNG or JPEG file's bytes. The format is detected from the data.
    /// Shared rather than copied.
    pub bytes: Arc<[u8]>,
    /// Scale the image down so its longer side is at most this many pixels.
    /// `None` uses the configured default. Set per image, since a small grid
    /// tile and a full-screen view of the same photo need very different
    /// sizes.
    pub max_side: Option<u32>,
}

impl Image {
    /// Creates an image from its file's bytes, such as a `Vec<u8>` or a
    /// `&[u8]`. See [`Image::with_max_side`] to limit its size.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            bytes: bytes.into(),
            max_side: None,
        }
    }

    /// Sets [`Image::max_side`].
    pub fn with_max_side(mut self, max_side: u32) -> Self {
        self.max_side = Some(max_side);
        self
    }
}

// ---------------------------------------------------------------------------
// BakedImage component
// ---------------------------------------------------------------------------

/// Where an entity's image is in the main atlas, added once it is baked. The
/// entity's background [`QuadInstance`] draws it.
///
/// [`QuadInstance`]: proteus_render::QuadInstance
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BakedImage {
    /// Texture coordinates of the image's top-left corner in the atlas.
    pub uv_offset: [f32; 2],
    /// The image's size in texture coordinates; `uv_offset + uv_scale` is its
    /// bottom-right corner.
    pub uv_scale: [f32; 2],
    /// The main-atlas page the image is on. The main atlas has several pages,
    /// so the texture coordinates alone don't locate it.
    pub page: u32,
    /// The image's size in pixels, after any scaling down. The entity isn't
    /// resized to it.
    pub pixel_size: [f32; 2],
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    #[test]
    fn image_component_stores_bytes_from_vec() {
        let img = Image::new(vec![1u8, 2, 3, 4]);
        assert_eq!(&*img.bytes, &[1, 2, 3, 4]);
    }

    #[test]
    fn image_component_stores_bytes_from_slice() {
        let img = Image::new([5u8, 6, 7].as_slice());
        assert_eq!(&*img.bytes, &[5, 6, 7]);
    }

    #[test]
    fn image_component_round_trips_through_world() {
        let mut world = World::new();
        let e = world.spawn(Image::new(vec![9u8, 8, 7])).id();
        let img = world.get::<Image>(e).unwrap();
        assert_eq!(&*img.bytes, &[9, 8, 7]);
    }

    #[test]
    fn baked_image_stores_uv_coords() {
        let baked = BakedImage {
            uv_offset: [0.1, 0.2],
            uv_scale: [0.3, 0.4],
            page: 2,
            pixel_size: [200.0, 300.0],
        };
        let mut world = World::new();
        let e = world.spawn(baked.clone()).id();
        let b = world.get::<BakedImage>(e).unwrap();
        assert_eq!(b.uv_offset, [0.1, 0.2]);
        assert_eq!(b.uv_scale, [0.3, 0.4]);
        assert_eq!(b.page, 2);
        assert_eq!(b.pixel_size, [200.0, 300.0]);
    }

    #[test]
    fn entity_can_have_both_image_and_baked_image() {
        let mut world = World::new();
        let e = world
            .spawn((
                Image::new(vec![1u8, 2, 3]),
                BakedImage {
                    uv_offset: [0.0, 0.0],
                    uv_scale: [0.1, 0.1],
                    page: 0,
                    pixel_size: [64.0, 64.0],
                },
            ))
            .id();
        assert!(world.get::<Image>(e).is_some());
        assert!(world.get::<BakedImage>(e).is_some());
    }
}
