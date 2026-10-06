//! Baking a component's `Text` and `Image` into the atlas, once per frame from
//! [`crate::Renderer::render`].
//!
//! Baking a whole component (`ComponentSpec::bake`) is separate: it is
//! `proteus_ui::bake_system`, which runs in the ECS schedule because it walks
//! the component's children.

use std::sync::Arc;

use bevy_ecs::prelude::{Entity, Without};
use bevy_ecs::world::World;

use proteus_render::{decode_image, resize_to_fit, FontAtlas, QuadPipeline, TextureId};
use proteus_sdk::TextureHandle;
use proteus_ui::{BakedImage, BakedText, EffectiveVisibility, Image, Text, TextureRef, Visibility};

use crate::services::HostServices;
use proteus_sdk::TextureRequest;

/// Whether an entity is visible: its cascaded visibility if that has been
/// computed, else its own, else visible. The same rule
/// `proteus_ui::collect_instances` uses.
fn is_visible(vis: Option<&Visibility>, eff_vis: Option<&EffectiveVisibility>) -> bool {
    eff_vis
        .map(|v| v.0)
        .unwrap_or_else(|| vis.map(|v| v.visible).unwrap_or(true))
}

/// Loads an asset through `services`, decodes it and bakes it. Implements
/// [`Frame::load_texture`](crate::Frame::load_texture). A missing or
/// undecodable asset gives a null handle.
pub(crate) fn load_texture(
    proteus: &mut proteus_sdk::Proteus,
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
    // Decoded here rather than by `Proteus::load_texture`, so that the
    // failure log can name the asset key.
    bake_texture(
        proteus,
        decoded.width,
        decoded.height,
        decoded.rgba_pixels,
        req,
    )
}
/// Adds RGBA pixels to the atlas, through
/// [`proteus_sdk::Proteus::bake_texture`].
pub(crate) fn bake_texture(
    proteus: &mut proteus_sdk::Proteus,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    req: TextureRequest,
) -> TextureHandle {
    proteus.bake_texture(width, height, rgba, req)
}

/// Rasterizes and uploads the text of every entity that has `Text` but no
/// `BakedText` yet. With `lazy_load`, a hidden entity is skipped until it is
/// visible.
pub(crate) fn bake_pending_text(
    world: &mut World,
    font_atlas: &mut FontAtlas,
    queue: &wgpu::Queue,
    lazy_load: bool,
) {
    let pending: Vec<(Entity, Text)> = {
        let mut query = world.query_filtered::<(
            Entity,
            &Text,
            Option<&Visibility>,
            Option<&EffectiveVisibility>,
        ), Without<BakedText>>();
        query
            .iter(world)
            .filter(|(_, _, vis, eff_vis)| !lazy_load || is_visible(*vis, *eff_vis))
            .map(|(e, t, _, _)| (e, t.clone()))
            .collect()
    };

    for (entity, text) in pending {
        let Some(glyphs) =
            font_atlas.rasterize_text(&text.content, text.size_px, text.letter_spacing_px)
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

/// Decodes and uploads the image of every entity that has `Image` but no
/// `BakedImage` yet. With `lazy_load`, a hidden entity is skipped until it is
/// visible.
///
/// An image is scaled down to its own [`Image::max_side`], or else to
/// `default_max_side`; with neither, it keeps its full size.
pub(crate) fn bake_pending_images(
    world: &mut World,
    queue: &wgpu::Queue,
    default_max_side: Option<u32>,
    lazy_load: bool,
) {
    let pending: Vec<(Entity, Arc<[u8]>, Option<u32>)> = {
        let mut query = world.query_filtered::<(
            Entity,
            &Image,
            Option<&Visibility>,
            Option<&EffectiveVisibility>,
        ), Without<BakedImage>>();
        query
            .iter(world)
            .filter(|(_, _, vis, eff_vis)| !lazy_load || is_visible(*vis, *eff_vis))
            .map(|(e, img, _, _)| (e, img.bytes.clone(), img.max_side))
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
            BakedImage::new(
                uv.uv_offset,
                uv.uv_scale,
                uv.page,
                [decoded.width as f32, decoded.height as f32],
            ),
            TextureRef(texture_id),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::is_visible;
    use proteus_ui::{EffectiveVisibility, Visibility};

    // `is_visible` is the part of lazy loading worth testing on its own, and
    // it needs no GPU.

    #[test]
    fn no_components_defaults_to_visible() {
        assert!(is_visible(None, None));
    }

    #[test]
    fn raw_visibility_used_when_no_effective_visibility() {
        assert!(is_visible(Some(&Visibility::VISIBLE), None));
        assert!(!is_visible(Some(&Visibility::HIDDEN), None));
    }

    #[test]
    fn effective_visibility_wins_over_raw_visibility() {
        // A visible entity under a hidden ancestor: the cascaded visibility
        // must win over the entity's own, as in collect_instances.
        assert!(!is_visible(
            Some(&Visibility::VISIBLE),
            Some(&EffectiveVisibility(false))
        ));
        assert!(is_visible(
            Some(&Visibility::HIDDEN),
            Some(&EffectiveVisibility(true))
        ));
    }
}
