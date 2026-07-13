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
//! `transition_tick_system`. When all virtuals in a group complete,
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

use crate::component::{Lifecycle, QuadState, TransitionRequest, Virtual, Visibility};
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
    (0..n)
        .map(|i| {
            let x = leftmost_center + slice_w * i as f32;
            QuadState {
                position: glam::Vec3::new(x, source.position.y, source.position.z),
                size: glam::Vec2::new(slice_w, source.size.y),
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
pub fn one_to_n_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &OneToNRequest, &QuadState)>,
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
                // Normalize to N→N via virtual entities.
                let slices = horizontal_slices(source_state, n);

                // Hide all target entities until the transition completes.
                for target in &request.targets {
                    commands.entity(target.entity).insert(Visibility::HIDDEN);
                }

                let reveal: Vec<Entity> = request.targets.iter().map(|t| t.entity).collect();
                let total = n;

                // Spawn one virtual entity per slice.
                for (i, (slice_state, target)) in
                    slices.iter().zip(request.targets.iter()).enumerate()
                {
                    let cfg = request
                        .child_behavior
                        .map(|f| f(i, n))
                        .unwrap_or(request.default_config);

                    let active =
                        ActiveTransition::new(slice_state.clone(), target.state.clone(), cfg);

                    commands.spawn((
                        slice_state.clone(),
                        Lifecycle::Transitioning,
                        active,
                        Virtual,
                        PartOfGroup(source_entity),
                    ));
                }

                // Coordinator: source entity tracks group completion.
                commands.entity(source_entity).insert((
                    Lifecycle::Transitioning,
                    ActiveGroupTransition {
                        reveal_on_complete: reveal,
                        total,
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
pub fn n_to_one_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &NToOneRequest, &QuadState)>,
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

        // Spawn one virtual entity per source, transitioning to the matching slice.
        for (i, (source, target_slice)) in
            request.sources.iter().zip(target_slices.iter()).enumerate()
        {
            let cfg = request
                .child_behavior
                .map(|f| f(i, n))
                .unwrap_or(request.default_config);

            let active = ActiveTransition::new(source.state.clone(), target_slice.clone(), cfg);

            commands.spawn((
                source.state.clone(),
                Lifecycle::Transitioning,
                active,
                Virtual,
                PartOfGroup(dest_entity),
            ));
        }

        // Coordinator: destination entity tracks group completion.
        commands.entity(dest_entity).insert(ActiveGroupTransition {
            reveal_on_complete: vec![dest_entity],
            total: n,
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
/// 4. Queues despawn of all virtual entities in the group.
///
/// Completion is idempotent: once all deferred commands from step 3 are
/// applied (next `FlushCommands`), the virtual entities are gone and the
/// coordinator has no `ActiveGroupTransition`, so the system does no work.
pub fn group_transition_complete_system(
    mut commands: Commands,
    virtuals: Query<(Entity, &PartOfGroup, &ActiveTransition), With<Virtual>>,
    mut coordinators: Query<(&ActiveGroupTransition, &mut Lifecycle)>,
) {
    // Build a map: coordinator_entity → (all_virtual_entities, complete_count).
    let mut by_coord: HashMap<Entity, (Vec<Entity>, usize)> = HashMap::new();

    for (v_entity, PartOfGroup(coord_entity), active) in virtuals.iter() {
        let entry = by_coord.entry(*coord_entity).or_insert((vec![], 0));
        entry.0.push(v_entity);
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

        // Restore coordinator lifecycle and remove group state.
        *lifecycle = Lifecycle::Idle;
        commands
            .entity(coord_entity)
            .remove::<ActiveGroupTransition>();

        // Despawn all virtual entities.
        for v_entity in v_entities {
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

    #[test]
    fn horizontal_slices_preserves_color_and_radius() {
        let slices = horizontal_slices(&source(), 3);
        for s in &slices {
            assert_eq!(s.color, Vec4::new(1.0, 0.0, 0.0, 1.0));
            assert_eq!(s.corner_radius, 0.0);
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
