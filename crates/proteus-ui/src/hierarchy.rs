//! Component composition & hierarchy — M10.
//!
//! Parent/child entity relationships built directly on `bevy_ecs`'s first-class
//! [`ChildOf`]/[`Children`] relationship components (bevy_ecs 0.18 ships these
//! with automatic cascading despawn — no hand-rolled hierarchy component
//! needed).
//!
//! ## Local vs. world `QuadState`
//!
//! A root entity's [`QuadState`] is world-space, exactly as before this
//! milestone — nothing currently has a parent, so nothing's behavior changes.
//! A *child* entity's `QuadState` fields are declared in its parent's local
//! frame. [`resolve_world_position`] (and its `Query`-based twin
//! [`resolve_world_position_query`], for systems that only have typed query
//! access rather than a raw `&World`) compose position, rotation, *and* scale
//! down the parent chain — see the function docs for the exact formula.
//!
//! This composition is a pure, on-demand function, not a cached component
//! written by a schedule system. `collect_instances`/`collect_entity_instances`
//! (`collect.rs`) and the transition-bake capture (`topology.rs`) call it fresh
//! every time they need an entity's world state. This sidesteps a same-frame
//! staleness problem entirely (no system writes a `WorldQuadState` that another
//! system must then read later in the same frame) and — as a bonus — means a
//! future declarative/percentage-relative positioning system only needs to set
//! a child's *local* `QuadState` before this resolution runs; nothing here
//! needs to change to support that later.
//!
//! ## Cascading visibility & opacity
//!
//! Unlike position resolution, [`EffectiveVisibility`] and [`EffectiveOpacity`]
//! *are* real components written by schedule systems ([`visibility_system`],
//! [`opacity_system`]), because the DoD for this milestone explicitly calls for
//! replacing `stub_visibility_system`/`stub_opacity_system` with cascade
//! implementations in their existing schedule slots. See
//! `schedule::ProteusSet::CascadeFlush` for why an extra `ApplyDeferred` had to
//! be added right after them.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::component::QuadState;

// ---------------------------------------------------------------------------
// Opacity — declared, local
// ---------------------------------------------------------------------------

/// Declared per-entity opacity multiplier, local to this entity (not yet
/// cascaded with its ancestors — see [`EffectiveOpacity`] for that).
///
/// Absent is equivalent to `Opacity(1.0)` — matches the codebase's existing
/// "no component = default" convention (e.g. [`crate::Visibility`]).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Opacity(pub f32);

impl Default for Opacity {
    fn default() -> Self {
        Self(1.0)
    }
}

// ---------------------------------------------------------------------------
// EffectiveVisibility / EffectiveOpacity — computed, cascaded
// ---------------------------------------------------------------------------

/// Computed by [`visibility_system`]: `own.visible && parent.effective`.
///
/// Roots have no parent term (`effective = own.visible`). Written every frame
/// for every entity reachable from a root — including entities with no
/// `ChildOf`, which are treated as roots of their own single-entity subtree.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EffectiveVisibility(pub bool);

/// Computed by [`opacity_system`]: `own.opacity * parent.effective`.
///
/// Roots have no parent term (`effective = own.opacity`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct EffectiveOpacity(pub f32);

// ---------------------------------------------------------------------------
// World-position resolution
// ---------------------------------------------------------------------------

/// Compose a child's local [`QuadState`] with its parent's resolved world
/// state, one level: position, rotation, and scale all inherit; size, anchor,
/// color, and corner_radius stay the child's own.
///
/// ```text
/// world.rotation = parent_world.rotation + local.rotation
/// world.scale    = parent_world.scale * local.scale
/// world.position.xy = parent_world.position.xy
///                    + rotate(local.position.xy * parent_world.scale, parent_world.rotation)
/// world.position.z  = parent_world.position.z + local.position.z
/// ```
///
/// `size`/`anchor`/`color`/`corner_radius` are *not* touched here — they stay
/// `local`'s own values. `size` deliberately isn't multiplied by the parent's
/// scale directly: the vertex shader already applies `size * scale` at render
/// time, and `world.scale` above already carries the compounded multiplier, so
/// double-scaling is avoided.
pub(crate) fn compose_with_parent(parent_world: &QuadState, local: &QuadState) -> QuadState {
    let rotated_offset = Vec2::from_angle(parent_world.rotation)
        .rotate(local.position.truncate() * parent_world.scale);
    QuadState {
        position: glam::Vec3::new(
            parent_world.position.x + rotated_offset.x,
            parent_world.position.y + rotated_offset.y,
            parent_world.position.z + local.position.z,
        ),
        rotation: parent_world.rotation + local.rotation,
        scale: parent_world.scale * local.scale,
        ..local.clone()
    }
}

/// Resolve `entity`'s world-space [`QuadState`] given its already-known local
/// state `local`, recursively composing through any `ChildOf` ancestor chain.
///
/// Returns `local` unchanged for a root entity (no `ChildOf`) — zero-cost,
/// zero-behavior-change for every entity that predates this milestone.
///
/// Call this fresh every time you need an entity's world state (per-frame
/// rendering, on-demand bake capture) rather than caching the result — see the
/// module docs for why.
pub fn resolve_world_position(world: &World, entity: Entity, local: &QuadState) -> QuadState {
    match world.get::<ChildOf>(entity) {
        Some(child_of) => {
            let parent = child_of.parent();
            let parent_local = world.get::<QuadState>(parent).cloned().unwrap_or_default();
            let parent_world = resolve_world_position(world, parent, &parent_local);
            compose_with_parent(&parent_world, local)
        }
        None => local.clone(),
    }
}

/// `Query`-based twin of [`resolve_world_position`], for systems (like
/// `hit_test_system`) that only have typed query access, not a raw `&World`.
///
/// Same composition formula; same "returns `local` unchanged for a root
/// entity" behavior.
pub fn resolve_world_position_query(
    entity: Entity,
    local: &QuadState,
    quad_states: &Query<&QuadState>,
    parents: &Query<&ChildOf>,
) -> QuadState {
    match parents.get(entity) {
        Ok(child_of) => {
            let parent = child_of.parent();
            let parent_local = quad_states.get(parent).cloned().unwrap_or_default();
            let parent_world =
                resolve_world_position_query(parent, &parent_local, quad_states, parents);
            compose_with_parent(&parent_world, local)
        }
        Err(_) => local.clone(),
    }
}

// ---------------------------------------------------------------------------
// Cascade systems
// ---------------------------------------------------------------------------

/// Replaces `stub_visibility_system`. Cascades [`crate::Visibility`] down the
/// hierarchy into [`EffectiveVisibility`]: a hidden parent makes its entire
/// subtree effectively invisible regardless of what each descendant declares
/// for itself.
///
/// Walks from roots (`Without<ChildOf>`) down through `Children`, so entities
/// with no hierarchy involvement at all (every entity that predates this
/// milestone) are treated as single-node roots and still get an
/// `EffectiveVisibility` written (`own.visible`, unchanged from today).
pub fn visibility_system(
    mut commands: Commands,
    roots: Query<(Entity, Option<&crate::component::Visibility>), Without<ChildOf>>,
    children_q: Query<&Children>,
    vis_q: Query<Option<&crate::component::Visibility>>,
) {
    fn cascade(
        entity: Entity,
        parent_effective: bool,
        commands: &mut Commands,
        children_q: &Query<&Children>,
        vis_q: &Query<Option<&crate::component::Visibility>>,
    ) {
        let own_visible = vis_q
            .get(entity)
            .ok()
            .flatten()
            .map(|v| v.visible)
            .unwrap_or(true);
        let effective = parent_effective && own_visible;
        commands
            .entity(entity)
            .insert(EffectiveVisibility(effective));
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                cascade(child, effective, commands, children_q, vis_q);
            }
        }
    }

    for (root, vis) in roots.iter() {
        let own_visible = vis.map(|v| v.visible).unwrap_or(true);
        commands
            .entity(root)
            .insert(EffectiveVisibility(own_visible));
        if let Ok(children) = children_q.get(root) {
            for child in children.iter() {
                cascade(child, own_visible, &mut commands, &children_q, &vis_q);
            }
        }
    }
}

/// Replaces `stub_opacity_system`. Cascades [`Opacity`] down the hierarchy
/// into [`EffectiveOpacity`], multiplying down the parent chain — a parent at
/// `0.5` with a child at `0.8` yields an effective child opacity of `0.4`.
///
/// Same root/children traversal shape as [`visibility_system`]; see its docs.
pub fn opacity_system(
    mut commands: Commands,
    roots: Query<(Entity, Option<&Opacity>), Without<ChildOf>>,
    children_q: Query<&Children>,
    op_q: Query<Option<&Opacity>>,
) {
    fn cascade(
        entity: Entity,
        parent_effective: f32,
        commands: &mut Commands,
        children_q: &Query<&Children>,
        op_q: &Query<Option<&Opacity>>,
    ) {
        let own = op_q.get(entity).ok().flatten().copied().unwrap_or_default();
        let effective = parent_effective * own.0;
        commands.entity(entity).insert(EffectiveOpacity(effective));
        if let Ok(children) = children_q.get(entity) {
            for child in children.iter() {
                cascade(child, effective, commands, children_q, op_q);
            }
        }
    }

    for (root, op) in roots.iter() {
        let own = op.copied().unwrap_or_default();
        commands.entity(root).insert(EffectiveOpacity(own.0));
        if let Ok(children) = children_q.get(root) {
            for child in children.iter() {
                cascade(child, own.0, &mut commands, &children_q, &op_q);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Visibility;
    use bevy_ecs::hierarchy::ChildOf;
    use glam::{Vec3, Vec4};

    fn qs(x: f32, y: f32) -> QuadState {
        QuadState {
            position: Vec3::new(x, y, 0.0),
            ..Default::default()
        }
    }

    #[test]
    fn opacity_defaults_to_one() {
        assert_eq!(Opacity::default(), Opacity(1.0));
    }

    #[test]
    fn resolve_world_position_root_is_unchanged() {
        let mut world = World::new();
        let root = world.spawn(qs(10.0, 20.0)).id();
        let local = world.get::<QuadState>(root).unwrap().clone();
        let resolved = resolve_world_position(&world, root, &local);
        assert_eq!(resolved.position, Vec3::new(10.0, 20.0, 0.0));
    }

    #[test]
    fn resolve_world_position_composes_translation() {
        let mut world = World::new();
        let parent = world.spawn(qs(100.0, 50.0)).id();
        let child = world.spawn((qs(10.0, -5.0), ChildOf(parent))).id();
        let local = world.get::<QuadState>(child).unwrap().clone();
        let resolved = resolve_world_position(&world, child, &local);
        assert!((resolved.position.x - 110.0).abs() < 1e-4);
        assert!((resolved.position.y - 45.0).abs() < 1e-4);
    }

    #[test]
    fn resolve_world_position_two_level_chain() {
        let mut world = World::new();
        let grandparent = world.spawn(qs(0.0, 0.0)).id();
        let parent = world.spawn((qs(100.0, 0.0), ChildOf(grandparent))).id();
        let child = world.spawn((qs(10.0, 0.0), ChildOf(parent))).id();
        let local = world.get::<QuadState>(child).unwrap().clone();
        let resolved = resolve_world_position(&world, child, &local);
        assert!((resolved.position.x - 110.0).abs() < 1e-4);
    }

    #[test]
    fn resolve_world_position_rotated_parent_swaps_axes() {
        let mut world = World::new();
        let parent = QuadState {
            position: Vec3::ZERO,
            rotation: std::f32::consts::FRAC_PI_2, // 90 degrees
            ..Default::default()
        };
        let parent_id = world.spawn(parent).id();
        let child = world.spawn((qs(10.0, 0.0), ChildOf(parent_id))).id();
        let local = world.get::<QuadState>(child).unwrap().clone();
        let resolved = resolve_world_position(&world, child, &local);
        // A local offset of (10, 0) rotated 90 degrees should land at ~(0, 10).
        assert!(
            resolved.position.x.abs() < 1e-3,
            "x was {}",
            resolved.position.x
        );
        assert!(
            (resolved.position.y - 10.0).abs() < 1e-3,
            "y was {}",
            resolved.position.y
        );
    }

    #[test]
    fn resolve_world_position_composes_rotation_and_scale() {
        let mut world = World::new();
        let parent = QuadState {
            rotation: 0.3,
            scale: 2.0,
            ..Default::default()
        };
        let parent_id = world.spawn(parent).id();
        let child_local = QuadState {
            rotation: 0.1,
            scale: 1.5,
            ..Default::default()
        };
        let child = world.spawn((child_local.clone(), ChildOf(parent_id))).id();
        let resolved = resolve_world_position(&world, child, &child_local);
        assert!((resolved.rotation - 0.4).abs() < 1e-5);
        assert!((resolved.scale - 3.0).abs() < 1e-5);
    }

    #[test]
    fn resolve_world_position_leaves_size_color_local() {
        let mut world = World::new();
        let parent_id = world.spawn(qs(5.0, 5.0)).id();
        let child_local = QuadState {
            size: Vec2::new(42.0, 24.0),
            color: Vec4::new(0.1, 0.2, 0.3, 0.4),
            corner_radius: 7.0,
            ..qs(1.0, 1.0)
        };
        let child = world.spawn((child_local.clone(), ChildOf(parent_id))).id();
        let resolved = resolve_world_position(&world, child, &child_local);
        assert_eq!(resolved.size, child_local.size);
        assert_eq!(resolved.color, child_local.color);
        assert_eq!(resolved.corner_radius, child_local.corner_radius);
    }

    #[test]
    fn resolve_world_position_query_matches_world_based_version() {
        use bevy_ecs::system::SystemState;

        let mut world = World::new();
        let parent = world.spawn(qs(100.0, 50.0)).id();
        let child = world.spawn((qs(10.0, -5.0), ChildOf(parent))).id();
        let child_local = world.get::<QuadState>(child).unwrap().clone();

        let expected = resolve_world_position(&world, child, &child_local);

        let mut system_state: SystemState<(Query<&QuadState>, Query<&ChildOf>)> =
            SystemState::new(&mut world);
        let (quad_states, parents) = system_state.get(&world);
        let via_query = resolve_world_position_query(child, &child_local, &quad_states, &parents);

        assert!((via_query.position.x - expected.position.x).abs() < 1e-5);
        assert!((via_query.position.y - expected.position.y).abs() < 1e-5);
    }

    #[test]
    fn visibility_system_cascades_hidden_parent() {
        let mut world = World::new();
        let parent = world.spawn(Visibility::HIDDEN).id();
        let child = world.spawn((Visibility::VISIBLE, ChildOf(parent))).id();

        let mut schedule = Schedule::default();
        schedule.add_systems(visibility_system);
        schedule.run(&mut world);

        assert_eq!(
            world.get::<EffectiveVisibility>(parent),
            Some(&EffectiveVisibility(false))
        );
        assert_eq!(
            world.get::<EffectiveVisibility>(child),
            Some(&EffectiveVisibility(false)),
            "child declares itself visible, but a hidden parent must still make it effectively invisible"
        );
    }

    #[test]
    fn opacity_system_multiplies_down_chain() {
        let mut world = World::new();
        let parent = world.spawn(Opacity(0.5)).id();
        let child = world.spawn((Opacity(0.8), ChildOf(parent))).id();

        let mut schedule = Schedule::default();
        schedule.add_systems(opacity_system);
        schedule.run(&mut world);

        let effective = world.get::<EffectiveOpacity>(child).unwrap().0;
        assert!(
            (effective - 0.4).abs() < 1e-5,
            "expected ~0.4, got {effective}"
        );
    }
}
