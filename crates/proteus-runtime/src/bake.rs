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
use proteus_ui::{
    BakedImage, BakedText, EffectiveVisibility, Image, ImageTextureRef, Text, TextTextureRef,
    Visibility,
};

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
///
/// Text wider or taller than an atlas page is clipped to the page, with a
/// warning, and drawn at its normal size. Text that draws nothing, such as an
/// empty string, gets a zero-size `BakedText` and no texture, so it counts as
/// baked and isn't tried again every frame.
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
        let Some(mut glyphs) =
            font_atlas.rasterize_text(&text.content, text.size_px, text.letter_spacing_px)
        else {
            world.entity_mut(entity).insert(BakedText {
                uv_offset: [0.0, 0.0],
                uv_scale: [0.0, 0.0],
                page: 0,
                pixel_size: [0.0, 0.0],
            });
            continue;
        };

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let max = pipeline.texture_registry.max_texture_side();
            if glyphs.width > max || glyphs.height > max {
                let (width, height) = (glyphs.width.min(max), glyphs.height.min(max));
                log::warn!(
                    "the text of entity {entity:?} is {}x{} pixels, larger than an atlas page \
                     ({max}x{max}); it is clipped to {width}x{height}",
                    glyphs.width,
                    glyphs.height,
                );
                glyphs.rgba_pixels = clip_rgba(&glyphs.rgba_pixels, glyphs.width, width, height);
                glyphs.width = width;
                glyphs.height = height;
            }
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
            TextTextureRef(texture_id),
        ));
    }
}

/// Decodes and uploads the image of every entity that has `Image` but no
/// `BakedImage` yet. With `lazy_load`, a hidden entity is skipped until it is
/// visible.
///
/// An image is scaled down to its own [`Image::max_side`], or else to
/// `default_max_side`; with neither, it keeps its full size. An image still
/// larger than an atlas page is then scaled down to fit it, with a warning.
/// An image that can't be decoded is removed from its entity, with a warning,
/// so it isn't tried again every frame.
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
                log::warn!("bake_pending_images: entity {entity:?}: {e}; the image is removed");
                world.entity_mut(entity).remove::<Image>();
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
            let max = pipeline.texture_registry.max_texture_side();
            if decoded.width > max || decoded.height > max {
                log::warn!(
                    "the image of entity {entity:?} is {}x{} pixels, larger than an atlas page \
                     ({max}x{max}); it is scaled down to fit",
                    decoded.width,
                    decoded.height,
                );
                decoded = resize_to_fit(decoded, max);
            }
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
            ImageTextureRef(texture_id),
        ));
    }
}

/// The top-left `width × height` pixels of an RGBA image `src_width` pixels
/// wide.
fn clip_rgba(rgba: &[u8], src_width: u32, width: u32, height: u32) -> Vec<u8> {
    let src_row = src_width as usize * 4;
    let row = width as usize * 4;
    rgba.chunks_exact(src_row)
        .take(height as usize)
        .flat_map(|src| &src[..row])
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_render::{AtlasConfig, DEFAULT_TRANSITION_ATLAS_SIZE};

    use crate::app::video_tests::headless_device;

    // A world with a one-page atlas of 256 × 256, so that modest content is
    // larger than a page, and with texture references counted.
    fn world_with_small_atlas(device: &wgpu::Device, queue: &wgpu::Queue) -> World {
        let mut world = World::new();
        proteus_ui::texture_ref::register_texture_ref_hooks(&mut world);
        world.insert_resource(QuadPipeline::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            16,
            AtlasConfig {
                page_size: 256,
                page_count: 1,
            },
            DEFAULT_TRANSITION_ATLAS_SIZE,
        ));
        world
    }

    #[test]
    fn text_wider_than_a_page_is_clipped_to_it() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        let mut world = world_with_small_atlas(&device, &queue);
        let mut font_atlas = FontAtlas::with_embedded_font();
        let unclipped = font_atlas.rasterize_text("PROTEUS", 128.0, 0.0).unwrap();
        assert!(
            unclipped.width > 256,
            "sanity: the text is wider than a page"
        );

        let entity = world.spawn(Text::new("PROTEUS", 128.0)).id();
        bake_pending_text(&mut world, &mut font_atlas, &queue, false);

        let baked = world.get::<BakedText>(entity).expect("baked, clipped");
        assert_eq!(
            baked.pixel_size,
            [256.0, unclipped.height as f32],
            "clipped to the page's width, at its normal height"
        );
    }

    #[test]
    fn an_image_larger_than_a_page_is_scaled_to_fit_it() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        let mut world = world_with_small_atlas(&device, &queue);
        let mut png = Vec::new();
        image::RgbaImage::new(512, 128)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let entity = world.spawn(Image::new(png)).id();
        bake_pending_images(&mut world, &queue, None, false);

        let baked = world.get::<BakedImage>(entity).expect("baked, scaled");
        assert_eq!(baked.pixel_size, [256.0, 64.0], "half size, aspect kept");
    }

    // A component that draws text and an image holds a reference to each, so
    // neither can be evicted while it is drawn.
    #[test]
    fn text_and_an_image_on_one_component_both_survive_eviction() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        let mut world = world_with_small_atlas(&device, &queue);
        let mut png = Vec::new();
        image::RgbaImage::new(64, 64)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let entity = world.spawn((Text::new("Hi", 32.0), Image::new(png))).id();
        bake_pending_text(
            &mut world,
            &mut FontAtlas::with_embedded_font(),
            &queue,
            false,
        );
        let text = world.get::<TextTextureRef>(entity).unwrap().0;
        bake_pending_images(&mut world, &queue, None, false);
        let image = world.get::<ImageTextureRef>(entity).unwrap().0;

        // Far more than the page holds, so everything unreferenced is evicted
        // once a frame has passed.
        let mut pipeline = world.resource_mut::<QuadPipeline>();
        pipeline.texture_registry.advance_frame();
        for _ in 0..200 {
            pipeline.texture_registry.register_static(32, 32, false);
        }

        let registry = &pipeline.texture_registry;
        assert!(
            registry.main_atlas_region(text).is_some(),
            "the text is kept"
        );
        assert!(
            registry.main_atlas_region(image).is_some(),
            "the image is kept"
        );
    }

    // `set_text` gets the new text baked and releases the old text's
    // texture, so it can be reclaimed.
    #[test]
    fn set_text_rebakes_and_releases_the_old_texture() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        let mut proteus = proteus_sdk::Proteus::new();
        let world = proteus.world_mut();
        world.insert_resource(QuadPipeline::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            16,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        ));
        let mut font_atlas = FontAtlas::with_embedded_font();
        let handle = proteus.component(
            proteus_sdk::ComponentSpec::new(proteus_ui::QuadState::default())
                .text(Text::new("Hi", 32.0)),
        );
        bake_pending_text(proteus.world_mut(), &mut font_atlas, &queue, false);
        let old = proteus
            .world()
            .get::<TextTextureRef>(handle.id())
            .unwrap()
            .0;
        let old_width = proteus
            .world()
            .get::<BakedText>(handle.id())
            .unwrap()
            .pixel_size[0];

        handle
            .set_text(&mut proteus, Text::new("Hello there", 32.0))
            .unwrap();
        bake_pending_text(proteus.world_mut(), &mut font_atlas, &queue, false);

        let baked = proteus.world().get::<BakedText>(handle.id()).unwrap();
        assert!(baked.pixel_size[0] > old_width, "the new, longer text");
        let mut pipeline = proteus.world_mut().resource_mut::<QuadPipeline>();
        pipeline.texture_registry.free(old);
        assert!(
            pipeline.texture_registry.main_atlas_region(old).is_none(),
            "the old texture has no references left, so it can be freed"
        );
    }

    // Content that can never bake must settle, not be tried every frame: a
    // `.bake()` component waits for its subtree's text and images.
    #[test]
    fn empty_text_and_an_undecodable_image_settle_instead_of_retrying() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        let mut world = world_with_small_atlas(&device, &queue);
        let empty = world.spawn(Text::new(" ", 16.0)).id();
        let broken = world.spawn(Image::new(vec![1u8, 2, 3])).id();

        bake_pending_text(
            &mut world,
            &mut FontAtlas::with_embedded_font(),
            &queue,
            false,
        );
        bake_pending_images(&mut world, &queue, None, false);

        let baked = world.get::<BakedText>(empty).expect("counts as baked");
        assert_eq!(baked.pixel_size, [0.0, 0.0]);
        assert!(
            world.get::<TextTextureRef>(empty).is_none(),
            "with no texture"
        );
        assert!(world.get::<Image>(broken).is_none(), "removed");
    }

    #[test]
    fn clip_rgba_keeps_the_top_left_pixels() {
        // A 3 × 2 image whose pixels are numbered 0 to 5, clipped to 2 × 1.
        let rgba: Vec<u8> = (0..6u8).flat_map(|i| [i; 4]).collect();
        assert_eq!(clip_rgba(&rgba, 3, 2, 1), [0, 0, 0, 0, 1, 1, 1, 1]);
    }

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
