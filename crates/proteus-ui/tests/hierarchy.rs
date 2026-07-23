//! M10 component composition & hierarchy regression tests.
//!
//! Pure `resolve_world_position` math (translation/rotation/scale composition,
//! multi-level chains) is covered by unit tests in `proteus-ui/src/hierarchy.rs`
//! itself. These integration tests exercise the same behavior through the full
//! `ProteusWorld` schedule — cascading visibility/opacity, hierarchy
//! construction/teardown, rendering, hit-testing, and independent child
//! transitions all running together, the way a real application would use them.
//!
//! ## Test matrix
//!
//! | Test | What it guards |
//! |---|---|
//! | `child_of_populates_children` | `ChildOf`/`Children` wiring sanity |
//! | `despawning_parent_despawns_child` | No entity leaks on parent destroy |
//! | `visibility_cascade_hidden_parent_hides_child` | Hidden parent ⇒ child `EffectiveVisibility` false regardless of its own declaration |
//! | `visibility_cascade_visible_parent_hidden_child_only_affects_that_child` | Sibling unaffected |
//! | `opacity_cascade_multiplies_down_chain` | Parent × child opacity composes |
//! | `opacity_cascade_defaults_to_one_when_undeclared` | Missing `Opacity` = 1.0 at every level |
//! | `collect_instances_positions_child_at_composed_world_offset` | End-to-end render position |
//! | `child_transitions_independently_of_parent` | Parent + child each mid-lerp, independently |
//! | `interactable_child_hit_tests_at_world_position` | Hit-testing uses resolved world position |

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use proteus_ui::{
    collect_instances, linear, transition::TransitionConfig, EffectiveOpacity, EffectiveVisibility,
    Interactable, InteractionEvents, Opacity, PointerInput, ProteusWorld, QuadState,
    TransitionRequest, Visibility,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn quad_at(x: f32, y: f32) -> QuadState {
    QuadState {
        position: Vec3::new(x, y, 0.0),
        size: Vec2::new(100.0, 100.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: 0.0,
    }
}

fn click_at(world: &mut ProteusWorld, pos: Vec2) -> Vec<Entity> {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_pressed = true;
        pi.is_pressed = true;
    }
    world.update(0.0);
    world.world.resource_mut::<PointerInput>().just_pressed = false;
    world.world.resource::<InteractionEvents>().clicked.clone()
}

// ---------------------------------------------------------------------------
// Hierarchy construction / teardown
// ---------------------------------------------------------------------------

#[test]
fn child_of_populates_children() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = world.spawn(ChildOf(parent)).id();

    let children = world.get::<Children>(parent).unwrap();
    assert_eq!(&**children, &[child]);
}

/// bevy_ecs's `ChildOf`/`Children` relationship despawns descendants
/// recursively when the parent is despawned — verifying this directly since
/// it's the mechanism M10's "no entity leaks on parent destroy" DoD item
/// relies on rather than a hand-rolled cleanup system.
#[test]
fn despawning_parent_despawns_child() {
    let mut world = World::new();
    let parent = world.spawn_empty().id();
    let child = world.spawn(ChildOf(parent)).id();
    let grandchild = world.spawn(ChildOf(child)).id();

    world.despawn(parent);

    assert!(world.get_entity(parent).is_err());
    assert!(world.get_entity(child).is_err(), "child should not leak");
    assert!(
        world.get_entity(grandchild).is_err(),
        "grandchild should not leak"
    );
}

// ---------------------------------------------------------------------------
// Visibility cascade
// ---------------------------------------------------------------------------

#[test]
fn visibility_cascade_hidden_parent_hides_child() {
    let mut world = ProteusWorld::new();
    let parent = world
        .world
        .spawn((quad_at(0.0, 0.0), Visibility::HIDDEN))
        .id();
    let child = world
        .world
        .spawn((quad_at(10.0, 0.0), Visibility::VISIBLE, ChildOf(parent)))
        .id();

    world.update(0.0);

    assert_eq!(
        world.world.get::<EffectiveVisibility>(child),
        Some(&EffectiveVisibility(false)),
        "child declares itself visible, but a hidden parent must still hide it"
    );
}

#[test]
fn visibility_cascade_visible_parent_hidden_child_only_affects_that_child() {
    let mut world = ProteusWorld::new();
    let parent = world
        .world
        .spawn((quad_at(0.0, 0.0), Visibility::VISIBLE))
        .id();
    let hidden_child = world
        .world
        .spawn((quad_at(10.0, 0.0), Visibility::HIDDEN, ChildOf(parent)))
        .id();
    let visible_child = world
        .world
        .spawn((quad_at(-10.0, 0.0), Visibility::VISIBLE, ChildOf(parent)))
        .id();

    world.update(0.0);

    assert_eq!(
        world.world.get::<EffectiveVisibility>(hidden_child),
        Some(&EffectiveVisibility(false))
    );
    assert_eq!(
        world.world.get::<EffectiveVisibility>(visible_child),
        Some(&EffectiveVisibility(true))
    );
    assert_eq!(
        world.world.get::<EffectiveVisibility>(parent),
        Some(&EffectiveVisibility(true))
    );
}

// ---------------------------------------------------------------------------
// Opacity cascade
// ---------------------------------------------------------------------------

#[test]
fn opacity_cascade_multiplies_down_chain() {
    let mut world = ProteusWorld::new();
    let parent = world.world.spawn((quad_at(0.0, 0.0), Opacity(0.5))).id();
    let child = world
        .world
        .spawn((quad_at(10.0, 0.0), Opacity(0.8), ChildOf(parent)))
        .id();

    world.update(0.0);

    let effective = world.world.get::<EffectiveOpacity>(child).unwrap().0;
    assert!(
        (effective - 0.4).abs() < 1e-5,
        "expected 0.5 * 0.8 = 0.4, got {effective}"
    );
}

#[test]
fn opacity_cascade_defaults_to_one_when_undeclared() {
    let mut world = ProteusWorld::new();
    let parent = world.world.spawn(quad_at(0.0, 0.0)).id();
    let child = world
        .world
        .spawn((quad_at(10.0, 0.0), ChildOf(parent)))
        .id();

    world.update(0.0);

    assert_eq!(
        world.world.get::<EffectiveOpacity>(parent),
        Some(&EffectiveOpacity(1.0))
    );
    assert_eq!(
        world.world.get::<EffectiveOpacity>(child),
        Some(&EffectiveOpacity(1.0))
    );
}

// ---------------------------------------------------------------------------
// End-to-end rendering
// ---------------------------------------------------------------------------

/// A child's local `QuadState` is relative to its parent — `collect_instances`
/// must resolve it to the composed world position before emitting a
/// `QuadInstance`, not the raw local offset.
#[test]
fn collect_instances_positions_child_at_composed_world_offset() {
    let mut world = ProteusWorld::new();
    let parent = world.world.spawn(quad_at(100.0, 50.0)).id();
    let _child = world
        .world
        .spawn((quad_at(10.0, -5.0), ChildOf(parent)))
        .id();

    world.update(0.0);
    let instances = collect_instances(&mut world.world);

    let positions: Vec<[f32; 3]> = instances.iter().map(|i| i.position).collect();
    assert!(
        positions
            .iter()
            .any(|p| (p[0] - 110.0).abs() < 1e-3 && (p[1] - 45.0).abs() < 1e-3),
        "expected a composed child instance at (110, 45), got {positions:?}"
    );
}

/// `collect_instances` must emit a child's instance *after* its parent's, so
/// the child draws on top (the "last pushed = on top" convention the whole
/// render pipeline relies on) — regardless of whichever order the ECS's own
/// archetype storage happens to iterate entities in. A handful of unrelated
/// root entities with varied component combinations (mimicking a Text-bearing
/// button, a plain quad, etc.) are interspersed to guard against a future
/// regression back to a flat, iteration-order-dependent collection.
#[test]
fn collect_instances_draws_child_after_parent() {
    let mut world = ProteusWorld::new();

    // Unrelated "noise" roots, spawned before and after the parent/child
    // pair, with different component shapes than either.
    world.world.spawn(quad_at(500.0, 500.0));
    let parent = world.world.spawn(quad_at(0.0, 0.0)).id();
    world.world.spawn((quad_at(-500.0, -500.0), Interactable));
    let _child = world
        .world
        .spawn((quad_at(50.0, 0.0), ChildOf(parent)))
        .id();
    world.world.spawn(quad_at(999.0, 999.0));

    world.update(0.0);
    let instances = collect_instances(&mut world.world);

    let parent_idx = instances
        .iter()
        .position(|i| (i.position[0]).abs() < 1e-3)
        .expect("parent instance (x=0) not found");
    let child_idx = instances
        .iter()
        .position(|i| (i.position[0] - 50.0).abs() < 1e-3)
        .expect("child instance (world x=50) not found");

    assert!(
        parent_idx < child_idx,
        "parent (index {parent_idx}) must be drawn before its child (index {child_idx})"
    );
}

// ---------------------------------------------------------------------------
// Independent transitions
// ---------------------------------------------------------------------------

/// A parent mid-transition and a child mid-*independent* transition must each
/// progress on their own — and the child's rendered position must compose the
/// parent's *current* (mid-lerp) world position with the child's own current
/// (also mid-lerp) local offset, not either one in isolation.
#[test]
fn child_transitions_independently_of_parent() {
    let mut world = ProteusWorld::new();
    let parent = world.world.spawn(quad_at(0.0, 0.0)).id();
    let child = world.world.spawn((quad_at(0.0, 0.0), ChildOf(parent))).id();

    let cfg = TransitionConfig {
        duration: 1.0,
        delay: 0.0,
        easing: linear,
    };

    world.world.entity_mut(parent).insert(TransitionRequest {
        to: quad_at(100.0, 0.0),
        from_state: None,
        config: cfg,
    });
    world.world.entity_mut(child).insert(TransitionRequest {
        to: quad_at(20.0, 0.0),
        from_state: None,
        config: cfg,
    });

    // First tick: TransitionSetup converts the requests into ActiveTransition.
    world.update(0.0);
    // Second tick: advance halfway through the 1s duration.
    world.update(0.5);

    let parent_qs = world.world.get::<QuadState>(parent).unwrap();
    assert!(
        (parent_qs.position.x - 50.0).abs() < 1e-3,
        "parent should be halfway from 0 to 100, got {}",
        parent_qs.position.x
    );

    let child_qs = world.world.get::<QuadState>(child).unwrap();
    assert!(
        (child_qs.position.x - 10.0).abs() < 1e-3,
        "child's own local x should be halfway from 0 to 20, got {}",
        child_qs.position.x
    );

    // Rendered (world) position must compose the parent's current 50.0 with
    // the child's current local 10.0 — i.e. 60.0 — not just one or the other.
    let instances = collect_instances(&mut world.world);
    let child_world_x = instances
        .iter()
        .find(|i| (i.position[0] - 60.0).abs() < 1e-3)
        .map(|i| i.position[0]);
    assert!(
        child_world_x.is_some(),
        "expected a rendered instance at composed world x=60, got positions {:?}",
        instances.iter().map(|i| i.position[0]).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Hit-testing
// ---------------------------------------------------------------------------

/// An `Interactable` child hit-tests against its *resolved world* position,
/// not its raw parent-relative local coordinates — otherwise a click at the
/// child's true on-screen location would silently miss.
#[test]
fn interactable_child_hit_tests_at_world_position() {
    let mut world = ProteusWorld::new();
    let parent = world.world.spawn(quad_at(200.0, 0.0)).id();
    // Local offset (10, 0) relative to the parent — world position (210, 0).
    let child = world
        .world
        .spawn((quad_at(10.0, 0.0), Interactable, ChildOf(parent)))
        .id();

    // A click at the child's raw *local* coordinates (10, 0) must miss.
    let clicked_at_local = click_at(&mut world, Vec2::new(10.0, 0.0));
    assert!(
        !clicked_at_local.contains(&child),
        "clicking the child's local coordinates should not hit it once it has a parent offset"
    );

    // A click at the child's true *world* coordinates (210, 0) must hit.
    let clicked_at_world = click_at(&mut world, Vec2::new(210.0, 0.0));
    assert_eq!(
        clicked_at_world,
        vec![child],
        "clicking the child's resolved world position should hit it"
    );
}
