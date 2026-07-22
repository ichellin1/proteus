//! Static image ECS components (M9.7) — PNG/JPEG-as-texture rendering.
//!
//! Mirrors [`crate::text`]'s `Text`/`BakedText` split exactly, just with
//! `proteus_render::static_texture::decode_image` in place of glyph
//! rasterization:
//!
//! ```text
//! Image { bytes }                    ← developer declares on an entity
//!         │
//!         │ (shell detects Image without BakedImage, calls decode_image)
//!         ▼
//! decode_image → DecodedImage (RGBA8 pixels + dimensions)
//!         │
//!         │ (shell calls FontAtlas::bake_image, then write_to_main_atlas)
//!         ▼
//! BakedImage { uv_offset, uv_scale, pixel_size }  ← written back onto the entity
//!         │
//!         │ (collect_instances uses these UVs in the entity's background QuadInstance)
//!         ▼
//! GPU shader samples main_atlas sub-region → tinted image pixel
//! ```
//!
//! ## Sizing — unlike `BakedText`
//!
//! `Text` renders as a *second* overlay instance layered on top of a
//! (possibly differently-sized) parent quad, so `BakedText::pixel_size`
//! exists to size that overlay to the glyph run's own footprint rather than
//! inheriting the wrong size from its parent.
//!
//! `Image` has no such parent/overlay split — it maps directly onto the
//! entity's own background instance, at whatever size the entity's
//! `QuadState` already declares (same as a plain solid-color fill). A tile
//! sized to its poster's aspect ratio just shows the poster; a tile sized
//! differently stretches it to fit, same as any other textured quad.
//! `BakedImage::pixel_size` is carried for parity with `BakedText` and any
//! future aspect-fit logic, but `collect_instances` does not use it to
//! resize the quad.
//!
//! ## Color
//!
//! Unlike text (white base + coverage alpha, designed for tinting), image
//! pixels are the image's real RGB values. `QuadState::color` still
//! multiplies them in the shader, so `Vec4::ONE` (white) is the "untinted"
//! choice for an entity meant to show the image as-is.

use bevy_ecs::prelude::*;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Image component
// ---------------------------------------------------------------------------

/// Declares that an entity should display a decoded PNG/JPEG image.
///
/// When this component is present on an entity that does not yet have a
/// [`BakedImage`] component, the shell's image-bake step decodes `bytes`,
/// uploads the result to `main_atlas`, and inserts [`BakedImage`] with the
/// resulting UV coordinates. The baked texture is permanent for the entity's
/// lifetime — re-baking on content change is not implemented.
#[derive(Component, Clone, Debug)]
pub struct Image {
    /// Raw PNG or JPEG file bytes (format sniffed from the data, not a file
    /// extension). `Arc` so the shell's per-frame "already baked?" check
    /// (mirroring `bake_pending_text`) doesn't copy the whole buffer.
    pub bytes: Arc<[u8]>,
}

impl Image {
    /// `bytes` accepts anything convertible to `Arc<[u8]>` — a `Vec<u8>`
    /// from `std::fs::read`, or a `&[u8]` slice (e.g. from a wasm-bindgen
    /// parameter), without an extra explicit conversion at call sites.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// BakedImage component
// ---------------------------------------------------------------------------

/// Written by the shell after an [`Image`] entity's bytes have been decoded
/// and uploaded to the GPU `main_atlas`.
///
/// The render loop reads `uv_offset`/`uv_scale` to point the entity's
/// background [`QuadInstance`] at the correct atlas sub-region — see the
/// [module docs](self) for why, unlike `BakedText`, this does not resize the
/// entity's own quad.
///
/// [`QuadInstance`]: proteus_render::QuadInstance
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BakedImage {
    /// Normalised UV origin within `main_atlas` (top-left corner of the image region).
    /// Range: [0, 1] × [0, 1].
    pub uv_offset: [f32; 2],
    /// Normalised UV extent of the image region.
    /// `uv_offset + uv_scale` gives the bottom-right UV corner.
    pub uv_scale: [f32; 2],
    /// The decoded image's native size in pixels (`DecodedImage::width`/`height`).
    /// Not used to resize the entity's quad — see the [module docs](self).
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
            pixel_size: [200.0, 300.0],
        };
        let mut world = World::new();
        let e = world.spawn(baked.clone()).id();
        let b = world.get::<BakedImage>(e).unwrap();
        assert_eq!(b.uv_offset, [0.1, 0.2]);
        assert_eq!(b.uv_scale, [0.3, 0.4]);
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
                    pixel_size: [64.0, 64.0],
                },
            ))
            .id();
        assert!(world.get::<Image>(e).is_some());
        assert!(world.get::<BakedImage>(e).is_some());
    }
}
