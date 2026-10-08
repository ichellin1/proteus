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
//! BakedImage + ImageTextureRef   where the image is in the atlas
//!         │
//!         ▼
//! drawn as the entity's background
//! ```
//!
//! Unlike text, the image fills the entity at the entity's size: an entity
//! with a different shape from its image stretches it. To show part of it
//! instead, see [`ImageCrop`] and `Handle::crop_image`.
//!
//! `QuadState::color` multiplies the image's colors, so use `Vec4::ONE` to show
//! it unchanged.
//!
//! An image larger than an atlas page ([`AtlasConfig::page_size`], 2048 pixels
//! by default), after any [`Image::max_side`], is scaled down to fit the page,
//! with a warning.
//!
//! [`AtlasConfig::page_size`]: proteus_render::AtlasConfig::page_size

use bevy_ecs::prelude::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Image component
// ---------------------------------------------------------------------------

/// A PNG or JPEG image to draw on an entity.
///
/// The renderer decodes it into the main atlas and adds a [`BakedImage`].
/// Changing `bytes` afterwards has no effect until the `BakedImage` is
/// removed; `proteus-sdk`'s `Handle::set_image` does both. An image larger than an atlas page is scaled down to fit it; see
/// the [module docs](self).
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
    /// The whole image's `uv_offset`, before any crop. [`BakedImage::crop`]
    /// always works from it, so cropping again replaces the crop.
    pub full_uv_offset: [f32; 2],
    /// The whole image's `uv_scale`, before any crop.
    pub full_uv_scale: [f32; 2],
}

impl BakedImage {
    /// A baked image showing all of its region of the atlas, uncropped.
    pub fn new(uv_offset: [f32; 2], uv_scale: [f32; 2], page: u32, pixel_size: [f32; 2]) -> Self {
        Self {
            uv_offset,
            uv_scale,
            page,
            pixel_size,
            full_uv_offset: uv_offset,
            full_uv_scale: uv_scale,
        }
    }

    /// Shows only the part of the image `crop` selects. The crop is worked
    /// out from the whole image, so a second call replaces the first rather
    /// than cropping the crop, and [`ImageCrop::None`] shows all of it again.
    pub fn crop(&mut self, crop: ImageCrop) {
        let [x, y, w, h] = crop.region(self.pixel_size[0], self.pixel_size[1]);
        self.uv_offset = [
            self.full_uv_offset[0] + x * self.full_uv_scale[0],
            self.full_uv_offset[1] + y * self.full_uv_scale[1],
        ];
        self.uv_scale = [w * self.full_uv_scale[0], h * self.full_uv_scale[1]];
    }
}

// ---------------------------------------------------------------------------
// ImageCrop
// ---------------------------------------------------------------------------

/// Which part of an image a component shows. Always measured from the whole
/// image, so changing the crop never compounds it.
///
/// Only the visible region changes: no pixels are copied and no atlas space is
/// used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageCrop {
    /// The whole image: no crop.
    None,
    /// The largest centered square. Fills a square cell, such as a grid tile,
    /// with an image of any shape.
    CenteredSquare,
    /// The largest region with this width-to-height `ratio`, placed by
    /// `anchor`: `(0.5, 0.5)` centers it, `(0.0, 0.0)` keeps the top-left
    /// corner, `(1.0, 1.0)` the bottom-right. A `ratio` that isn't positive
    /// shows the whole image.
    Aspect {
        /// Width divided by height, such as `16.0 / 9.0`.
        ratio: f32,
        /// Where the region sits within the image, each axis from `0` to `1`.
        anchor: glam::Vec2,
    },
    /// An explicit region, in fractions of the image: `x` and `y` are its
    /// top-left corner, from `0` to `1`. Clamped to the image.
    Rect {
        /// Left edge, from `0` to `1`.
        x: f32,
        /// Top edge, from `0` to `1`.
        y: f32,
        /// Width, from `0` to `1`.
        width: f32,
        /// Height, from `0` to `1`.
        height: f32,
    },
}

impl ImageCrop {
    /// The region this crop selects from a `width × height` image, as
    /// `[x, y, width, height]` in fractions of the image.
    pub fn region(&self, width: f32, height: f32) -> [f32; 4] {
        let aspect = |ratio: f32, anchor: glam::Vec2| {
            let image_ratio = width / height;
            // `is_nan` as well: a NaN ratio would fail neither comparison.
            if ratio.is_nan() || ratio <= 0.0 || image_ratio.is_nan() || image_ratio <= 0.0 {
                return [0.0, 0.0, 1.0, 1.0];
            }
            let anchor = anchor.clamp(glam::Vec2::ZERO, glam::Vec2::ONE);
            if ratio < image_ratio {
                // Narrower than the image: full height, part of the width.
                let w = ratio / image_ratio;
                [(1.0 - w) * anchor.x, 0.0, w, 1.0]
            } else {
                let h = image_ratio / ratio;
                [0.0, (1.0 - h) * anchor.y, 1.0, h]
            }
        };
        match *self {
            ImageCrop::None => [0.0, 0.0, 1.0, 1.0],
            ImageCrop::CenteredSquare => aspect(1.0, glam::Vec2::splat(0.5)),
            ImageCrop::Aspect { ratio, anchor } => aspect(ratio, anchor),
            ImageCrop::Rect {
                x,
                y,
                width: w,
                height: h,
            } => {
                let x = x.clamp(0.0, 1.0);
                let y = y.clamp(0.0, 1.0);
                [x, y, w.clamp(0.0, 1.0 - x), h.clamp(0.0, 1.0 - y)]
            }
        }
    }
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
        let baked = BakedImage::new([0.1, 0.2], [0.3, 0.4], 2, [200.0, 300.0]);
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
                BakedImage::new([0.0, 0.0], [0.1, 0.1], 0, [64.0, 64.0]),
            ))
            .id();
        assert!(world.get::<Image>(e).is_some());
        assert!(world.get::<BakedImage>(e).is_some());
    }
}
