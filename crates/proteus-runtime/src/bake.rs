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
use proteus_ui::{BakedImage, BakedText, EffectiveVisibility, Image, Text, TextureRef, Visibility};

use crate::services::{HostServices, TextureRequest};

/// Same "prefer the cascaded `EffectiveVisibility`, fall back to the
/// entity's own raw `Visibility`, default visible" convention
/// `proteus_ui::collect_instances` already uses — see that function's own
/// doc for why (a bare `World` in a test may never have run the visibility
/// cascade at all).
fn is_visible(vis: Option<&Visibility>, eff_vis: Option<&EffectiveVisibility>) -> bool {
    eff_vis
        .map(|v| v.0)
        .unwrap_or_else(|| vis.map(|v| v.visible).unwrap_or(true))
}

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

/// Rasterize and upload every `Text` entity that has no `BakedText` yet. If
/// `lazy_load` is set (M13.4 step 2 — `ResourceConfig.lazy_load`, declared in
/// M13.5, previously never read), an entity that isn't currently visible is
/// left pending rather than baked — it'll be picked up here again on some
/// later call once it becomes visible.
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
/// `lazy_load` — see [`bake_pending_text`]'s identical doc.
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

#[cfg(test)]
mod tests {
    use super::is_visible;
    use proteus_ui::{EffectiveVisibility, Visibility};

    // Mirrors M6's own stated preference for deterministic, GPU-free tests
    // over pixel/integration ones (see PLANNING.md) — `is_visible` is the
    // one piece of lazy-load logic worth pinning down in isolation; the
    // surrounding `query_filtered` plumbing reuses the exact pattern
    // `proteus_ui::collect_instances` already has extensive coverage for.

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
        // A visible entity under a hidden ancestor: EffectiveVisibility
        // reflects the cascade, raw Visibility does not — the cascaded
        // value must win, exactly like collect_instances.
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
