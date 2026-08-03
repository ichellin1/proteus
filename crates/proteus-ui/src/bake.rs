//! Static component baking — M10.5, freeing wired to the real registry in M11.
//!
//! `Baked` collapses a composite (a `Quad` parent + its children) into a
//! single permanent textured quad: the subtree is rendered once into
//! `main_atlas`, the child entities are despawned, and the parent becomes a
//! plain leaf quad pointed at the baked region — "indistinguishable from any
//! other component," per the original design in `PLANNING.md`.
//!
//! ## Data flow
//!
//! ```text
//! Baked                              ← developer declares on the composite's root entity
//!         │
//!         │ (bake_system detects Baked without BakedComposite)
//!         ▼
//! gather_bake_instances → Vec<QuadInstance>   (crate::topology — recursive subtree capture,
//!         │                                     the same logic the Slice-transition crossfade uses)
//!         │
//!         │ TextureRegistry::register_static + QuadPipeline::bake_instances_to_main_atlas
//!         ▼
//! BakedComposite { uv_offset, uv_scale, page, pixel_size } + TextureRef  ← written back onto the root entity
//!         │
//!         │ children despawned; root's own Border/Glow/DropShadow removed and
//!         │ QuadState color/corner_radius neutralized (the bake already captured them as pixels)
//!         ▼
//! collect_instances renders the root as a plain textured quad, same as BakedImage
//! ```
//!
//! ## Why this is a real ECS system, not a shell method like `bake_pending_text`
//!
//! `gather_bake_instances`/the queries below need real ECS `Query` access to walk an arbitrary-
//! depth subtree — naturally available inside a system, awkward from a shell method. `main_atlas`
//! allocation itself goes through `QuadPipeline::texture_registry` (M11's `TextureRegistry`), which
//! `bake_system` reaches the same way it reaches `GpuContext`/`QuadPipeline` — no `FontAtlas`
//! involvement needed here at all: composite baking renders pixels directly via
//! `bake_instances_to_main_atlas`, it never had text/image bytes to rasterize or decode.
//!
//! ## Freeing (M11)
//!
//! `TextureRef`, inserted alongside `BakedComposite` below, ref-counts this region against the
//! entity's lifetime via `ComponentHooks` — despawning the entity (or replacing/removing
//! `TextureRef`) decrements the registry's ref count, making the region a reclaim candidate. See
//! `crate::texture_ref` for the full mechanism.

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

/// Declares that an entity's subtree should be permanently baked into a
/// single textured quad.
///
/// When this component is present on an entity that does not yet have a
/// [`BakedComposite`], `bake_system` renders the entity and every descendant
/// into `main_atlas`, despawns the children, and inserts [`BakedComposite`].
/// `Baked` itself is left on the entity afterward — it stays the marker of
/// "this entity is a baked composite," the same way `Text` stays present
/// alongside `BakedText` once baked.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Baked;

/// Written by `bake_system` once an entity's [`Baked`] subtree has been
/// rendered into `main_atlas`.
///
/// Same shape as [`crate::BakedImage`]/[`crate::BakedText`] — the render path
/// (`collect.rs`) reads `uv_offset`/`uv_scale` to point the entity's
/// `QuadInstance` at the baked region, exactly like any other textured quad.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BakedComposite {
    /// Normalised UV origin within `main_atlas`.
    pub uv_offset: [f32; 2],
    /// Normalised UV extent within `main_atlas`.
    pub uv_scale: [f32; 2],
    /// Which `main_atlas` array layer (M11.2) `uv_offset`/`uv_scale` address — see
    /// `proteus_ui::image::BakedImage::page`'s doc for why this exists.
    pub page: u32,
    /// The baked region's size in pixels.
    pub pixel_size: [f32; 2],
}

// ---------------------------------------------------------------------------
// bake_system
// ---------------------------------------------------------------------------

/// Read-only queries `bake_system` needs beyond the `Baked`-matching query
/// itself — bundled via `#[derive(SystemParam)]` to keep the system's own
/// argument count reasonable (bevy systems commonly need many queries/
/// resources; grouping the read-only ones is the idiomatic way to do that
/// without tripping `clippy::too_many_arguments`).
#[derive(SystemParam)]
pub struct BakeQueries<'w, 's> {
    quad_states: Query<'w, 's, &'static QuadState>,
    parents: Query<'w, 's, &'static ChildOf>,
    children_q: Query<'w, 's, &'static Children>,
    visuals: BakeVisualsQuery<'w, 's>,
}

/// Query filter matching `bake_system`'s targets: entities with a declared
/// [`Baked`] intent that haven't been baked yet.
type PendingBakeQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static QuadState), (With<Baked>, Without<BakedComposite>)>;

/// Replaces `stub_bake_system`. Runs every frame in
/// [`crate::schedule::ProteusSet::Bake`].
///
/// Matches `(With<Baked>, Without<BakedComposite>)` rather than
/// `Added<Baked>` so a bake that fails one frame (e.g. `main_atlas` full, or
/// `GpuContext`/`QuadPipeline`/`FontAtlas` not yet available) is simply
/// retried the next frame — the same graceful-degradation shape
/// `bake_pending_text`/`bake_pending_images` already use, rather than a
/// one-shot trigger that could silently never fire again.
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

        // The bake above already captured this entity's own fill/border/glow/
        // shadow as pixels in the texture — leaving them live would render
        // them a second time on top of the baked result. Neutralize to a
        // plain white pass-through quad, same fix already used for the
        // Slice-transition crossfade in topology.rs (see its `BakedTexture`
        // handling): the baked pixels carry the real shape and color now.
        let mut neutralized = local_qs.clone();
        neutralized.color = Vec4::ONE;
        neutralized.corner_radius = 0.0;
        commands
            .entity(entity)
            .insert(neutralized)
            .remove::<Border>()
            .remove::<Glow>()
            .remove::<DropShadow>();

        // Despawn direct children — bevy_ecs's ChildOf/Children relationship
        // cascades this recursively, so descendants-of-descendants are
        // cleaned up for free (proven by M10's despawning_parent_despawns_child
        // test).
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                commands.entity(child).despawn();
            }
        }
    }
}
