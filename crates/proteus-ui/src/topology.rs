//! Splits (1→N) and merges (N→1).
//!
//! A transition takes one of three shapes:
//!
//! - **1→1:** one entity transitions into another; see `transition.rs`.
//! - **1→N:** one source splits into N targets; see [`OneToNRequest`].
//! - **N→1:** N sources merge into one destination; see [`NToOneRequest`].
//!
//! ## How splits and merges work
//!
//! Except for [`SplitStrategy::PerTarget`], a split or merge creates one
//! [`Virtual`] entity per piece, with an ordinary `ActiveTransition`. When GPU
//! resources are available, each piece shows baked images of the entities at
//! both ends and fades from one to the other. Without them, or if a bake
//! fails, a piece instead carries the visual components ([`DropShadow`],
//! [`Glow`], [`BakedText`]) of the entity it comes from, so they don't appear
//! or vanish at the end.
//!
//! When every piece has arrived, [`group_transition_complete_system`] shows
//! the real entities, removes the pieces, and returns the entity that
//! coordinates the group, the source of a split or the destination of a
//! merge, to `Idle`.

use std::collections::HashMap;

use bevy_ecs::hierarchy::Children;
use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{
    pack_atlas_page, GpuContext, QuadInstance, QuadPipeline, TransitionAllocId, TransitionRegion,
    ATLAS_SELECTOR_MAIN, DEFAULT_TRANSITION_ATLAS_SIZE,
};

use crate::collect::{quad_state_to_instance, BakedTexture};
use crate::component::{Lifecycle, QuadState, TransitionRequest, Virtual, Visibility};
use crate::effects::{Border, DropShadow, Glow};
use crate::hierarchy::{compose_with_parent, EffectiveOpacity, Opacity};
use crate::image::BakedImage;
use crate::text::{BakedText, Text};
use crate::transition::{ActiveTransition, TransitionConfig};

// ---------------------------------------------------------------------------
// TransitionAtlasSize
// ---------------------------------------------------------------------------

/// The transition atlas's size in pixels, as configured, for turning atlas
/// regions into texture coordinates. The renderer sets it when it starts; until
/// then it holds the default size.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionAtlasSize(pub u32);

impl Default for TransitionAtlasSize {
    fn default() -> Self {
        Self(DEFAULT_TRANSITION_ATLAS_SIZE)
    }
}

// ---------------------------------------------------------------------------
// transition_atlas reclamation
// ---------------------------------------------------------------------------

/// Frees each transition-atlas region when the component that owns it is
/// removed or its entity is destroyed, however the transition ended.
///
/// This works like [`crate::texture_ref::register_texture_ref_hooks`] for the
/// main atlas: removing the component is the one event that always happens,
/// even if the coordinating entity is destroyed mid-transition.
///
/// Called once, from `ProteusWorld::new`, before any entity exists: `bevy_ecs`
/// panics if a hook is added after the component is in use.
pub fn register_transition_alloc_hooks(world: &mut World) {
    world
        .register_component_hooks::<ActiveGroupTransition>()
        .on_remove(|mut world, ctx| {
            let Some(shared) = world
                .get::<ActiveGroupTransition>(ctx.entity)
                .and_then(|g| g.shared_alloc)
            else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.free_transition_region(shared);
            }
        });

    world
        .register_component_hooks::<BakedTexture>()
        .on_remove(|mut world, ctx| {
            let Some(own) = world.get::<BakedTexture>(ctx.entity).map(|b| b.own_alloc) else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.free_transition_region(own);
            }
        });
}

// ---------------------------------------------------------------------------
// ChildBehaviorFn
// ---------------------------------------------------------------------------

/// A transition config for each piece of a split or merge, in the same order as
/// the request's targets or sources. See `Handle::split_to_with_behavior`.
///
/// It may be shorter than the number of pieces; the rest use the request's
/// `default_config`.
pub type ChildConfigs = Vec<TransitionConfig>;

// ---------------------------------------------------------------------------
// SplitStrategy
// ---------------------------------------------------------------------------

/// How a split turns one entity into several.
#[derive(Debug, Clone)]
pub enum SplitStrategy {
    /// Each target transitions from the source's position to its own, as an
    /// independent 1→1 transition. **Experimental:** only tests use it so far.
    ///
    /// Each target shows its own content throughout; it starts where the
    /// source was, but doesn't look like it. There are no virtual entities and
    /// no GPU work. Use it when you design each target yourself. When the
    /// pieces should look like parts of the source, use one of the other
    /// strategies.
    ///
    /// Completion is reported on each target, since the source has no
    /// transition of its own. See `Handle::on_transition_complete`.
    PerTarget,

    /// The source is cut into strips side by side, left to right, and strip
    /// `i` transitions to target `i`. The source and its children are baked
    /// into one image, so the strips look like pieces of it. The targets are
    /// hidden until every strip arrives.
    Row,

    /// Like [`SplitStrategy::Row`], but the strips are stacked top to bottom.
    Column,

    /// Like [`SplitStrategy::Row`], but the source is cut into a grid of
    /// `cols` by `rows`, filled row by row from the top-left: target 0 gets
    /// the top-left cell.
    ///
    /// Use it when the targets are themselves laid out in a grid, such as a
    /// photo gallery. Each piece then starts from the part of the source in
    /// the same row and column as its target, so the pieces spread outward
    /// rather than crossing each other. The grid must have a cell for every
    /// target: `Handle::split_to` returns an error if `cols × rows` is
    /// smaller than the number of targets.
    Grid {
        /// Columns in the grid.
        cols: usize,
        /// Rows in the grid.
        rows: usize,
    },
}

impl SplitStrategy {
    /// The grid this strategy cuts `n` pieces from, as `(cols, rows)`. `None`
    /// for [`SplitStrategy::PerTarget`], which doesn't cut the source.
    pub fn grid(&self, n: usize) -> Option<(usize, usize)> {
        match *self {
            SplitStrategy::PerTarget => None,
            SplitStrategy::Row => Some((n, 1)),
            SplitStrategy::Column => Some((1, n)),
            SplitStrategy::Grid { cols, rows } => Some((cols, rows)),
        }
    }
}

// ---------------------------------------------------------------------------
// OneToNRequest — 1→N group transition
// ---------------------------------------------------------------------------

/// Added to the source entity to start a split (1→N).
///
/// The source is hidden when the split starts. With
/// [`SplitStrategy::PerTarget`], each target gets its own `TransitionRequest`;
/// otherwise virtual pieces are created. Removing the component before the
/// next tick cancels the split.
#[derive(Component, Debug, Clone)]
pub struct OneToNRequest {
    /// The targets, and the geometry each should end at. Target 0 gets the
    /// first piece.
    pub targets: Vec<GroupTarget>,
    /// The transition config for any target `child_configs` doesn't cover.
    pub default_config: TransitionConfig,
    /// A config for each target, in the same order as `targets`.
    pub child_configs: Option<ChildConfigs>,
    /// How the source is divided.
    pub strategy: SplitStrategy,
}

/// One target of a split.
#[derive(Debug, Clone)]
pub struct GroupTarget {
    /// The target entity. Hidden until the pieces arrive, except with
    /// [`SplitStrategy::PerTarget`], where it transitions itself.
    pub entity: Entity,
    /// The geometry it ends at.
    pub state: QuadState,
}

// ---------------------------------------------------------------------------
// NToOneRequest — N→1 group transition
// ---------------------------------------------------------------------------

/// Added to the destination entity to start a merge (N→1).
///
/// The sources are hidden when the merge starts, and the destination until it
/// completes. One virtual piece per source moves from the source's geometry to
/// its part of the destination; [`MergeLayout`] decides which part.
#[derive(Component, Debug, Clone)]
pub struct NToOneRequest {
    /// The sources, and the geometry each starts from. The caller supplies
    /// it, since the setup system can't read other entities' geometry.
    pub sources: Vec<GroupSource>,
    /// The transition config for any source `child_configs` doesn't cover.
    pub default_config: TransitionConfig,
    /// A config for each source, in the same order as `sources`.
    pub child_configs: Option<ChildConfigs>,
    /// Which part of the destination each source moves toward.
    pub layout: MergeLayout,
}

/// How a merge divides the destination among its sources.
#[derive(Debug, Clone, Copy)]
pub enum MergeLayout {
    /// Strips side by side, left to right: source `i` moves to strip `i`.
    Row,

    /// Strips stacked top to bottom: source `i` moves to strip `i`.
    Column,

    /// A grid of `cols` by `rows`, filled row by row from the top-left, like
    /// [`SplitStrategy::Grid`]. Use it when the sources are laid out in a
    /// grid, so each moves toward the matching cell. The grid must have a
    /// cell for every source: `Handle::merge_from` returns an error if
    /// `cols × rows` is smaller than the number of sources.
    Grid {
        /// Columns in the grid.
        cols: usize,
        /// Rows in the grid.
        rows: usize,
    },
}

impl MergeLayout {
    /// The grid this layout divides the destination into for `n` sources, as
    /// `(cols, rows)`.
    pub fn grid(&self, n: usize) -> (usize, usize) {
        match *self {
            MergeLayout::Row => (n, 1),
            MergeLayout::Column => (1, n),
            MergeLayout::Grid { cols, rows } => (cols, rows),
        }
    }
}

/// One source of a merge.
#[derive(Debug, Clone)]
pub struct GroupSource {
    /// The source entity, hidden when the merge starts.
    pub entity: Entity,
    /// The geometry it starts from.
    pub state: QuadState,
}

// ---------------------------------------------------------------------------
// ActiveGroupTransition: the coordinator's state during a split or merge
// ---------------------------------------------------------------------------

/// On the entity coordinating a split or merge, while it runs: the source of a
/// split, or the destination of a merge. `group_transition_complete_system`
/// finishes the group once all its pieces have arrived.
#[derive(Component, Debug)]
pub struct ActiveGroupTransition {
    /// The entities to show when every piece has arrived.
    pub reveal_on_complete: Vec<Entity>,
    /// The bake every piece shows part of: the source's for a split, the
    /// destination's for a merge. Each piece's own bake is on its
    /// `BakedTexture`. `None` when baking wasn't possible and the pieces are
    /// plain colored shapes. Freed when this component is removed.
    pub shared_alloc: Option<TransitionAllocId>,
}

// ---------------------------------------------------------------------------
// PartOfGroup: links a piece to its coordinator
// ---------------------------------------------------------------------------

/// Links a virtual piece to the entity coordinating its split or merge, so the
/// group can be finished once all its pieces have arrived.
#[derive(Component, Debug, Clone)]
pub struct PartOfGroup(pub Entity);

// ---------------------------------------------------------------------------
// Slice geometry helpers
// ---------------------------------------------------------------------------

/// Divides `source` into `n` equal strips, side by side from left to right:
/// a grid of `n` columns and one row. See [`grid_slices`].
///
/// # Panics
/// Panics if `n == 0`.
///
/// # Example
/// A 300×100 source at (0, 0) divided into 3 strips:
/// - Strip 0: 100×100 at (-100, 0)
/// - Strip 1: 100×100 at (  0, 0)
/// - Strip 2: 100×100 at ( 100, 0)
pub fn row_slices(source: &QuadState, n: usize) -> Vec<QuadState> {
    grid_slices(source, n, 1)
}

/// Divides `source` into `n` equal strips, stacked from top to bottom: a grid
/// of one column and `n` rows. See [`grid_slices`].
///
/// # Panics
/// Panics if `n == 0`.
pub fn column_slices(source: &QuadState, n: usize) -> Vec<QuadState> {
    grid_slices(source, 1, n)
}

/// Divides `source` into an equal grid of `cols` by `rows`, row by row: cell
/// `row * cols + col`, with row 0 at the top. Used by every split strategy
/// except [`SplitStrategy::PerTarget`], and by every merge layout.
///
/// Each cell's corner radius is clamped to half its size. A round source's
/// radius is half its width, far larger than a narrow cell's, and would shrink
/// the cell's rounded rectangle to a sliver.
///
/// # Panics
/// Panics if `cols == 0` or `rows == 0`.
pub fn grid_slices(source: &QuadState, cols: usize, rows: usize) -> Vec<QuadState> {
    assert!(
        cols > 0 && rows > 0,
        "grid_slices: cols and rows must be > 0"
    );
    let cell_w = source.size.x / cols as f32;
    let cell_h = source.size.y / rows as f32;
    let leftmost_center = source.position.x - source.size.x * 0.5 + cell_w * 0.5;
    let topmost_center = source.position.y + source.size.y * 0.5 - cell_h * 0.5;
    let corner_radius = source.corner_radius.min(cell_w * 0.5).min(cell_h * 0.5);
    (0..rows)
        .flat_map(move |row| {
            let y = topmost_center - cell_h * row as f32;
            (0..cols).map(move |col| {
                let x = leftmost_center + cell_w * col as f32;
                QuadState {
                    position: glam::Vec3::new(x, y, source.position.z),
                    size: glam::Vec2::new(cell_w, cell_h),
                    corner_radius,
                    ..source.clone()
                }
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Baking for splits and merges
// ---------------------------------------------------------------------------

/// The visual components needed to bake an entity's appearance, besides its
/// `QuadState`. Also used by `bake::bake_system`, which bakes permanently.
pub(crate) type BakeVisualsQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Border>,
        Option<&'static Glow>,
        Option<&'static DropShadow>,
        Option<&'static BakedText>,
        Option<&'static Text>,
        Option<&'static BakedImage>,
        Option<&'static EffectiveOpacity>,
        Option<&'static Opacity>,
    ),
>;

/// Builds the `QuadInstance`s that draw `entity` and all its descendants: for
/// each, its background and any text, as `collect_instances` would, with each
/// child placed in the world by [`compose_with_parent`].
///
/// Baking a component therefore includes its children, such as a button's
/// text label. Also used by `bake::bake_system`, which bakes permanently.
pub(crate) fn gather_bake_instances(
    visuals: &BakeVisualsQuery,
    children_q: &Query<&Children>,
    quad_states: &Query<&QuadState>,
    entity: Entity,
    qs: &QuadState,
) -> Vec<QuadInstance> {
    let Ok((border, glow, shadow, baked_text, text, baked_image, effective_opacity, opacity)) =
        visuals.get(entity)
    else {
        return vec![quad_state_to_instance(qs, None, None, None, None)];
    };
    // The cascaded opacity if computed, else the entity's own, else opaque,
    // as `push_entity_instances` does. Otherwise a faded entity would bake at
    // full opacity and snap to its real opacity when the transition ends.
    let effective_opacity = effective_opacity
        .map(|o| o.0)
        .unwrap_or_else(|| opacity.map(|o| o.0).unwrap_or(1.0));

    let mut bg_inst = quad_state_to_instance(qs, None, shadow, glow, border);
    bg_inst.opacity = effective_opacity;
    // Draw the entity's image, as `push_entity_instances` does, or a target
    // would bake as its plain color. Set the atlas page too: the image may be
    // on any page of the main atlas.
    if let Some(image) = baked_image {
        bg_inst.uv_offset = image.uv_offset;
        bg_inst.uv_scale = image.uv_scale;
        bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_MAIN, image.page);
    }
    let mut out = vec![bg_inst];

    if let Some(b) = baked_text {
        let mut text_qs = qs.clone();
        text_qs.color = text.map(|t| t.color).unwrap_or(Vec4::ONE);
        // Sized and squared off like the text overlay in collect.rs.
        text_qs.size = b.pixel_size.into();
        text_qs.corner_radius = 0.0;
        let mut text_inst = quad_state_to_instance(&text_qs, Some(b), None, None, None);
        text_inst.opacity = effective_opacity;
        out.push(text_inst);
    }

    // Every descendant, not only direct children, as in collect_instances.
    if let Ok(children) = children_q.get(entity) {
        for child in children.iter() {
            let child_local = quad_states.get(child).cloned().unwrap_or_default();
            let child_world = compose_with_parent(qs, &child_local);
            out.extend(gather_bake_instances(
                visuals,
                children_q,
                quad_states,
                child,
                &child_world,
            ));
        }
    }

    out
}

/// A transition-atlas region as `(uv_offset, uv_scale)`.
fn region_uv(region: &TransitionRegion, atlas_size: f32) -> ([f32; 2], [f32; 2]) {
    let atlas = atlas_size;
    (
        [region.x as f32 / atlas, region.y as f32 / atlas],
        [region.width as f32 / atlas, region.height as f32 / atlas],
    )
}

/// Divides a baked region into a grid of `cols` by `rows`, row by row, in
/// texture coordinates: the texture part of each of [`grid_slices`]' pieces.
/// Row 0 is the top of the bake, which is the top of the source, since baking
/// doesn't flip the image.
fn region_uv_grid_slices(
    region: &TransitionRegion,
    cols: usize,
    rows: usize,
    atlas_size: f32,
) -> Vec<([f32; 2], [f32; 2])> {
    let atlas = atlas_size;
    let cell_w = region.width as f32 / cols as f32;
    let cell_h = region.height as f32 / rows as f32;
    (0..rows)
        .flat_map(|row| {
            let y = region.y as f32 + cell_h * row as f32;
            (0..cols).map(move |col| {
                let x = region.x as f32 + cell_w * col as f32;
                ([x / atlas, y / atlas], [cell_w / atlas, cell_h / atlas])
            })
        })
        .collect()
}

/// Bakes `entity`'s appearance, at `qs`, into a new transition-atlas region.
/// Returns `None` if there is nothing to bake or the atlas is full, in which
/// case the caller uses a plain colored shape instead.
fn bake_one(
    pipeline: &mut QuadPipeline,
    gpu: &GpuContext,
    visuals: &BakeVisualsQuery,
    children_q: &Query<&Children>,
    quad_states: &Query<&QuadState>,
    entity: Entity,
    qs: &QuadState,
) -> Option<(TransitionAllocId, TransitionRegion)> {
    let instances = gather_bake_instances(visuals, children_q, quad_states, entity, qs);
    if instances.is_empty() {
        return None;
    }

    let width = qs.size.x.max(1.0).ceil() as u32;
    let height = qs.size.y.max(1.0).ceil() as u32;
    let (alloc_id, granted) = pipeline.allocate_transition_region(width, height)?;

    // The allocator may grant a larger region than requested. Use only the
    // requested size, so the slices' texture coordinates stay exact.
    let region = TransitionRegion {
        x: granted.x,
        y: granted.y,
        width,
        height,
    };
    let view_projection =
        QuadPipeline::ortho_centered(qs.position.x, qs.position.y, qs.size.x, qs.size.y);
    pipeline.bake_instances_to_transition_atlas(
        &gpu.device,
        &gpu.queue,
        &instances,
        view_projection,
        region.as_tuple(),
    );

    Some((alloc_id, region))
}

// ---------------------------------------------------------------------------
// one_to_n_setup_system
// ---------------------------------------------------------------------------

/// Starts each split requested with [`OneToNRequest`].
///
/// With [`SplitStrategy::PerTarget`]: hides the source, and gives each target a
/// [`TransitionRequest`] from the source's geometry to its own.
///
/// With `Row`, `Column` or `Grid`: hides the source and the targets, creates one
/// [`Virtual`] piece per target, starting from its part of the source, and
/// adds [`ActiveGroupTransition`] to the source. When GPU resources are
/// present, it bakes the source once and each target once, and each piece
/// fades from its part of the source's bake to its target's bake. Without
/// them, or if a bake fails, a piece is a plain colored shape.
#[allow(clippy::too_many_arguments)]
pub fn one_to_n_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &OneToNRequest, &QuadState)>,
    visuals: BakeVisualsQuery,
    children_q: Query<&Children>,
    quad_states: Query<&QuadState>,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
    atlas_size: Res<TransitionAtlasSize>,
) {
    let atlas_size = atlas_size.0 as f32;
    for (source_entity, request, source_state) in query.iter() {
        let n = request.targets.len();

        // Always remove the request so it isn't re-processed.
        commands.entity(source_entity).remove::<OneToNRequest>();

        if n == 0 {
            continue;
        }
        if let Some((cols, rows)) = request.strategy.grid(n) {
            if cols * rows < n {
                // `Handle::split_to` rejects this; a request made directly
                // through the ECS is skipped, not started with pieces missing.
                log::warn!(
                    "split of {source_entity:?}: the grid has {} cells for {n} targets — skipped",
                    cols * rows
                );
                continue;
            }
        }

        // Hide the source: the targets replace it.
        commands.entity(source_entity).insert(Visibility::HIDDEN);

        match request.strategy {
            SplitStrategy::PerTarget => {
                // One independent 1→1 transition per target, from the source's
                // geometry to its own.
                for (i, target) in request.targets.iter().enumerate() {
                    let cfg = request
                        .child_configs
                        .as_ref()
                        .and_then(|c| c.get(i).copied())
                        .unwrap_or(request.default_config);

                    commands.entity(target.entity).insert(TransitionRequest {
                        to: target.state.clone(),
                        from_state: Some(source_state.clone()),
                        config: cfg,
                    });
                }

                // The source has no transition of its own, so it goes Idle.
                commands.entity(source_entity).insert(Lifecycle::Idle);
            }

            SplitStrategy::Row | SplitStrategy::Column | SplitStrategy::Grid { .. } => {
                let (cols, rows) = request.strategy.grid(n).expect("not PerTarget");
                // Hide all target entities until the transition completes.
                for target in &request.targets {
                    commands.entity(target.entity).insert(Visibility::HIDDEN);
                }
                let reveal: Vec<Entity> = request.targets.iter().map(|t| t.entity).collect();

                // Bake the source once, shared by every piece, and each target
                // once. Only if GPU resources are present and the source bake
                // succeeds, since the pieces are cut from it.
                let mut shared_alloc: Option<TransitionAllocId> = None;
                let mut from_uv_slices: Vec<([f32; 2], [f32; 2])> = Vec::new();
                let mut target_bakes: Vec<Option<(TransitionAllocId, TransitionRegion)>> =
                    Vec::new();

                if let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) {
                    if let Some((src_id, src_region)) = bake_one(
                        pipeline,
                        gpu,
                        &visuals,
                        &children_q,
                        &quad_states,
                        source_entity,
                        source_state,
                    ) {
                        shared_alloc = Some(src_id);
                        from_uv_slices = region_uv_grid_slices(&src_region, cols, rows, atlas_size);
                        target_bakes = request
                            .targets
                            .iter()
                            .map(|t| {
                                bake_one(
                                    pipeline,
                                    gpu,
                                    &visuals,
                                    &children_q,
                                    &quad_states,
                                    t.entity,
                                    &t.state,
                                )
                            })
                            .collect();
                    }
                }

                // The source's visual components, for any piece that isn't
                // baked.
                let src_glow = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, g, _, _, _, _, _, _)| g.cloned());
                let src_shadow = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, _, s, _, _, _, _, _)| s.cloned());
                let src_baked = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, _, _, b, _, _, _, _)| b.cloned());

                let slices = grid_slices(source_state, cols, rows);

                // One virtual piece per target.
                for (i, (slice_state, target)) in
                    slices.iter().zip(request.targets.iter()).enumerate()
                {
                    let cfg = request
                        .child_configs
                        .as_ref()
                        .and_then(|c| c.get(i).copied())
                        .unwrap_or(request.default_config);

                    let own_bake = target_bakes.get(i).copied().flatten();
                    let (from_state, to_state, baked_texture) = match (shared_alloc, own_bake) {
                        (Some(_), Some((own_id, own_region))) => {
                            let (from_off, from_scale) = from_uv_slices[i];
                            let (to_off, to_scale) = region_uv(&own_region, atlas_size);
                            // Both ends are white and untinted: the baked
                            // image has the real colors.
                            //
                            // The corner radius differs between the ends. The
                            // start is a slice of the source's bake, and only
                            // slices at the source's corners should be
                            // rounded, so its radius stays 0 and the bake's
                            // own rounded edges show. The end is a whole
                            // target, so it uses the target's real radius.
                            // That also keeps its corners round while it is
                            // small, where the bake alone would look jagged,
                            // since the atlases have no smaller versions of
                            // each image (mipmaps).
                            let from_state = QuadState {
                                color: Vec4::ONE,
                                corner_radius: 0.0,
                                ..slice_state.clone()
                            };
                            let to_state = QuadState {
                                color: Vec4::ONE,
                                ..target.state.clone()
                            };
                            let baked_texture = BakedTexture {
                                from_uv_offset: from_off,
                                from_uv_scale: from_scale,
                                to_uv_offset: to_off,
                                to_uv_scale: to_scale,
                                own_alloc: own_id,
                            };
                            (from_state, to_state, Some(baked_texture))
                        }
                        _ => (slice_state.clone(), target.state.clone(), None),
                    };

                    let active = ActiveTransition::new(from_state.clone(), to_state, cfg);

                    let mut entity_cmd = commands.spawn((
                        from_state,
                        Lifecycle::Transitioning,
                        active,
                        Virtual,
                        PartOfGroup(source_entity),
                    ));
                    if let Some(bt) = baked_texture {
                        entity_cmd.insert(bt);
                    } else {
                        // Not baked: carry the source's own visual
                        // components.
                        if let Some(ref s) = src_shadow {
                            entity_cmd.insert(s.clone());
                        }
                        if let Some(ref g) = src_glow {
                            entity_cmd.insert(g.clone());
                        }
                        if let Some(ref b) = src_baked {
                            entity_cmd.insert(b.clone());
                        }
                    }
                }

                // The source coordinates the group.
                commands.entity(source_entity).insert((
                    Lifecycle::Transitioning,
                    ActiveGroupTransition {
                        reveal_on_complete: reveal,
                        shared_alloc,
                    },
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// n_to_one_setup_system
// ---------------------------------------------------------------------------

/// Starts each merge requested with [`NToOneRequest`].
///
/// Hides the sources and the destination, creates one [`Virtual`] piece per
/// source, moving from the source's geometry to its part of the destination,
/// and adds [`ActiveGroupTransition`] to the destination, which is shown when
/// the merge completes.
///
/// Baking mirrors [`one_to_n_setup_system`]: each source is baked, and the
/// destination is baked once and shared, each piece fading from its source's
/// bake to its part of the destination's. Without GPU resources, or if a bake
/// fails, a piece is a plain colored shape.
#[allow(clippy::too_many_arguments)]
pub fn n_to_one_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &NToOneRequest, &QuadState)>,
    visuals: BakeVisualsQuery,
    children_q: Query<&Children>,
    quad_states: Query<&QuadState>,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
    atlas_size: Res<TransitionAtlasSize>,
) {
    let atlas_size = atlas_size.0 as f32;
    for (dest_entity, request, dest_state) in query.iter() {
        let n = request.sources.len();

        commands.entity(dest_entity).remove::<NToOneRequest>();

        if n == 0 {
            continue;
        }
        let (cols, rows) = request.layout.grid(n);
        if cols * rows < n {
            // `Handle::merge_from` rejects this; a request made directly
            // through the ECS is skipped, not started with sources missing.
            log::warn!(
                "merge into {dest_entity:?}: the grid has {} cells for {n} sources — skipped",
                cols * rows
            );
            continue;
        }

        // Hide the destination until the merge completes.
        commands
            .entity(dest_entity)
            .insert(Visibility::HIDDEN)
            .insert(Lifecycle::Transitioning);

        // Hide all source entities.
        for source in &request.sources {
            commands.entity(source.entity).insert(Visibility::HIDDEN);
        }

        // The part of the destination each source moves to.
        let target_slices = grid_slices(dest_state, cols, rows);

        // Bake the destination once, shared by every piece, and each source
        // once, as one_to_n_setup_system does.
        let mut shared_alloc: Option<TransitionAllocId> = None;
        let mut to_uv_slices: Vec<([f32; 2], [f32; 2])> = Vec::new();
        let mut source_bakes: Vec<Option<(TransitionAllocId, TransitionRegion)>> = Vec::new();

        if let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) {
            if let Some((dest_id, dest_region)) = bake_one(
                pipeline,
                gpu,
                &visuals,
                &children_q,
                &quad_states,
                dest_entity,
                dest_state,
            ) {
                shared_alloc = Some(dest_id);
                to_uv_slices = region_uv_grid_slices(&dest_region, cols, rows, atlas_size);
                source_bakes = request
                    .sources
                    .iter()
                    .map(|s| {
                        bake_one(
                            pipeline,
                            gpu,
                            &visuals,
                            &children_q,
                            &quad_states,
                            s.entity,
                            &s.state,
                        )
                    })
                    .collect();
            }
        }

        // One virtual piece per source, moving to its part of the destination.
        for (i, (source, target_slice)) in
            request.sources.iter().zip(target_slices.iter()).enumerate()
        {
            let cfg = request
                .child_configs
                .as_ref()
                .and_then(|c| c.get(i).copied())
                .unwrap_or(request.default_config);

            let own_bake = source_bakes.get(i).copied().flatten();
            let (from_state, to_state, baked_texture) = match (shared_alloc, own_bake) {
                (Some(_), Some((own_id, own_region))) => {
                    let (to_off, to_scale) = to_uv_slices[i];
                    let (from_off, from_scale) = region_uv(&own_region, atlas_size);
                    // The corner radius, reversed from one_to_n_setup_system:
                    // the start is a whole source, so it uses the source's
                    // real radius; the end is a slice of the destination's
                    // bake, so its radius stays 0.
                    let from_state = QuadState {
                        color: Vec4::ONE,
                        ..source.state.clone()
                    };
                    let to_state = QuadState {
                        color: Vec4::ONE,
                        corner_radius: 0.0,
                        ..target_slice.clone()
                    };
                    let baked_texture = BakedTexture {
                        from_uv_offset: from_off,
                        from_uv_scale: from_scale,
                        to_uv_offset: to_off,
                        to_uv_scale: to_scale,
                        own_alloc: own_id,
                    };
                    (from_state, to_state, Some(baked_texture))
                }
                _ => (source.state.clone(), target_slice.clone(), None),
            };

            let active = ActiveTransition::new(from_state.clone(), to_state, cfg);

            let mut entity_cmd = commands.spawn((
                from_state,
                Lifecycle::Transitioning,
                active,
                Virtual,
                PartOfGroup(dest_entity),
            ));

            if let Some(bt) = baked_texture {
                entity_cmd.insert(bt);
            } else if let Ok((_, glow, shadow, baked, _, _, _, _)) = visuals.get(source.entity) {
                // Not baked: carry the source's own visual components.
                if let Some(s) = shadow {
                    entity_cmd.insert(s.clone());
                }
                if let Some(g) = glow {
                    entity_cmd.insert(g.clone());
                }
                if let Some(b) = baked {
                    entity_cmd.insert(b.clone());
                }
            }
        }

        // The destination coordinates the group.
        commands.entity(dest_entity).insert(ActiveGroupTransition {
            reveal_on_complete: vec![dest_entity],
            shared_alloc,
        });
    }
}

// ---------------------------------------------------------------------------
// group_transition_complete_system
// ---------------------------------------------------------------------------

/// Finishes each split or merge whose pieces have all arrived. Runs after
/// `transition_complete_system`.
///
/// For each finished group, it:
///
/// 1. shows the `reveal_on_complete` entities;
/// 2. records the coordinator in `CompletedTransitions`;
/// 3. returns the coordinator to `Idle` and removes its
///    `ActiveGroupTransition`;
/// 4. removes the virtual pieces.
///
/// Removing the component and the pieces frees their transition-atlas regions,
/// through the hooks from `register_transition_alloc_hooks`.
///
/// If the coordinator was destroyed mid-transition, its pieces are removed
/// without finishing the group.
pub fn group_transition_complete_system(
    mut commands: Commands,
    virtuals: Query<
        (
            Entity,
            &PartOfGroup,
            &ActiveTransition,
            Option<&BakedTexture>,
        ),
        With<Virtual>,
    >,
    mut coordinators: Query<(&ActiveGroupTransition, &mut Lifecycle)>,
    mut completed: ResMut<crate::transition::CompletedTransitions>,
) {
    // Build a map: coordinator_entity → (all_virtual_entities, complete_count).
    type CoordEntry = (Vec<(Entity, Option<TransitionAllocId>)>, usize);
    let mut by_coord: HashMap<Entity, CoordEntry> = HashMap::new();

    for (v_entity, PartOfGroup(coord_entity), active, baked) in virtuals.iter() {
        let entry = by_coord.entry(*coord_entity).or_insert((vec![], 0));
        entry.0.push((v_entity, baked.map(|b| b.own_alloc)));
        if active.is_complete {
            entry.1 += 1;
        }
    }

    for (coord_entity, (v_entities, complete_count)) in by_coord {
        let total = v_entities.len();

        // The coordinator was destroyed, so the group can't be finished.
        // Remove its pieces rather than leave them frozen on screen. Their
        // atlas regions are freed by the removal hooks.
        if !coordinators.contains(coord_entity) {
            log::warn!(
                "group transition coordinator {coord_entity:?} disappeared with {} virtual(s) \
                 still in flight — cleaning them up",
                v_entities.len()
            );
            for (v_entity, _) in v_entities {
                commands.entity(v_entity).despawn();
            }
            continue;
        }

        if complete_count < total {
            continue; // still waiting on some virtuals
        }

        // Every piece has arrived: finish the group.
        let Ok((group, mut lifecycle)) = coordinators.get_mut(coord_entity) else {
            continue;
        };

        // Show the real entities.
        for &entity in &group.reveal_on_complete {
            commands.entity(entity).insert(Visibility::VISIBLE);
        }

        // Atlas regions aren't freed here: removing `ActiveGroupTransition`
        // and the pieces does that, through the removal hooks. Freeing here too
        // would free them twice.

        // Record one completion for the group, on its coordinator: the source
        // of a split, the destination of a merge. The pieces don't count.
        // `transition_complete_system` has already run and cleared the list
        // this tick, so appending is safe.
        completed.entities.push(coord_entity);

        // Return the coordinator to Idle and remove the group's state.
        *lifecycle = Lifecycle::Idle;
        commands
            .entity(coord_entity)
            .remove::<ActiveGroupTransition>();

        // Remove the pieces.
        for (v_entity, _) in v_entities {
            commands.entity(v_entity).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec2, Vec3, Vec4};

    fn source() -> QuadState {
        QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(300.0, 100.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 0.0, 0.0, 1.0),
            corner_radius: 0.0,
        }
    }

    // A column is the row turned on its side: full-width strips, top to
    // bottom, with strip 0 at the top (highest y).
    #[test]
    fn column_slices_stack_full_width_strips_from_the_top() {
        let slices = column_slices(&source(), 4);
        assert_eq!(slices.len(), 4);
        for s in &slices {
            assert!((s.size.x - 300.0).abs() < 1e-4);
            assert!((s.size.y - 25.0).abs() < 1e-4);
            assert!(s.position.x.abs() < 1e-4);
        }
        assert!((slices[0].position.y - 37.5).abs() < 1e-4);
        assert!((slices[3].position.y + 37.5).abs() < 1e-4);
    }

    #[test]
    fn row_slices_count() {
        let slices = row_slices(&source(), 5);
        assert_eq!(slices.len(), 5);
    }

    #[test]
    fn row_slices_width() {
        let slices = row_slices(&source(), 5);
        for s in &slices {
            assert!(
                (s.size.x - 60.0).abs() < 1e-4,
                "each slice should be 60px wide"
            );
        }
    }

    #[test]
    fn row_slices_height_preserved() {
        let slices = row_slices(&source(), 5);
        for s in &slices {
            assert!((s.size.y - 100.0).abs() < 1e-4);
        }
    }

    #[test]
    fn row_slices_positions_span_source() {
        // The leftmost slice center should be at x = -120 and rightmost at x = +120
        // for a 300px-wide source centered at 0.
        let slices = row_slices(&source(), 5);
        let xs: Vec<f32> = slices.iter().map(|s| s.position.x).collect();
        assert!((xs[0] - (-120.0)).abs() < 1e-3, "leftmost x={}", xs[0]);
        assert!((xs[4] - 120.0).abs() < 1e-3, "rightmost x={}", xs[4]);
    }

    #[test]
    fn row_slices_no_gap_no_overlap() {
        // Adjacent slice centers should be exactly slice_width apart.
        let slices = row_slices(&source(), 5);
        let slice_w = 300.0 / 5.0; // 60.0
        for i in 1..slices.len() {
            let gap = slices[i].position.x - slices[i - 1].position.x;
            assert!((gap - slice_w).abs() < 1e-3, "gap={}", gap);
        }
    }

    // A baked target must show its image, not its plain color.
    #[test]
    fn gather_bake_instances_includes_baked_image_uv() {
        use crate::image::BakedImage;
        use bevy_ecs::system::SystemState;

        let mut world = World::new();
        let entity = world
            .spawn((
                source(),
                BakedImage::new([0.4, 0.5], [0.2, 0.3], 0, [400.0, 600.0]),
            ))
            .id();

        let mut state: SystemState<(BakeVisualsQuery, Query<&Children>, Query<&QuadState>)> =
            SystemState::new(&mut world);
        let (visuals, children_q, quad_states) = state.get(&world);

        let instances =
            gather_bake_instances(&visuals, &children_q, &quad_states, entity, &source());
        assert_eq!(instances.len(), 1, "no BakedText — just the background");
        assert_eq!(instances[0].uv_offset, [0.4, 0.5]);
        assert_eq!(instances[0].uv_scale, [0.2, 0.3]);
    }

    // A baked image must keep its atlas page, or an image on any page but the
    // first bakes whatever is on the first page.
    #[test]
    fn gather_bake_instances_carries_the_baked_images_page() {
        use crate::image::BakedImage;
        use bevy_ecs::system::SystemState;

        let mut world = World::new();
        let entity = world
            .spawn((
                source(),
                BakedImage::new([0.4, 0.5], [0.2, 0.3], 3, [400.0, 600.0]),
            ))
            .id();

        let mut state: SystemState<(BakeVisualsQuery, Query<&Children>, Query<&QuadState>)> =
            SystemState::new(&mut world);
        let (visuals, children_q, quad_states) = state.get(&world);

        let instances =
            gather_bake_instances(&visuals, &children_q, &quad_states, entity, &source());
        let (selector, page) = proteus_render::unpack_atlas_page(instances[0].atlas_page);
        assert_eq!(selector, ATLAS_SELECTOR_MAIN);
        assert_eq!(page, 3);
    }

    // Baking must include every descendant, such as a button's label. Three
    // levels deep, to show it isn't limited to direct children.
    #[test]
    fn gather_bake_instances_walks_every_descendant() {
        use bevy_ecs::hierarchy::ChildOf;
        use bevy_ecs::system::SystemState;

        let mut world = World::new();
        let root = world.spawn(source()).id();
        let child = world
            .spawn((
                QuadState {
                    color: Vec4::new(0.0, 1.0, 0.0, 1.0),
                    ..Default::default()
                },
                BakedText {
                    uv_offset: [0.1, 0.1],
                    uv_scale: [0.2, 0.2],
                    page: 0,
                    pixel_size: [50.0, 20.0],
                },
                Text::new("child", 16.0),
                ChildOf(root),
            ))
            .id();
        let _grandchild = world
            .spawn((
                QuadState {
                    color: Vec4::new(0.0, 0.0, 1.0, 1.0),
                    ..Default::default()
                },
                BakedImage::new([0.6, 0.6], [0.1, 0.1], 0, [10.0, 10.0]),
                ChildOf(child),
            ))
            .id();

        let mut state: SystemState<(BakeVisualsQuery, Query<&Children>, Query<&QuadState>)> =
            SystemState::new(&mut world);
        let (visuals, children_q, quad_states) = state.get(&world);

        let instances = gather_bake_instances(&visuals, &children_q, &quad_states, root, &source());

        // root: 1 background instance (no BakedText on root).
        // child: 1 background + 1 text overlay (BakedText present).
        // grandchild: 1 background (BakedImage, no BakedText).
        assert_eq!(
            instances.len(),
            4,
            "expected root bg + child bg + child text overlay + grandchild bg, got {}",
            instances.len()
        );
    }

    #[test]
    fn row_slices_preserves_color_and_radius() {
        let slices = row_slices(&source(), 3);
        for s in &slices {
            assert_eq!(s.color, Vec4::new(1.0, 0.0, 0.0, 1.0));
            assert_eq!(s.corner_radius, 0.0);
        }
    }

    // Slicing a round source must clamp each slice's corner radius; see
    // `row_slices`.
    #[test]
    fn row_slices_clamps_corner_radius_to_slice_half_extents() {
        let circle = QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(200.0, 200.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: 100.0, // full circle: radius == half the width
        };
        let slices = row_slices(&circle, 3);
        let slice_half_width = (200.0 / 3.0) / 2.0;
        for s in &slices {
            assert!(
                s.corner_radius <= slice_half_width + 1e-4,
                "corner_radius {} exceeds slice half-width {slice_half_width}",
                s.corner_radius,
            );
            assert!(s.corner_radius > 0.0, "clamping should not zero it out");
        }
    }

    #[test]
    fn grid_slices_count_and_size() {
        // 300x100 source divided into a 3x2 grid: each cell 100x50.
        let slices = grid_slices(&source(), 3, 2);
        assert_eq!(slices.len(), 6);
        for s in &slices {
            assert!((s.size.x - 100.0).abs() < 1e-4, "cell width {}", s.size.x);
            assert!((s.size.y - 50.0).abs() < 1e-4, "cell height {}", s.size.y);
        }
    }

    #[test]
    fn grid_slices_row_major_order() {
        // Row 0 (top, highest Y) comes first, left-to-right, then row 1.
        let slices = grid_slices(&source(), 3, 2);
        let xs: Vec<f32> = slices.iter().map(|s| s.position.x).collect();
        let ys: Vec<f32> = slices.iter().map(|s| s.position.y).collect();
        assert!(
            (xs[0] - (-100.0)).abs() < 1e-3,
            "index 0 (top-left) x={}",
            xs[0]
        );
        assert!(
            (xs[2] - 100.0).abs() < 1e-3,
            "index 2 (top-right) x={}",
            xs[2]
        );
        assert!(
            (xs[3] - (-100.0)).abs() < 1e-3,
            "index 3 (row 1, leftmost) x={}",
            xs[3]
        );
        assert!(ys[0] > ys[3], "row 0 should be above row 1 (Y-up)");
        assert_eq!(ys[0], ys[1], "row 0 is at one shared Y");
        assert_eq!(ys[3], ys[4], "row 1 is at one shared Y");
    }

    #[test]
    #[should_panic(expected = "cols and rows must be > 0")]
    fn grid_slices_rejects_zero_cols() {
        grid_slices(&source(), 0, 3);
    }

    #[test]
    #[should_panic(expected = "cols and rows must be > 0")]
    fn grid_slices_rejects_zero_rows() {
        grid_slices(&source(), 3, 0);
    }
}
