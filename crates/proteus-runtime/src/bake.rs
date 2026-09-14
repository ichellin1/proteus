//! Generic `Text` / `Image` → `main_atlas` baking.
//!
//! Lifted verbatim from `proteus-shell-native/src/main.rs` (M13.1 step 2),
//! where `bake_pending_text` / `bake_pending_images` / `bake_images` were
//! identical free functions hand-duplicated into `proteus-shell-web`. Only
//! *which* bytes and *what size cap* were ever platform- or app-specific;
//! rasterizing a `Text` and decoding an `Image` into the atlas is generic,
//! so it belongs here, driven once per frame by [`crate::Renderer::render`].
//!
//! Static **composite** baking (`Baked` / `childBehavior: 'bake'`) is a
//! different thing entirely — it stays in `proteus_ui::bake_system`, which
//! runs inside the ECS schedule and needs `Query` access to walk a subtree.

use std::sync::Arc;

use bevy_ecs::prelude::{Entity, Without};
use bevy_ecs::world::World;

use proteus_render::{
    decode_image, resize_to_fit, DecodedImage, FontAtlas, GpuContext, QuadPipeline, TextureId,
};
use proteus_sdk::TextureHandle;
use proteus_ui::{BakedImage, BakedText, Image, Text, TextureRef};

use crate::services::{HostServices, TextureRequest};

/// Fetch an asset's bytes via `services`, decode, and bake — backs
/// [`Frame::load_texture`]. The fetch-and-decode half of the work; the actual
/// atlas registration/upload is [`bake_texture`], shared with
/// [`Frame::bake_texture`] (M13.4) for a caller that already has pixels in
/// hand and has no key to fetch (e.g. procedurally generated content).
///
/// A missing/undecodable asset yields a null `TextureHandle` — see
/// [`bake_texture`]'s own doc for the rest of the graceful-degradation story.
///
/// [`Frame::load_texture`]: crate::Frame::load_texture
/// [`Frame::bake_texture`]: crate::Frame::bake_texture
pub(crate) fn load_texture(
    world: &mut World,
    services: &mut dyn HostServices,
    key: &str,
    req: TextureRequest,
) -> TextureHandle {
    let Some(bytes) = services.load_asset(key) else {
        log::warn!("load_texture: asset not found: {key}");
        return TextureHandle::from_texture_id(TextureId::default());
    };
    let decoded = match decode_image(&bytes) {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("load_texture: {key}: {e}");
            return TextureHandle::from_texture_id(TextureId::default());
        }
    };
    bake_texture(
        world,
        decoded.width,
        decoded.height,
        decoded.rgba_pixels,
        req,
    )
}

/// Bake already-decoded RGBA pixels (`rgba.len() == width * height * 4`)
/// directly into `main_atlas` and return a handle — the bake-alone half of
/// [`load_texture`], for a caller that already has bytes in hand instead of
/// an asset key to fetch (M13.4; backs [`Frame::bake_texture`]). `req.max_side`
/// still applies, same as `load_texture`'s own downscale.
///
/// A full atlas or a missing `QuadPipeline` both yield a null `TextureHandle`
/// — `Handle::set_texture` and the collect path both no-op on an unknown id,
/// so the component renders as nothing. Same graceful degradation the M12
/// shells' `set_*` asset paths had.
///
/// [`Frame::bake_texture`]: crate::Frame::bake_texture
pub(crate) fn bake_texture(
    world: &mut World,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    req: TextureRequest,
) -> TextureHandle {
    let null = TextureHandle::from_texture_id(TextureId::default());

    let mut decoded = DecodedImage {
        width,
        height,
        rgba_pixels: rgba,
    };
    if let Some(cap) = req.max_side {
        decoded = resize_to_fit(decoded, cap);
    }

    let queue = world.resource::<GpuContext>().queue.clone();
    let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
        return null;
    };
    let Some(texture_id) =
        pipeline
            .texture_registry
            .register_static(decoded.width, decoded.height, req.eternal)
    else {
        log::warn!(
            "bake_texture: main_atlas full — could not register {}x{}",
            decoded.width,
            decoded.height,
        );
        return null;
    };
    let placement = pipeline
        .texture_registry
        .main_atlas_region(texture_id)
        .expect("just registered");
    pipeline.write_to_main_atlas(&queue, placement, &decoded.rgba_pixels);
    TextureHandle::from_texture_id(texture_id)
}

/// Rasterize and upload every `Text` entity that has no `BakedText` yet.
pub(crate) fn bake_pending_text(
    world: &mut World,
    font_atlas: &mut FontAtlas,
    queue: &wgpu::Queue,
) {
    let pending: Vec<(Entity, Text)> = {
        let mut query = world.query_filtered::<(Entity, &Text), Without<BakedText>>();
        query.iter(world).map(|(e, t)| (e, t.clone())).collect()
    };

    for (entity, text) in pending {
        let Some(glyphs) =
            font_atlas.rasterize_text_tracked(&text.content, text.size_px, text.letter_spacing_px)
        else {
            continue;
        };

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(glyphs.width, glyphs.height, false)
            else {
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &glyphs.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            (uv, texture_id)
        };

        world.entity_mut(entity).insert((
            BakedText {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [glyphs.width as f32, glyphs.height as f32],
            },
            TextureRef(texture_id),
        ));
    }
}

/// Decode and upload every `Image` entity that has no `BakedImage` yet.
///
/// Each entity's own [`Image::max_side`] wins; `default_max_side` is the
/// fallback for entities that don't set one (`None` on both = no downscale).
pub(crate) fn bake_pending_images(
    world: &mut World,
    queue: &wgpu::Queue,
    default_max_side: Option<u32>,
) {
    let pending: Vec<(Entity, Arc<[u8]>, Option<u32>)> = {
        let mut query = world.query_filtered::<(Entity, &Image), Without<BakedImage>>();
        query
            .iter(world)
            .map(|(e, img)| (e, img.bytes.clone(), img.max_side))
            .collect()
    };

    for (entity, bytes, max_side) in pending {
        let mut decoded = match decode_image(&bytes) {
            Ok(decoded) => decoded,
            Err(e) => {
                log::warn!("bake_pending_images: entity {entity:?}: {e}");
                continue;
            }
        };
        if let Some(cap) = max_side.or(default_max_side) {
            decoded = resize_to_fit(decoded, cap);
        }

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(decoded.width, decoded.height, false)
            else {
                log::warn!(
                    "bake_pending_images: main_atlas full — could not register {}x{} for entity {entity:?}",
                    decoded.width,
                    decoded.height,
                );
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &decoded.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            (uv, texture_id)
        };

        world.entity_mut(entity).insert((
            BakedImage {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [decoded.width as f32, decoded.height as f32],
            },
            TextureRef(texture_id),
        ));
    }
}
