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

use proteus_render::{decode_image, resize_to_fit, FontAtlas, GpuContext, QuadPipeline, TextureId};
use proteus_sdk::TextureHandle;
use proteus_ui::{BakedImage, BakedText, Image, Text, TextureRef};

use crate::services::{HostServices, TextureRequest};

/// Fetch an asset's bytes via `services`, decode + downscale, upload to
/// `main_atlas`, and return a handle. Backs [`Frame::load_texture`].
///
/// A missing/undecodable asset, a full atlas, or a missing `QuadPipeline`
/// all yield a null `TextureHandle` — `Handle::set_texture` and the collect
/// path both no-op on an unknown id, so the component renders as nothing.
/// Same graceful degradation the M12 shells' `set_*` asset paths had.
///
/// [`Frame::load_texture`]: crate::Frame::load_texture
pub(crate) fn load_texture(
    world: &mut World,
    services: &mut dyn HostServices,
    key: &str,
    req: TextureRequest,
) -> TextureHandle {
    let null = TextureHandle::from_texture_id(TextureId::default());

    let Some(bytes) = services.load_asset(key) else {
        log::warn!("load_texture: asset not found: {key}");
        return null;
    };
    let mut decoded = match decode_image(&bytes) {
        Ok(decoded) => decoded,
        Err(e) => {
            log::warn!("load_texture: {key}: {e}");
            return null;
        }
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
            "load_texture: {key}: main_atlas full — could not register {}x{}",
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
