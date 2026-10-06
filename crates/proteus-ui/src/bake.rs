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
//! BakedComposite + TextureRef    where the bake is in the atlas
//!         │  children destroyed; the entity's own color, corner radius
//!         │  and effects removed, since the bake includes them
//!         ▼
//! drawn as a plain quad showing the bake
//! ```
//!
//! This is an ECS system, unlike text and image baking, because walking the
//! entity's descendants needs queries.
//!
//! The `TextureRef` releases the region when the entity is destroyed; see
//! `crate::texture_ref`.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use glam::Vec4;

use proteus_render::{GpuContext, QuadPipeline};

use crate::component::QuadState;
use crate::effects::{Border, DropShadow, Glow};
use crate::hierarchy::resolve_world_position_query;
use crate::texture_ref::TextureRef;
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
    /// The bake's size in pixels.
    pub pixel_size: [f32; 2],
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
}

/// The entities `bake_system` bakes: marked [`Baked`], not yet baked.
type PendingBakeQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static QuadState), (With<Baked>, Without<BakedComposite>)>;

/// Bakes each entity marked [`Baked`] that isn't baked yet. Runs in
/// [`crate::schedule::ProteusSet::Bake`].
///
/// A bake that can't happen, because the atlas is full or the GPU isn't set up
/// yet, is tried again the next tick.
pub fn bake_system(
    mut commands: Commands,
    query: PendingBakeQuery,
    queries: BakeQueries,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
) {
    let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) else {
        return;
    };
    let BakeQueries {
        quad_states,
        parents,
        children_q,
        visuals,
    } = queries;

    for (entity, local_qs) in query.iter() {
        let world_qs = resolve_world_position_query(entity, local_qs, &quad_states, &parents);

        let instances =
            gather_bake_instances(&visuals, &children_q, &quad_states, entity, &world_qs);
        if instances.is_empty() {
            continue;
        }

        let width = world_qs.size.x.max(1.0).ceil() as u32;
        let height = world_qs.size.y.max(1.0).ceil() as u32;

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
            TextureRef(texture_id),
        ));

        // The bake already includes this entity's color, border, glow and
        // shadow, so remove them, or they would be drawn a second time. The
        // entity becomes a plain white quad showing the bake.
        let mut neutralized = local_qs.clone();
        neutralized.color = Vec4::ONE;
        neutralized.corner_radius = 0.0;
        commands
            .entity(entity)
            .insert(neutralized)
            .remove::<Border>()
            .remove::<Glow>()
            .remove::<DropShadow>();

        // Destroy the direct children; bevy_ecs destroys their descendants
        // with them.
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
    }
}
