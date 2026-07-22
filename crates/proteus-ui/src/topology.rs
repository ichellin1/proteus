//! Transition topology — 1→N and N→1 mechanics.
//!
//! Proteus supports three transition topologies:
//!
//! - **1→1** — a single entity morphs into another. Implemented in `transition.rs`.
//! - **1→N** — one source entity splits into N targets. [`OneToNRequest`].
//! - **N→1** — N source entities converge into one target. [`NToOneRequest`].
//!
//! ## How group transitions work
//!
//! For both topologies the framework creates [`Virtual`] entities that carry
//! `ActiveTransition` components and are ticked by the normal
//! `transition_tick_system`.  Virtual entities also inherit the visual
//! components ([`DropShadow`], [`Glow`], [`BakedText`]) from their source
//! entity so that effects and text persist correctly through the transition
//! rather than popping in or out at completion.  When all virtuals in a group complete,
//! [`group_transition_complete_system`] reveals the real target entities,
//! despawns the virtuals, and restores the coordinator's `Lifecycle` to `Idle`.
//!
//! ## Strategy variants
//!
//! For 1→N, two strategies are available via [`SplitStrategy`]:
//!
//! - **Bake** — normalize to N independent 1→1 transitions. Each target
//!   animates directly from the source geometry to its own geometry. No virtual
//!   entities are created; `transition_complete_system` handles each target's
//!   completion normally.
//! - **Slice** — divide the source into N equal horizontal slices. One virtual
//!   entity per slice animates to the corresponding target. Real target entities
//!   are hidden during the transition and revealed on group completion.
//!
//! For N→1, only the Slice strategy is implemented in M3. Each source entity's
//! geometry is treated as a "slice" that animates to its portion of the target.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{
    GpuContext, QuadInstance, QuadPipeline, TransitionAllocId, TransitionRegion,
    TRANSITION_ATLAS_SIZE,
};

use crate::collect::{quad_state_to_instance, BakedTexture};
use crate::component::{Lifecycle, QuadState, TransitionRequest, Virtual, Visibility};
use crate::effects::{Border, DropShadow, Glow};
use crate::image::BakedImage;
use crate::text::{BakedText, Text};
use crate::transition::{ActiveTransition, TransitionConfig};

// ---------------------------------------------------------------------------
// ChildBehaviorFn
// ---------------------------------------------------------------------------

/// Per-child transition config function for group transitions.
///
/// Called once per child during group setup. Returns the `TransitionConfig`
/// to apply to that child's virtual entity (or direct target, for bake).
///
/// ```rust,ignore
/// fn stagger(idx: usize, total: usize) -> TransitionConfig {
///     TransitionConfig {
///         duration: 0.4,
///         delay: idx as f32 * 0.08,
///         easing: ease_out_cubic,
///     }
/// }
/// ```
///
/// This is the Rust equivalent of the TypeScript `childBehavior` iterator.
pub type ChildBehaviorFn = fn(idx: usize, total: usize) -> TransitionConfig;

// ---------------------------------------------------------------------------
// SplitStrategy
// ---------------------------------------------------------------------------

/// How a 1→N transition is normalized to a set of 1→1 lerps.
#[derive(Debug, Clone)]
pub enum SplitStrategy {
    /// **Bake** — normalize to 1→1 per target.
    ///
    /// Each of the N target entities receives a `TransitionRequest` whose
    /// `from_state` is set to the source entity's geometry. All N transitions
    /// run simultaneously and independently. No virtual entities are created.
    ///
    /// Visual: N copies of the source fan out to their respective positions.
    Bake,

    /// **Slice** — normalize to N→N via virtual entities.
    ///
    /// The source geometry is divided into N equal horizontal slices. A virtual
    /// entity is spawned for each slice, starting at the slice geometry and
    /// lerping to the corresponding target entity's geometry. Real target
    /// entities are hidden during the transition and revealed on group completion.
    ///
    /// Visual: the source "splits apart" into N pieces that each morph to a target.
    Slice,
}

// ---------------------------------------------------------------------------
// OneToNRequest — 1→N group transition
// ---------------------------------------------------------------------------

/// Added to the **source** entity to trigger a 1→N split transition.
///
/// The source entity goes invisible immediately on setup. Depending on
/// `strategy`, either virtual slice entities are created (Slice) or the
/// target entities are given direct `TransitionRequest`s (Bake).
///
/// Remove the component yourself before the next frame if you decide not to
/// proceed — otherwise `one_to_n_setup_system` will process it.
#[derive(Component, Debug, Clone)]
pub struct OneToNRequest {
    /// Destination entities and their target geometric states.
    /// The order determines pairing with source slices: target 0 receives slice 0.
    pub targets: Vec<GroupTarget>,
    /// Default transition config. Applied to all targets unless `child_behavior`
    /// returns a different config for that index.
    pub default_config: TransitionConfig,
    /// Optional per-child config override. When `Some`, called once per target
    /// with `(index, total)`. Overrides `default_config` for that child.
    pub child_behavior: Option<ChildBehaviorFn>,
    /// Which strategy normalizes the 1→N to a set of 1→1 lerps.
    pub strategy: SplitStrategy,
}

/// One target entry in a 1→N group transition.
#[derive(Debug, Clone)]
pub struct GroupTarget {
    /// The real destination entity. Hidden during a Slice transition;
    /// transitions directly for a Bake transition.
    pub entity: Entity,
    /// The geometric state this entity should settle at after the transition.
    pub state: QuadState,
}

// ---------------------------------------------------------------------------
// NToOneRequest — N→1 group transition
// ---------------------------------------------------------------------------

/// Added to the **destination** entity to trigger an N→1 merge transition.
///
/// The destination entity is hidden during the transition and revealed on
/// group completion. N source entities go invisible immediately.
/// N virtual entities are created (one per source), each animating from the
/// corresponding source's geometry to a horizontal slice of the destination.
///
/// Only the [`SplitStrategy::Slice`] strategy is implemented for N→1 in M3.
#[derive(Component, Debug, Clone)]
pub struct NToOneRequest {
    /// Source entities and their current geometric states.
    ///
    /// The caller must snapshot each source entity's `QuadState` and provide
    /// it here — the setup system cannot query arbitrary entity components
    /// without borrowing the world a second time.
    pub sources: Vec<GroupSource>,
    /// Default transition config.
    pub default_config: TransitionConfig,
    /// Optional per-source config override.
    pub child_behavior: Option<ChildBehaviorFn>,
    // Bake N→1 is deferred to M4 (requires tracking multiple non-virtual
    // completions). Strategy field reserved here for API symmetry.
}

/// One source entry in an N→1 group transition.
#[derive(Debug, Clone)]
pub struct GroupSource {
    /// The source entity (becomes invisible immediately on setup).
    pub entity: Entity,
    /// The source entity's current geometric state.
    /// Must be supplied by the caller (snapshot it before inserting `NToOneRequest`).
    pub state: QuadState,
}

// ---------------------------------------------------------------------------
// ActiveGroupTransition — coordinator state during a Slice group transition
// ---------------------------------------------------------------------------

/// Attached to the group coordinator entity while a Slice group transition runs.
///
/// For 1→N: the coordinator is the source entity.
/// For N→1: the coordinator is the destination entity.
///
/// `group_transition_complete_system` checks `remaining` each frame and
/// finalizes when all virtual entities have completed.
#[derive(Component, Debug)]
pub struct ActiveGroupTransition {
    /// Entities to make visible (`Visibility::VISIBLE`) when all virtuals complete.
    pub reveal_on_complete: Vec<Entity>,
    /// Total virtual entities created for this group transition.
    ///
    /// Used to detect completion: when the count of complete virtuals that
    /// carry `PartOfGroup(coordinator)` equals `total`, the group is done.
    pub total: usize,
    /// The one `transition_atlas` bake shared by every virtual in this group
    /// (the source's bake for 1→N, the destination's bake for N→1) — as
    /// opposed to each virtual's own individual bake, tracked per-virtual on
    /// its own `BakedTexture::own_alloc`. `None` when baking was unavailable
    /// or failed and the group fell back to flat-color slices.
    /// `group_transition_complete_system` frees this once, on completion.
    pub shared_alloc: Option<TransitionAllocId>,
}

// ---------------------------------------------------------------------------
// PartOfGroup — links a virtual entity to its coordinator
// ---------------------------------------------------------------------------

/// Links a virtual entity to its group coordinator.
///
/// `group_transition_complete_system` groups all virtual entities by their
/// coordinator to detect when a group's transition is fully complete.
#[derive(Component, Debug, Clone)]
pub struct PartOfGroup(pub Entity);

// ---------------------------------------------------------------------------
// Slice geometry helpers
// ---------------------------------------------------------------------------

/// Divide `source` into `n` equal horizontal strips (left-to-right columns).
///
/// Each strip has the full height of the source and `source.width / n` width.
/// Strips are centered along the same Y axis as the source. The anchor,
/// rotation, scale, and color are inherited unchanged.
///
/// # Panics
/// Panics if `n == 0`.
///
/// # Example
/// A 300×100 source at (0, 0) divided into 3 strips:
/// - Strip 0: 100×100 at (-100, 0)
/// - Strip 1: 100×100 at (  0, 0)
/// - Strip 2: 100×100 at ( 100, 0)
pub fn horizontal_slices(source: &QuadState, n: usize) -> Vec<QuadState> {
    assert!(n > 0, "horizontal_slices: n must be > 0");
    let slice_w = source.size.x / n as f32;
    let leftmost_center = source.position.x - source.size.x * 0.5 + slice_w * 0.5;
    // Clamp corner_radius to the slice's own half-extents. Uncapped, a large
    // radius inherited from the source (e.g. a circular button's radius =
    // half its full width) exceeds a narrow slice's half-width, and the
    // rounded-rect SDF collapses to little more than a sliver around the
    // slice's center — the same degenerate case fixed for the text overlay
    // quad in collect.rs, here applied to slice geometry instead.
    let corner_radius = source
        .corner_radius
        .min(slice_w * 0.5)
        .min(source.size.y * 0.5);
    (0..n)
        .map(|i| {
            let x = leftmost_center + slice_w * i as f32;
            QuadState {
                position: glam::Vec3::new(x, source.position.y, source.position.z),
                size: glam::Vec2::new(slice_w, source.size.y),
                corner_radius,
                ..source.clone()
            }
        })
        .collect()
}

/// Divide `source` into `n` equal vertical strips (top-to-bottom rows, Y-down).
///
/// Each strip has the full width of the source and `source.height / n` height.
pub fn vertical_slices(source: &QuadState, n: usize) -> Vec<QuadState> {
    assert!(n > 0, "vertical_slices: n must be > 0");
    let slice_h = source.size.y / n as f32;
    // Y-down: the topmost strip has the highest Y (most negative in world space).
    // Position top strip at the top of the source's bounding box.
    let topmost_center = source.position.y + source.size.y * 0.5 - slice_h * 0.5;
    (0..n)
        .map(|i| {
            let y = topmost_center - slice_h * i as f32;
            QuadState {
                position: glam::Vec3::new(source.position.x, y, source.position.z),
                size: glam::Vec2::new(source.size.x, slice_h),
                ..source.clone()
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Transition-bake helpers — shared by one_to_n_setup_system and n_to_one_setup_system
// ---------------------------------------------------------------------------

/// Visual components (beyond `QuadState`, which the caller already has —
/// either from the coordinator's own query or a `GroupTarget`/`GroupSource`'s
/// stored `state`) needed to bake an entity's rendered appearance.
type BakeVisualsQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Border>,
        Option<&'static Glow>,
        Option<&'static DropShadow>,
        Option<&'static BakedText>,
        Option<&'static Text>,
        Option<&'static BakedImage>,
    ),
>;

/// Build the `QuadInstance`s that represent `entity`'s own rendered
/// appearance — background (+ text overlay, if any) — the same two-instance
/// shape `collect_instances` produces per frame, just for one entity, once,
/// on demand.
///
/// This is deliberately its own named step (see `bake_one` below) rather than
/// inlined: a future composite-component feature (parent + children baked as
/// one unit) swaps this for a version that also walks a `Hierarchy`/children
/// component and gathers every descendant's instances too — nothing else in
/// `bake_one` or the setup systems would need to change.
fn gather_bake_instances(
    visuals: &BakeVisualsQuery,
    entity: Entity,
    qs: &QuadState,
) -> Vec<QuadInstance> {
    let Ok((border, glow, shadow, baked_text, text, baked_image)) = visuals.get(entity) else {
        return vec![quad_state_to_instance(qs, None, None, None, None)];
    };

    let mut bg_inst = quad_state_to_instance(qs, None, shadow, glow, border);
    // A static image (M9.7 box-cover art) is a one-time UV mapping into
    // main_atlas (atlas_page 0, already the default) — same handling as the
    // per-frame path in collect.rs's push_entity_instances. Without this, a
    // Slice-transition target with BakedImage would bake as its flat
    // placeholder color instead of the actual box art it shows once revealed.
    if let Some(image) = baked_image {
        bg_inst.uv_offset = image.uv_offset;
        bg_inst.uv_scale = image.uv_scale;
    }
    let mut out = vec![bg_inst];

    if let Some(b) = baked_text {
        let mut text_qs = qs.clone();
        text_qs.color = text.map(|t| t.color).unwrap_or(Vec4::ONE);
        // Same footprint-sizing / corner_radius-zeroing fix as the per-frame
        // text overlay in collect.rs — see that file for the full rationale.
        text_qs.size = b.pixel_size.into();
        text_qs.corner_radius = 0.0;
        out.push(quad_state_to_instance(&text_qs, Some(b), None, None, None));
    }

    out
}

/// Normalise a `TransitionRegion` into `(uv_offset, uv_scale)` within
/// `transition_atlas`.
fn region_uv(region: &TransitionRegion) -> ([f32; 2], [f32; 2]) {
    let atlas = TRANSITION_ATLAS_SIZE as f32;
    (
        [region.x as f32 / atlas, region.y as f32 / atlas],
        [region.width as f32 / atlas, region.height as f32 / atlas],
    )
}

/// Divide a baked region into `n` equal left-to-right UV thirds — the UV-space
/// counterpart of `horizontal_slices`, used to pair each geometry slice with
/// its matching crop of the shared bake.
fn region_uv_slices(region: &TransitionRegion, n: usize) -> Vec<([f32; 2], [f32; 2])> {
    let atlas = TRANSITION_ATLAS_SIZE as f32;
    let slice_w = region.width as f32 / n as f32;
    (0..n)
        .map(|i| {
            let x = region.x as f32 + slice_w * i as f32;
            (
                [x / atlas, region.y as f32 / atlas],
                [slice_w / atlas, region.height as f32 / atlas],
            )
        })
        .collect()
}

/// Bake `entity`'s rendered appearance (per `qs`) into a freshly allocated
/// `transition_atlas` region. Returns `None` if there's nothing to bake or
/// the atlas is full — callers treat that as "fall back to flat-color
/// geometry for this entity," not a hard error.
fn bake_one(
    pipeline: &mut QuadPipeline,
    gpu: &GpuContext,
    visuals: &BakeVisualsQuery,
    entity: Entity,
    qs: &QuadState,
) -> Option<(TransitionAllocId, TransitionRegion)> {
    let instances = gather_bake_instances(visuals, entity, qs);
    if instances.is_empty() {
        return None;
    }

    let width = qs.size.x.max(1.0).ceil() as u32;
    let height = qs.size.y.max(1.0).ceil() as u32;
    let (alloc_id, granted) = pipeline.allocate_transition_region(width, height)?;

    // The allocator may grant a padded region (shelf packers commonly round
    // up) — bake and UV-address only the requested width×height within it,
    // not the full grant, so slice UV math stays exact.
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

/// Processes [`OneToNRequest`] components and sets up the 1→N transition.
///
/// **Bake strategy:**
/// - Hides the source entity.
/// - Inserts [`TransitionRequest`] with `from_state = Some(source_state)` on
///   each target entity so they animate from the source's geometry to their own.
/// - No virtual entities; each target's completion is handled by
///   `transition_complete_system` independently.
///
/// **Slice strategy:**
/// - Hides the source and all target entities.
/// - Spawns N [`Virtual`] entities, each starting at a horizontal slice of
///   the source and lerping to the corresponding target's state.
/// - Attaches [`ActiveGroupTransition`] to the source (coordinator).
/// - When [`GpuContext`]/[`QuadPipeline`] are available (`Option<Res<...>>` —
///   absent in, e.g., a bare test `World`): bakes the source once and each
///   target once, and each virtual crossfades texel-for-texel between its
///   slice of the source bake and its target's own bake — real shape,
///   border, and text on both ends, not a flat-color approximation. If GPU
///   resources are unavailable, or a bake fails (atlas full), falls back to
///   today's flat-color slice geometry for that virtual.
pub fn one_to_n_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &OneToNRequest, &QuadState)>,
    visuals: BakeVisualsQuery,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
) {
    for (source_entity, request, source_state) in query.iter() {
        let n = request.targets.len();

        // Always remove the request so it isn't re-processed.
        commands.entity(source_entity).remove::<OneToNRequest>();

        if n == 0 {
            continue;
        }

        // Hide the source — it is being replaced by (or transitioning into) N targets.
        commands.entity(source_entity).insert(Visibility::HIDDEN);

        match request.strategy {
            SplitStrategy::Bake => {
                // Normalize to N independent 1→1 transitions.
                // Each target animates from the source geometry to its own position.
                for (i, target) in request.targets.iter().enumerate() {
                    let cfg = request
                        .child_behavior
                        .map(|f| f(i, n))
                        .unwrap_or(request.default_config);

                    commands.entity(target.entity).insert(TransitionRequest {
                        to: target.state.clone(),
                        from_state: Some(source_state.clone()),
                        config: cfg,
                    });
                }

                // Source has no active transition of its own — goes Idle immediately.
                commands.entity(source_entity).insert(Lifecycle::Idle);
            }

            SplitStrategy::Slice => {
                // Hide all target entities until the transition completes.
                for target in &request.targets {
                    commands.entity(target.entity).insert(Visibility::HIDDEN);
                }
                let reveal: Vec<Entity> = request.targets.iter().map(|t| t.entity).collect();

                // Try the baked, two-sided crossfade path: bake the source
                // once (shared across every slice), then each target once
                // (one bake per slice). Only proceeds if GPU resources are
                // present *and* the shared source bake succeeds — a partial
                // source bake can't produce meaningful slice crops.
                let mut shared_alloc: Option<TransitionAllocId> = None;
                let mut from_uv_slices: Vec<([f32; 2], [f32; 2])> = Vec::new();
                let mut target_bakes: Vec<Option<(TransitionAllocId, TransitionRegion)>> =
                    Vec::new();

                if let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) {
                    if let Some((src_id, src_region)) =
                        bake_one(pipeline, gpu, &visuals, source_entity, source_state)
                    {
                        shared_alloc = Some(src_id);
                        from_uv_slices = region_uv_slices(&src_region, n);
                        target_bakes = request
                            .targets
                            .iter()
                            .map(|t| bake_one(pipeline, gpu, &visuals, t.entity, &t.state))
                            .collect();
                    }
                }

                // Snapshot the source entity's visual components so a
                // fallen-back (non-baked) virtual still inherits them —
                // today's behavior, unchanged.
                let src_glow = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, g, _, _, _, _)| g.cloned());
                let src_shadow = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, _, s, _, _, _)| s.cloned());
                let src_baked = visuals
                    .get(source_entity)
                    .ok()
                    .and_then(|(_, _, _, b, _, _)| b.cloned());

                let slices = horizontal_slices(source_state, n);

                // Spawn one virtual entity per slice.
                for (i, (slice_state, target)) in
                    slices.iter().zip(request.targets.iter()).enumerate()
                {
                    let cfg = request
                        .child_behavior
                        .map(|f| f(i, n))
                        .unwrap_or(request.default_config);

                    let own_bake = target_bakes.get(i).copied().flatten();
                    let (from_state, to_state, baked_texture) = match (shared_alloc, own_bake) {
                        (Some(_), Some((own_id, own_region))) => {
                            let (from_off, from_scale) = from_uv_slices[i];
                            let (to_off, to_scale) = region_uv(&own_region);
                            // Both ends flattened to a plain white pass-through
                            // quad — the baked pixels carry the real shape,
                            // color, and text, so the QuadState wrapping them
                            // shouldn't also tint or re-round on top.
                            let from_state = QuadState {
                                color: Vec4::ONE,
                                corner_radius: 0.0,
                                ..slice_state.clone()
                            };
                            let to_state = QuadState {
                                color: Vec4::ONE,
                                corner_radius: 0.0,
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
                        // Fallback path (no bake): propagate the source's own
                        // visuals, exactly as before this feature existed.
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

                // Coordinator: source entity tracks group completion.
                commands.entity(source_entity).insert((
                    Lifecycle::Transitioning,
                    ActiveGroupTransition {
                        reveal_on_complete: reveal,
                        total: n,
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

/// Processes [`NToOneRequest`] components and sets up the N→1 transition.
///
/// Uses the Slice strategy (only strategy implemented for N→1 in M3):
/// - Hides all source entities and the destination entity.
/// - Spawns N [`Virtual`] entities, each animating from a source's geometry
///   to the corresponding horizontal slice of the destination.
/// - Attaches [`ActiveGroupTransition`] to the destination (coordinator).
/// - On group completion: destination entity becomes visible.
///
/// The mirror image of [`one_to_n_setup_system`]'s baked crossfade, with
/// source/target roles swapped: each source is baked individually (its own
/// bake, freed with its virtual), and the destination is baked once and
/// sliced (the shared bake, freed once on group completion). Same
/// `Option<Res<GpuContext>>`/`Option<ResMut<QuadPipeline>>` graceful
/// degradation to flat-color slices when GPU resources are unavailable or a
/// bake fails.
pub fn n_to_one_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &NToOneRequest, &QuadState)>,
    visuals: BakeVisualsQuery,
    gpu: Option<Res<GpuContext>>,
    mut pipeline: Option<ResMut<QuadPipeline>>,
) {
    for (dest_entity, request, dest_state) in query.iter() {
        let n = request.sources.len();

        commands.entity(dest_entity).remove::<NToOneRequest>();

        if n == 0 {
            continue;
        }

        // Hide the destination entity during the transition.
        commands
            .entity(dest_entity)
            .insert(Visibility::HIDDEN)
            .insert(Lifecycle::Transitioning);

        // Hide all source entities.
        for source in &request.sources {
            commands.entity(source.entity).insert(Visibility::HIDDEN);
        }

        // Compute target slices — one per source.
        let target_slices = horizontal_slices(dest_state, n);

        // Try the baked, two-sided crossfade path: bake the destination once
        // (shared across every virtual), then each source once (one bake per
        // virtual). Mirrors one_to_n_setup_system with roles swapped.
        let mut shared_alloc: Option<TransitionAllocId> = None;
        let mut to_uv_slices: Vec<([f32; 2], [f32; 2])> = Vec::new();
        let mut source_bakes: Vec<Option<(TransitionAllocId, TransitionRegion)>> = Vec::new();

        if let (Some(gpu), Some(pipeline)) = (gpu.as_deref(), pipeline.as_deref_mut()) {
            if let Some((dest_id, dest_region)) =
                bake_one(pipeline, gpu, &visuals, dest_entity, dest_state)
            {
                shared_alloc = Some(dest_id);
                to_uv_slices = region_uv_slices(&dest_region, n);
                source_bakes = request
                    .sources
                    .iter()
                    .map(|s| bake_one(pipeline, gpu, &visuals, s.entity, &s.state))
                    .collect();
            }
        }

        // Spawn one virtual entity per source, transitioning to the matching slice.
        for (i, (source, target_slice)) in
            request.sources.iter().zip(target_slices.iter()).enumerate()
        {
            let cfg = request
                .child_behavior
                .map(|f| f(i, n))
                .unwrap_or(request.default_config);

            let own_bake = source_bakes.get(i).copied().flatten();
            let (from_state, to_state, baked_texture) = match (shared_alloc, own_bake) {
                (Some(_), Some((own_id, own_region))) => {
                    let (to_off, to_scale) = to_uv_slices[i];
                    let (from_off, from_scale) = region_uv(&own_region);
                    let from_state = QuadState {
                        color: Vec4::ONE,
                        corner_radius: 0.0,
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
            } else if let Ok((_, glow, shadow, baked, _, _)) = visuals.get(source.entity) {
                // Fallback path (no bake): propagate the source entity's own
                // visuals, exactly as before this feature existed.
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

        // Coordinator: destination entity tracks group completion.
        commands.entity(dest_entity).insert(ActiveGroupTransition {
            reveal_on_complete: vec![dest_entity],
            total: n,
            shared_alloc,
        });
    }
}

// ---------------------------------------------------------------------------
// group_transition_complete_system
// ---------------------------------------------------------------------------

/// Detects when all virtual entities in a group are done and finalizes.
///
/// Runs after `transition_complete_system` in the schedule. Groups all
/// virtual entities by their [`PartOfGroup`] coordinator. When every virtual
/// in a group has `is_complete = true`, the system:
///
/// 1. Inserts `Visibility::VISIBLE` on all `reveal_on_complete` entities.
/// 2. Sets the coordinator's `Lifecycle` to `Idle`.
/// 3. Removes `ActiveGroupTransition` from the coordinator.
/// 4. If a [`QuadPipeline`] resource is available: frees the coordinator's
///    `shared_alloc` (once) and each virtual's own `BakedTexture::own_alloc`
///    (if baking was used for that group at all).
/// 5. Queues despawn of all virtual entities in the group.
///
/// Completion is idempotent: once all deferred commands from step 3 are
/// applied (next `FlushCommands`), the virtual entities are gone and the
/// coordinator has no `ActiveGroupTransition`, so the system does no work.
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
    mut pipeline: Option<ResMut<QuadPipeline>>,
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
        if complete_count < total {
            continue; // still waiting on some virtuals
        }

        // All virtuals for this group are done — finalize.
        let Ok((group, mut lifecycle)) = coordinators.get_mut(coord_entity) else {
            continue;
        };

        // Reveal target entities.
        for &entity in &group.reveal_on_complete {
            commands.entity(entity).insert(Visibility::VISIBLE);
        }

        // Free the transition_atlas allocations this group used, if any.
        if let Some(pipeline) = pipeline.as_deref_mut() {
            if let Some(shared) = group.shared_alloc {
                pipeline.free_transition_region(shared);
            }
            for (_, own_alloc) in &v_entities {
                if let Some(id) = own_alloc {
                    pipeline.free_transition_region(*id);
                }
            }
        }

        // Restore coordinator lifecycle and remove group state.
        *lifecycle = Lifecycle::Idle;
        commands
            .entity(coord_entity)
            .remove::<ActiveGroupTransition>();

        // Despawn all virtual entities.
        for (v_entity, _) in v_entities {
            commands.entity(v_entity).despawn();
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests — slice geometry math
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

    #[test]
    fn horizontal_slices_count() {
        let slices = horizontal_slices(&source(), 5);
        assert_eq!(slices.len(), 5);
    }

    #[test]
    fn horizontal_slices_width() {
        let slices = horizontal_slices(&source(), 5);
        for s in &slices {
            assert!(
                (s.size.x - 60.0).abs() < 1e-4,
                "each slice should be 60px wide"
            );
        }
    }

    #[test]
    fn horizontal_slices_height_preserved() {
        let slices = horizontal_slices(&source(), 5);
        for s in &slices {
            assert!((s.size.y - 100.0).abs() < 1e-4);
        }
    }

    #[test]
    fn horizontal_slices_positions_span_source() {
        // The leftmost slice center should be at x = -120 and rightmost at x = +120
        // for a 300px-wide source centered at 0.
        let slices = horizontal_slices(&source(), 5);
        let xs: Vec<f32> = slices.iter().map(|s| s.position.x).collect();
        assert!((xs[0] - (-120.0)).abs() < 1e-3, "leftmost x={}", xs[0]);
        assert!((xs[4] - 120.0).abs() < 1e-3, "rightmost x={}", xs[4]);
    }

    #[test]
    fn horizontal_slices_no_gap_no_overlap() {
        // Adjacent slice centers should be exactly slice_width apart.
        let slices = horizontal_slices(&source(), 5);
        let slice_w = 300.0 / 5.0; // 60.0
        for i in 1..slices.len() {
            let gap = slices[i].position.x - slices[i - 1].position.x;
            assert!((gap - slice_w).abs() < 1e-3, "gap={}", gap);
        }
    }

    /// Regression test: `gather_bake_instances` must include a target's
    /// `BakedImage` UV in the baked background instance. Before this fix,
    /// `BakeVisualsQuery` didn't query `BakedImage` at all, so a Slice
    /// transition's target (e.g. a video tile with box-cover art) baked as
    /// its flat placeholder color instead of the actual box art it shows
    /// once revealed — no crossfade ever showed the real image.
    #[test]
    fn gather_bake_instances_includes_baked_image_uv() {
        use crate::image::BakedImage;
        use bevy_ecs::system::SystemState;

        let mut world = World::new();
        let entity = world
            .spawn((
                source(),
                BakedImage {
                    uv_offset: [0.4, 0.5],
                    uv_scale: [0.2, 0.3],
                    pixel_size: [400.0, 600.0],
                },
            ))
            .id();

        let mut state: SystemState<BakeVisualsQuery> = SystemState::new(&mut world);
        let visuals = state.get(&world);

        let instances = gather_bake_instances(&visuals, entity, &source());
        assert_eq!(instances.len(), 1, "no BakedText — just the background");
        assert_eq!(instances[0].uv_offset, [0.4, 0.5]);
        assert_eq!(instances[0].uv_scale, [0.2, 0.3]);
    }

    #[test]
    fn horizontal_slices_preserves_color_and_radius() {
        let slices = horizontal_slices(&source(), 3);
        for s in &slices {
            assert_eq!(s.color, Vec4::new(1.0, 0.0, 0.0, 1.0));
            assert_eq!(s.corner_radius, 0.0);
        }
    }

    /// A circular source (corner_radius == half its full width, e.g. a round
    /// button) sliced into narrow strips must not carry that radius through
    /// unclamped — it would exceed each slice's own half-width and collapse
    /// the rounded-rect SDF to a sliver. See the comment in
    /// `horizontal_slices` for the full explanation.
    #[test]
    fn horizontal_slices_clamps_corner_radius_to_slice_half_extents() {
        let circle = QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(200.0, 200.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: 100.0, // full circle: radius == half the width
        };
        let slices = horizontal_slices(&circle, 3);
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
    fn vertical_slices_count() {
        let slices = vertical_slices(&source(), 4);
        assert_eq!(slices.len(), 4);
    }

    #[test]
    fn vertical_slices_height() {
        let slices = vertical_slices(&source(), 4);
        for s in &slices {
            assert!(
                (s.size.y - 25.0).abs() < 1e-4,
                "each slice should be 25px tall"
            );
        }
    }
}
