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
//! | `click_at_left_boundary_hits` / `..._right_boundary_misses` | `[left, right)` half-open bounds |
//! | `top_draw_order_entity_wins_when_quads_overlap` | Overlap resolved by draw order |
//! | `rotated_quad_hit_tests_its_true_footprint_not_the_unrotated_box` | M10.6: oriented hit box |
//! | `rotated_parent_rotates_interactable_childs_hit_region_too` | M10.6: applies to children too |
//! | `top_left_anchored_quad_is_clickable_where_it_renders_not_mirrored_above_it` | Anchor is Y-down, extents are Y-up |
//! | `higher_z_entity_wins_even_though_it_was_spawned_first` | `position.z` beats spawn order |
//! | `overlap_is_decided_by_spawn_order_across_archetypes` | Not by ECS iteration order |

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use proteus_ui::{
    ChildOf, Interactable, InteractionEvents, Opacity, PointerInput, ProteusWorld, QuadState,
    Virtual, Visibility,
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

// ---------------------------------------------------------------------------
// Oriented hit-test boxes (M10.6)
// ---------------------------------------------------------------------------

/// A rotated quad's hit region must match its true (rotated) footprint, not
/// the axis-aligned bounding box of its unrotated shape.
///
/// `quad_at` is a 100×100 square. Unrotated, its axis-aligned box spans
/// [-50, 50] on each axis. Rotated 45°, its footprint becomes a diamond with
/// vertices at distance 50√2 ≈ 70.7 along each axis — i.e. `|x| + |y| <=
/// 70.7` — which is *smaller* than the original box in the corners. (45, 45)
/// sits in the original box's corner (both 45 < 50) but outside the rotated
/// diamond (45 + 45 = 90 > 70.7): exactly the point that distinguishes a
/// correct oriented test from the old axis-aligned-only one.
#[test]
fn rotated_quad_hit_tests_its_true_footprint_not_the_unrotated_box() {
    let mut world = ProteusWorld::new();
    let rotated = QuadState {
        rotation: std::f32::consts::FRAC_PI_4,
        ..quad_at(0.0, 0.0)
    };
    world.world.spawn((rotated, Interactable));

    let clicked = click_at(&mut world, Vec2::new(45.0, 45.0));
    assert!(
        clicked.is_empty(),
        "a point in the unrotated box's corner but outside the rotated diamond must miss"
    );

    let clicked = click_at(&mut world, Vec2::new(20.0, 20.0));
    assert_eq!(
        clicked.len(),
        1,
        "a point well inside the rotated footprint should still hit"
    );
}

/// A rotated parent's rotation must carry into an `Interactable` child's
/// resolved world rotation too (`hierarchy::compose_with_parent` composes
/// rotation additively) — the same diamond-vs-box distinction as the root
/// case above, just via a child whose own local rotation is zero.
#[test]
fn rotated_parent_rotates_interactable_childs_hit_region_too() {
    let mut world = ProteusWorld::new();
    let parent = world
        .world
        .spawn(QuadState {
            rotation: std::f32::consts::FRAC_PI_4,
            ..quad_at(200.0, 0.0)
        })
        .id();
    // Zero local offset — the child's world pivot lands exactly on the
    // parent's, so the same 45/45 vs 20/20 offsets from that pivot apply.
    let child = world
        .world
        .spawn((quad_at(0.0, 0.0), Interactable, ChildOf(parent)))
        .id();

    let clicked = click_at(&mut world, Vec2::new(245.0, 45.0));
    assert!(
        clicked.is_empty(),
        "child's rotated hit region should exclude the unrotated box's corner"
    );

    let clicked = click_at(&mut world, Vec2::new(220.0, 20.0));
    assert_eq!(clicked, vec![child]);
}

// ---------------------------------------------------------------------------
// Anchor handedness
// ---------------------------------------------------------------------------

/// `QuadState::anchor` is the *screen* convention — `(0, 0)` pins the
/// component's **top**-left corner at `position` — while hit-testing works in
/// world space, where Y increases upward. `quad.wgsl` reconciles the two by
/// negating Y in its `anchor_shift`; `quad_contains` has to do the same, so a
/// `(0, 0)`-anchored quad occupies the rectangle to the right of and *below*
/// its position (below = smaller world Y).
///
/// Regression test: the Y extents were previously the plain mirror of the X
/// ones (`-anchor·size` … `(1 - anchor)·size` on both axes), which mirrored the
/// hit box about the pivot — a top-left-anchored button was clickable in the
/// rectangle *above* the one it renders in. Invisible at the default centre
/// anchor, where both formulas give `±size/2`, which is every anchor the
/// reference demo and the TS example use.
#[test]
fn top_left_anchored_quad_is_clickable_where_it_renders_not_mirrored_above_it() {
    let mut world = ProteusWorld::new();
    // 100×100 at (100, 100), anchor (0, 0) → occupies x ∈ [100, 200),
    // y ∈ [0, 100): right of, and below, the pivot.
    let e = world
        .world
        .spawn((
            QuadState {
                anchor: Vec2::new(0.0, 0.0),
                ..quad_at(100.0, 100.0)
            },
            Interactable,
        ))
        .id();

    let clicked = click_at(&mut world, Vec2::new(150.0, 50.0));
    assert_eq!(
        clicked,
        vec![e],
        "a point below-right of a top-left-anchored quad's pivot is inside the \
         footprint it actually renders in"
    );

    let clicked = click_at(&mut world, Vec2::new(150.0, 150.0));
    assert!(
        clicked.is_empty(),
        "a point *above* the pivot is outside a top-left-anchored quad — if this \
         hits, the Y extents are mirrored"
    );
}

// ---------------------------------------------------------------------------
// Overlap resolution — must agree with draw order (M13.8 follow-up)
// ---------------------------------------------------------------------------

/// `collect_instances` sorts root entities by `(position.z, SpawnOrder)`, so a
/// higher-z entity draws on top no matter when it was spawned. Hit-testing must
/// resolve overlaps the same way, or the pointer lands on something the user
/// can't see.
///
/// Regression test: `position.z` was ignored here entirely — overlap was decided
/// purely by which entity the query iterated last, so the *lower*-z entity won
/// simply by being spawned second.
#[test]
fn higher_z_entity_wins_even_though_it_was_spawned_first() {
    let mut world = ProteusWorld::new();
    let front = world
        .world
        .spawn((
            QuadState {
                position: Vec3::new(100.0, 100.0, 5.0),
                ..quad_at(100.0, 100.0)
            },
            Interactable,
        ))
        .id();
    let _behind = world
        .world
        .spawn((
            QuadState {
                position: Vec3::new(100.0, 100.0, 0.0),
                ..quad_at(100.0, 100.0)
            },
            Interactable,
        ))
        .id();

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(
        clicked,
        vec![front],
        "the higher-z entity draws on top, so it must receive the click even \
         though the lower-z one was spawned later"
    );
}

/// The input-side counterpart of `render_instances.rs`'s
/// `spawning_into_an_existing_archetype_does_not_reorder_a_different_archetype`.
///
/// Two overlapping interactables in *different* archetypes: "later spawned wins"
/// has to come from an explicit `SpawnOrder` stamp, because `bevy_ecs`'s
/// iteration order across two archetypes has no defined relationship to spawn
/// order. M13.8 fixed exactly this for rendering and left input on the old
/// assumption.
///
/// Measured against the pre-fix code, this is *worse* on the input side than it
/// was on the render side: the rendering repro needed a third entity to join an
/// existing archetype before the order flipped, whereas here the very first
/// assertion below already fails — two overlapping interactables in different
/// archetypes resolved in reverse-spawn order straight away, with nothing
/// perturbing them. The third entity is kept as a stability guard rather than
/// as the trigger.
#[test]
fn overlap_is_decided_by_spawn_order_across_archetypes() {
    let mut world = ProteusWorld::new();

    // `Opacity` is the archetype differentiator on purpose: it's inert for
    // hit-testing (unlike `Disabled`, which opts out, or `InteractionDef`,
    // which would start a mini-transition and gate the entity mid-test).
    let _under = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();
    let over = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable, Opacity(1.0)))
        .id();

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(
        clicked,
        vec![over],
        "the later-spawned entity draws on top and must receive the click"
    );

    // Add a third entity to the *first* archetype — the move that reordered
    // archetype iteration in M13.8's rendering repro. It doesn't overlap, so it
    // can never be the hit; it exists only to perturb iteration order.
    world.world.spawn((quad_at(400.0, 400.0), Interactable));

    let clicked = click_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(
        clicked,
        vec![over],
        "still the later-spawned entity — an unrelated third entity joining \
         another archetype must not flip which of these two receives the click"
    );
}
