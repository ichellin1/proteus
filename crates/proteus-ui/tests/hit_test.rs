//! M7 hit-testing regression tests.
//!
//! These tests verify the `hit_test_system` and the full
//! `PointerInput` → `InteractionEvents` data flow without requiring a GPU or a
//! real window.
//!
//! ## Test matrix
//!
//! | Test | What it guards |
//! |---|---|
//! | `correct_entity_found_under_cursor` | Basic hit detection |
//! | `no_click_outside_bounds` | Miss produces no events |
//! | `hidden_entity_not_hit_testable` | `Visibility::HIDDEN` opt-out |
//! | `virtual_entity_not_hit_testable` | `Virtual` opt-out |
//! | `hover_enter_then_exit` | `hover_entered` / `hover_exited` lifecycle |

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use proteus_ui::{
    Interactable, InteractionEvents, PointerInput, ProteusWorld, QuadState, Virtual, Visibility,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A 100 × 100 center-anchored quad at (`x`, `y`).
///
/// Occupies the half-open rectangle \[x−50, x+50) × \[y−50, y+50) in
/// window-space pixels.
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

/// Set the pointer to `pos` with `just_pressed = true`, run one update, then
/// clear the one-shot flag.  Returns the `clicked` vec from that frame.
fn click_at(world: &mut ProteusWorld, pos: Vec2) -> Vec<Entity> {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_pressed = true;
        pi.is_pressed = true;
    }
    world.update(0.0);
    // Clear the one-shot flag so it doesn't leak into follow-up calls.
    world.world.resource_mut::<PointerInput>().just_pressed = false;

    world.world.resource::<InteractionEvents>().clicked.clone()
}

/// Move the pointer to `pos` (no click) and run one update.
fn move_to(world: &mut ProteusWorld, pos: Option<Vec2>) {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = pos;
        pi.just_pressed = false;
    }
    world.update(0.0);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// An `Interactable` entity is added to `clicked` when the pointer is inside
/// its AABB on the frame `just_pressed` is true.
#[test]
fn correct_entity_found_under_cursor() {
    let mut world = ProteusWorld::new();
    // Quad occupies [50, 150) × [50, 150); center = (100, 100).
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));

    assert_eq!(clicked, vec![e], "entity under cursor should be clicked");
}

/// A pointer position that lies outside every entity's bounds produces an
/// empty `clicked` vec, even if `just_pressed` is true.
#[test]
fn no_click_outside_bounds() {
    let mut world = ProteusWorld::new();
    // Quad at (100, 100) occupies [50, 150) × [50, 150).
    world.world.spawn((quad_at(100.0, 100.0), Interactable));

    // (300, 300) is well outside that range.
    let clicked = click_at(&mut world, Vec2::new(300.0, 300.0));

    assert!(
        clicked.is_empty(),
        "click outside bounds should produce no events"
    );
}

/// Entities with `Visibility::HIDDEN` must not appear in `clicked` regardless
/// of whether the pointer is inside their bounds.
#[test]
fn hidden_entity_not_hit_testable() {
    let mut world = ProteusWorld::new();
    world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable, Visibility::HIDDEN));

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));

    assert!(
        clicked.is_empty(),
        "hidden entity should not be hit-testable"
    );
}

/// Entities marked `Virtual` must not appear in `clicked` even if they have
/// `Interactable` and the pointer is inside their bounds.
///
/// Virtual entities are the ephemeral participants in group transitions; they
/// should never receive user interaction events.
#[test]
fn virtual_entity_not_hit_testable() {
    let mut world = ProteusWorld::new();
    world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable, Virtual));

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));

    assert!(
        clicked.is_empty(),
        "virtual entity should not be hit-testable"
    );
}

/// Clicking exactly on the left edge of a quad's AABB must register as a hit.
/// `quad_contains` is defined as `[left, right)` — the left boundary is inclusive.
///
/// Quad at (100, 100) with size 100×100 and center anchor occupies x ∈ [50, 150).
/// x=50 is the left edge and must be inside the bounds.
#[test]
fn click_at_left_boundary_hits() {
    let mut world = ProteusWorld::new();
    // Bounds: x ∈ [50, 150), y ∈ [50, 150).
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    let clicked = click_at(&mut world, Vec2::new(50.0, 100.0));

    assert_eq!(
        clicked,
        vec![e],
        "left boundary x=50 must be inside [50, 150)"
    );
}

/// Clicking exactly on the right edge of a quad's AABB must register as a miss.
/// `quad_contains` is defined as `[left, right)` — the right boundary is exclusive.
///
/// Quad at (100, 100) with size 100×100 and center anchor occupies x ∈ [50, 150).
/// x=150 is the right edge and must be outside the bounds.
#[test]
fn click_at_right_boundary_misses() {
    let mut world = ProteusWorld::new();
    // Bounds: x ∈ [50, 150), y ∈ [50, 150).
    world.world.spawn((quad_at(100.0, 100.0), Interactable));

    let clicked = click_at(&mut world, Vec2::new(150.0, 100.0));

    assert!(
        clicked.is_empty(),
        "right boundary x=150 must be outside [50, 150)"
    );
}

/// When two quads overlap the pointer position, the one inserted later into the
/// ECS world wins — matching GPU draw order (last drawn = visually on top).
/// This specifies the current semantics; the alternative (first insertion wins)
/// would be equally valid but different.
#[test]
fn top_draw_order_entity_wins_when_quads_overlap() {
    let mut world = ProteusWorld::new();

    // Both quads centered at (100, 100), same size — fully overlapping.
    let _bottom = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();
    let top = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));

    assert_eq!(
        clicked,
        vec![top],
        "later-inserted (top draw order) entity must win over earlier-inserted"
    );
}

/// `hover_entered` fires on the first frame the pointer overlaps an entity;
/// `hover_exited` fires on the first frame the pointer no longer overlaps it.
#[test]
fn hover_enter_then_exit() {
    let mut world = ProteusWorld::new();
    // Quad occupies [50, 150) × [50, 150).
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    // Frame 1: pointer enters the entity.
    move_to(&mut world, Some(Vec2::new(100.0, 100.0)));
    let entered = world
        .world
        .resource::<InteractionEvents>()
        .hover_entered
        .clone();
    assert_eq!(
        entered,
        vec![e],
        "hover_entered should fire on the first frame the pointer overlaps"
    );
    // hover_exited must be empty on the enter frame.
    assert!(
        world
            .world
            .resource::<InteractionEvents>()
            .hover_exited
            .is_empty(),
        "hover_exited must be empty on the enter frame"
    );

    // Frame 2: pointer stays inside — no new enter/exit events.
    move_to(&mut world, Some(Vec2::new(110.0, 110.0)));
    assert!(
        world
            .world
            .resource::<InteractionEvents>()
            .hover_entered
            .is_empty(),
        "hover_entered should not fire again while pointer stays inside"
    );
    assert!(
        world
            .world
            .resource::<InteractionEvents>()
            .hover_exited
            .is_empty(),
        "hover_exited should not fire while pointer stays inside"
    );

    // Frame 3: pointer leaves — hover_exited fires.
    move_to(&mut world, Some(Vec2::new(300.0, 300.0)));
    let exited = world
        .world
        .resource::<InteractionEvents>()
        .hover_exited
        .clone();
    assert_eq!(
        exited,
        vec![e],
        "hover_exited should fire when pointer leaves the entity"
    );
}
