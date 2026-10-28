//! Baking a component: drawing it and its children into one texture,
//! permanently. Reduces complexity of composit components that do not have dynamic children.
//!
//! An entity marked [`Baked`] is rendered once, with all its descendants, into
//! the main atlas. The children are destroyed, and the entity becomes a plain
//! quad showing the bake, like any other component with an image.
//!
//! ## Data flow
//!
//! ```text
//! Baked                          on the entity to bake
//!         │  bake_system finds Baked without BakedComposite
//!         ▼
//! gather_bake_instances          the entity and its descendants, as quads
//!         │  drawn into a new main-atlas region
//!         ▼
//! BakedComposite + its ref      where the bake is in the atlas
//!         │  children destroyed; the entity's own color, corner radius
//!         │  and effects removed, since the bake includes them
//!         ▼
//! drawn as a plain quad showing the bake
//! ```
//!
//! This is an ECS system, unlike text and image baking, because walking the
//! entity's descendants needs queries.
//!
//! The bake waits until every `Text` and `Image` in the subtree, the entity's
//! own included, has been baked by the host. Text and images are baked when
//! the host renders, after the tick, so a component created before the first
//! frame would otherwise be baked without them, and its children, destroyed
//! by the bake, would take them with them.
//!
//! The `CompositeTextureRef` releases the region when the entity is destroyed; see
//! `crate::texture_ref`.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use glam::Vec4;

use proteus_render::{GpuContext, QuadPipeline};

use crate::component::QuadState;
use crate::effects::{Border, DropShadow, Glow};
use crate::hierarchy::resolve_world_position_query;
use crate::image::{BakedImage, Image};
use crate::text::{BakedText, Text};
use crate::texture_ref::{CompositeTextureRef, ImageTextureRef, TextTextureRef};
use crate::topology::{gather_bake_instances, BakeVisualsQuery};

// ---------------------------------------------------------------------------
// Baked / BakedComposite
// ---------------------------------------------------------------------------

/// Marks an entity to be baked, with its descendants, into one texture,
/// permanently.
///
/// `bake_system` renders it into the main atlas, destroys the children, and
/// adds a [`BakedComposite`]. `Baked` stays on the entity afterwards, as
/// `Text` stays alongside `BakedText`.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Baked;

/// Where a baked entity's image is in the main atlas, added once it has been
/// baked. Drawn like a [`crate::BakedImage`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BakedComposite {
    /// Texture coordinates of the bake's top-left corner in the atlas.
    pub uv_offset: [f32; 2],
    /// The bake's size in texture coordinates.
    pub uv_scale: [f32; 2],
    /// The main-atlas page the bake is on.
    pub page: u32,
    /// The bake's size in the atlas, in texels: its logical size times the
    /// [`BakeScale`] it was baked at. It is drawn at its logical size.
    pub pixel_size: [f32; 2],
}

// ---------------------------------------------------------------------------
// BakeScale
// ---------------------------------------------------------------------------

/// How many texels a baked texture has per logical pixel: the display's scale
/// factor, capped by `ResourceConfig::max_bake_scale` and at least 1. Text,
/// baked components and the images splits and merges bake are all baked at
/// it, and drawn at their logical size, so on a high-density display they're
/// as sharp as the shapes the shader draws.
///
/// The renderer sets it from the viewport; without one, as in tests, it is 1,
/// a texel per logical pixel.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct BakeScale(pub f32);

impl Default for BakeScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The scale to bake something `width × height` logical pixels at, so that it
/// fits a texture `max` pixels on a side: `scale` if that fits, else the
/// largest scale that does, but not below 1. At 1, something larger than
/// `max` still doesn't fit, and the caller handles it as it would without a
/// bake scale.
pub fn bake_scale_to_fit(scale: f32, width: f32, height: f32, max: u32) -> f32 {
    let max = max as f32;
    let fits = |s: f32| (width * s).ceil() <= max && (height * s).ceil() <= max;
    if fits(scale) {
        return scale;
    }
    // The largest that could fit, then down a 64th at a time until rounding
    // up to whole texels fits too.
    let mut s = (max / width.max(height).max(1.0)).min(scale);
    while s > 1.0 && !fits(s) {
        s = (s - 1.0 / 64.0).max(1.0);
    }
    s.max(1.0)
}

/// The most of an atlas a group's bakes may take between them at their scale,
/// leaving the rest for the gaps between regions as they're packed, and for
/// the gutters around them.
const GROUP_FILL: f32 = 0.6;

/// The scale to bake a group of things at together, such as a split's source
/// and its targets, each `size` logical pixels, so that they fit an atlas
/// `max` pixels on a side between them: `scale` if they take at most 60% of
/// it at that scale, else the largest scale at which they do, but never
/// below a scale of 1. A group of small bakes keeps the display's scale;
/// one with a bake near the window's size takes less, so none of its bakes
/// is left a plain shape for want of room.
pub fn group_bake_scale(scale: f32, sizes: impl IntoIterator<Item = glam::Vec2>, max: u32) -> f32 {
    let area: f32 = sizes.into_iter().map(|s| s.x.max(1.0) * s.y.max(1.0)).sum();
    let budget = GROUP_FILL * (max as f32) * (max as f32);
    if area * scale * scale <= budget {
        return scale;
    }
    (budget / area).sqrt().clamp(1.0, scale.max(1.0))
}

// ---------------------------------------------------------------------------
// bake_system
// ---------------------------------------------------------------------------

/// The read-only queries `bake_system` needs, grouped to keep its argument
/// list short.
#[derive(SystemParam)]
pub struct BakeQueries<'w, 's> {
    quad_states: Query<'w, 's, &'static QuadState>,
    parents: Query<'w, 's, &'static ChildOf>,
    children_q: Query<'w, 's, &'static Children>,
    visuals: BakeVisualsQuery<'w, 's>,
    /// Entities whose text or image the host hasn't baked yet.
    unbaked_content: Query<'w, 's, (), UnbakedContentFilter>,
}

/// Whether `entity` or any of its descendants has text or an image the host
/// hasn't baked yet.
fn has_unbaked_content(
    entity: Entity,
    children_q: &Query<&Children>,
    unbaked_content: &Query<(), UnbakedContentFilter>,
) -> bool {
    unbaked_content.contains(entity)
        || children_q.get(entity).is_ok_and(|children| {
            children
                .iter()
                .any(|child| has_unbaked_content(child, children_q, unbaked_content))
        })
}

type UnbakedContentFilter = Or<(
    (With<Text>, Without<BakedText>),
    (With<Image>, Without<BakedImage>),
)>;

/// The entities `bake_system` bakes: marked [`Baked`], not yet baked.
type PendingBakeQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static QuadState), (With<Baked>, Without<BakedComposite>)>;

/// Bakes each entity marked [`Baked`] that isn't baked yet. Runs in
/// [`crate::schedule::ProteusSet::Bake`].
///
/// A bake that can't happen yet, because the subtree's text or images aren't
/// baked yet, the atlas is full or the GPU isn't set up, is tried again the
/// next tick. An entity larger than an atlas page can
/// never be baked: its `Baked` marker is removed, with a warning, and it is
/// drawn normally, unbaked.
pub fn bake_system(
    mut commands: Commands,
    query: PendingBakeQuery,
    queries: BakeQueries,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
    bake_scale: Option<Res<BakeScale>>,
) {
    let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) else {
        return;
    };
    let BakeQueries {
        quad_states,
        parents,
        children_q,
        visuals,
        unbaked_content,
    } = queries;

    for (entity, local_qs) in query.iter() {
        if has_unbaked_content(entity, &children_q, &unbaked_content) {
            continue;
        }
        let world_qs = resolve_world_position_query(entity, local_qs, &quad_states, &parents);

        let instances =
            gather_bake_instances(&visuals, &children_q, &quad_states, entity, &world_qs);
        if instances.is_empty() {
            continue;
        }

        // At the bake scale, or as near as fits a page; drawn at its logical
        // size, by the same projection.
        let max = pipeline.texture_registry.max_texture_side();
        let (logical_w, logical_h) = (world_qs.size.x.max(1.0), world_qs.size.y.max(1.0));
        // 1 in a world without one, as in tests.
        let scale = bake_scale_to_fit(
            bake_scale.as_ref().map_or(1.0, |s| s.0),
            logical_w,
            logical_h,
            max,
        );
        let width = (logical_w * scale).ceil() as u32;
        let height = (logical_h * scale).ceil() as u32;

        if width > max || height > max {
            log::warn!(
                "bake_system: entity {entity:?} is {width}x{height} pixels, larger than an atlas \
                 page ({max}x{max}); it is drawn unbaked"
            );
            commands.entity(entity).remove::<Baked>();
            continue;
        }

        let Some(texture_id) = pipeline
            .texture_registry
            .register_static(width, height, false)
        else {
            log::warn!(
                "bake_system: main_atlas full — cannot bake entity {entity:?} ({width}x{height})"
            );
            continue;
        };
        let placement = pipeline
            .texture_registry
            .main_atlas_region(texture_id)
            .expect("just registered");

        let view_projection = QuadPipeline::ortho_centered(
            world_qs.position.x,
            world_qs.position.y,
            world_qs.size.x,
            world_qs.size.y,
        );
        pipeline.bake_instances_to_main_atlas(
            &gpu.device,
            &gpu.queue,
            &instances,
            view_projection,
            placement,
        );

        let uv = pipeline
            .texture_registry
            .main_atlas_uv(texture_id)
            .expect("just registered");
        commands.entity(entity).insert((
            BakedComposite {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [width as f32, height as f32],
            },
            CompositeTextureRef(texture_id),
        ));

        // The bake already includes this entity's color, border, glow,
        // shadow, text and image, so remove them, or they would be drawn a
        // second time. The entity becomes a plain white quad showing the bake.
        let mut neutralized = local_qs.clone();
        neutralized.color = Vec4::ONE;
        neutralized.corner_radius = 0.0;
        commands
            .entity(entity)
            .insert(neutralized)
            .remove::<(Border, Glow, DropShadow)>()
            .remove::<(Text, BakedText, TextTextureRef)>()
            .remove::<(Image, BakedImage, ImageTextureRef)>();

        // Destroy the direct children; bevy_ecs destroys their descendants
        // with them.
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
    }
}

#[cfg(test)]
mod bake_scale_tests {
    use super::bake_scale_to_fit;

    #[test]
    fn a_bake_that_fits_keeps_its_scale() {
        assert_eq!(bake_scale_to_fit(2.0, 300.0, 200.0, 2048), 2.0);
        assert_eq!(bake_scale_to_fit(1.25, 300.0, 200.0, 2048), 1.25);
    }

    #[test]
    fn a_bake_too_large_takes_the_largest_scale_that_fits() {
        // 1,200 wide fits a 2,048 page at 1, not at 2.
        let scale = bake_scale_to_fit(2.0, 1200.0, 800.0, 2048);
        assert!(scale < 2.0 && scale > 1.6, "{scale}");
        assert!((1200.0 * scale).ceil() <= 2048.0);
    }

    #[test]
    fn a_bake_too_large_at_one_is_left_at_one() {
        assert_eq!(bake_scale_to_fit(2.0, 3000.0, 100.0, 2048), 1.0);
    }

    #[test]
    fn a_small_group_keeps_its_scale() {
        let tiles = [glam::Vec2::splat(200.0); 4];
        assert_eq!(super::group_bake_scale(2.0, tiles, 2044), 2.0);
    }

    #[test]
    fn a_large_group_takes_a_scale_at_which_it_fits() {
        // The demo's gallery: an enlarged photo and twelve tiles.
        let group =
            std::iter::once(glam::Vec2::new(1100.0, 750.0)).chain([glam::Vec2::splat(220.0); 12]);
        let scale = super::group_bake_scale(2.0, group.clone(), 2044);
        assert!(scale > 1.0 && scale < 2.0, "{scale}");
        let area: f32 = group.map(|s| s.x * s.y * scale * scale).sum();
        assert!(area <= 0.6 * 2044.0 * 2044.0 + 1.0, "{area}");
    }

    #[test]
    fn a_group_too_large_at_one_is_left_at_one() {
        let huge = [glam::Vec2::splat(2000.0); 3];
        assert_eq!(super::group_bake_scale(2.0, huge, 2044), 1.0);
    }
}
