//! Integration tests for the M12.2 interaction system: `hit_test_system`'s
//! press/release/drag/focus additions and gating, plus
//! `interaction_style_system`'s per-state style resolution.

use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    component::{Disabled, Lifecycle, TransitioningConfig},
    input::{FocusState, InteractionEvents, PointerInput, PressedEntity},
    interaction::{InteractionDef, InteractionState, InteractionStateKind, StyleOverride},
    Interactable, ProteusWorld, QuadState, TransitionRequest,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A 100 × 100 center-anchored quad at (`x`, `y`).
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

/// `dt` big enough to run any interaction-style mini-transition
/// (`STYLE_TRANSITION_CONFIG`'s 0.15s duration) to completion within a single
/// `update()` call — `auto_insert_apply_deferred` (bevy_ecs's default) makes
/// the whole TransitionRequest → ActiveTransition → tick-to-completion →
/// Lifecycle::Idle cycle happen in one frame when `dt` comfortably exceeds
/// the transition's duration, so tests can assert on fully-settled state
/// between steps instead of a mid-flight snapshot.
const SETTLE_DT: f32 = 1.0;

fn press_at(world: &mut ProteusWorld, pos: Vec2) {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_pressed = true;
        pi.is_pressed = true;
    }
    world.update(SETTLE_DT);
    world.world.resource_mut::<PointerInput>().just_pressed = false;
}

fn move_while_pressed(world: &mut ProteusWorld, pos: Vec2) {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_pressed = false;
        pi.is_pressed = true;
    }
    world.update(SETTLE_DT);
}

fn release_at(world: &mut ProteusWorld, pos: Vec2) {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_released = true;
        pi.is_pressed = false;
    }
    world.update(SETTLE_DT);
    world.world.resource_mut::<PointerInput>().just_released = false;
}

fn move_to(world: &mut ProteusWorld, pos: Vec2) {
    {
        let mut pi = world.world.resource_mut::<PointerInput>();
        pi.position = Some(pos);
        pi.just_pressed = false;
        pi.is_pressed = false;
    }
    world.update(SETTLE_DT);
}

// ---------------------------------------------------------------------------
// pressed / released / dragged events
// ---------------------------------------------------------------------------

#[test]
fn press_then_release_fires_paired_events() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(world.world.resource::<InteractionEvents>().pressed, vec![e]);
    assert_eq!(
        world.world.resource::<PressedEntity>().entity,
        Some(e),
        "PressedEntity must track the pressed entity across frames"
    );

    release_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(
        world.world.resource::<InteractionEvents>().released,
        vec![e]
    );
    assert_eq!(world.world.resource::<PressedEntity>().entity, None);
}

#[test]
fn release_fires_even_if_pointer_moved_off_the_entity() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));
    move_while_pressed(&mut world, Vec2::new(400.0, 400.0)); // drag off the entity
    release_at(&mut world, Vec2::new(400.0, 400.0));

    assert_eq!(
        world.world.resource::<InteractionEvents>().released,
        vec![e],
        "release fires on whatever was pressed, regardless of current pointer position"
    );
}

#[test]
fn dragged_reports_zero_delta_on_press_frame_then_real_deltas() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));
    let first = world.world.resource::<InteractionEvents>().dragged.clone();
    assert_eq!(first, vec![(e, Vec2::ZERO)]);

    move_while_pressed(&mut world, Vec2::new(130.0, 90.0));
    let second = world.world.resource::<InteractionEvents>().dragged.clone();
    assert_eq!(second, vec![(e, Vec2::new(30.0, -10.0))]);
}

#[test]
fn no_dragged_events_while_not_pressed() {
    let mut world = ProteusWorld::new();
    world.world.spawn((quad_at(100.0, 100.0), Interactable));

    move_to(&mut world, Vec2::new(100.0, 100.0));
    assert!(world
        .world
        .resource::<InteractionEvents>()
        .dragged
        .is_empty());
}

// ---------------------------------------------------------------------------
// focus / blur
// ---------------------------------------------------------------------------

#[test]
fn click_focuses_the_clicked_entity() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));

    assert_eq!(world.world.resource::<FocusState>().focused, Some(e));
    assert_eq!(world.world.resource::<InteractionEvents>().focused, vec![e]);
}

#[test]
fn clicking_a_different_entity_blurs_the_old_one() {
    let mut world = ProteusWorld::new();
    let a = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();
    let b = world
        .world
        .spawn((quad_at(300.0, 300.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(world.world.resource::<FocusState>().focused, Some(a));

    press_at(&mut world, Vec2::new(300.0, 300.0));
    assert_eq!(world.world.resource::<FocusState>().focused, Some(b));
    assert_eq!(world.world.resource::<InteractionEvents>().blurred, vec![a]);
    assert_eq!(world.world.resource::<InteractionEvents>().focused, vec![b]);
}

#[test]
fn clicking_empty_space_does_not_change_focus() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));
    assert_eq!(world.world.resource::<FocusState>().focused, Some(e));

    press_at(&mut world, Vec2::new(900.0, 900.0)); // empty space
    assert_eq!(
        world.world.resource::<FocusState>().focused,
        Some(e),
        "clicking empty space must not blur the currently focused entity"
    );
    assert!(world
        .world
        .resource::<InteractionEvents>()
        .blurred
        .is_empty());
}

// ---------------------------------------------------------------------------
// gating: Disabled / TransitioningConfig
// ---------------------------------------------------------------------------

#[test]
fn disabled_entity_is_not_hit_testable() {
    let mut world = ProteusWorld::new();
    world
        .world
        .spawn((quad_at(100.0, 100.0), Interactable, Disabled));

    press_at(&mut world, Vec2::new(100.0, 100.0));

    assert!(world
        .world
        .resource::<InteractionEvents>()
        .pressed
        .is_empty());
    assert!(world
        .world
        .resource::<InteractionEvents>()
        .clicked
        .is_empty());
    assert_eq!(world.world.resource::<FocusState>().focused, None);
}

#[test]
fn transitioning_entity_without_allow_input_is_not_hit_testable() {
    let mut world = ProteusWorld::new();
    world.world.spawn((
        quad_at(100.0, 100.0),
        Interactable,
        Lifecycle::Transitioning,
    ));

    press_at(&mut world, Vec2::new(100.0, 100.0));

    assert!(world
        .world
        .resource::<InteractionEvents>()
        .pressed
        .is_empty());
}

#[test]
fn transitioning_entity_with_allow_input_is_hit_testable() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((
            quad_at(100.0, 100.0),
            Interactable,
            Lifecycle::Transitioning,
            TransitioningConfig {
                allow_input: true,
                allow_navigation: false,
            },
        ))
        .id();

    press_at(&mut world, Vec2::new(100.0, 100.0));

    assert_eq!(world.world.resource::<InteractionEvents>().pressed, vec![e]);
}

// ---------------------------------------------------------------------------
// interaction_style_system: precedence and mini-transitions
// ---------------------------------------------------------------------------

#[test]
fn first_frame_only_captures_declared_state_no_transition_request() {
    let mut world = ProteusWorld::new();
    let base = quad_at(100.0, 100.0);
    let e = world
        .world
        .spawn((
            base.clone(),
            Interactable,
            InteractionDef {
                hover: Some(StyleOverride {
                    color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .id();

    // Pointer already over the entity on its very first observed frame.
    move_to(&mut world, Vec2::new(100.0, 100.0));

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(
        state.current,
        InteractionStateKind::Default,
        "first-seen frame must baseline to Default even if already hovered"
    );
    assert_eq!(state.declared.color, base.color);
    assert!(
        world.world.get::<TransitionRequest>(e).is_none(),
        "no mini-transition should fire on the very first frame"
    );
}

#[test]
fn hover_triggers_mini_transition_to_resolved_override() {
    let mut world = ProteusWorld::new();
    let base = quad_at(100.0, 100.0);
    let e = world
        .world
        .spawn((
            base.clone(),
            Interactable,
            InteractionDef {
                hover: Some(StyleOverride {
                    color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
                    corner_radius: Some(8.0),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .id();

    // Frame 1: establish baseline (not yet hovered).
    move_to(&mut world, Vec2::new(900.0, 900.0));
    // Frame 2: pointer moves onto the entity.
    move_to(&mut world, Vec2::new(100.0, 100.0));

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(state.current, InteractionStateKind::Hover);

    // SETTLE_DT runs the mini-transition to completion within this same
    // update() call, so QuadState already reflects the resolved target.
    let quad = world.world.get::<QuadState>(e).unwrap();
    assert_eq!(quad.color, Vec4::new(0.0, 1.0, 0.0, 1.0));
    assert_eq!(quad.corner_radius, 8.0);
    // Undeclared fields (size, position, ...) must pass through from declared.
    assert_eq!(quad.size, base.size);
}

#[test]
fn returning_to_default_targets_declared_state() {
    let mut world = ProteusWorld::new();
    let base = quad_at(100.0, 100.0);
    let e = world
        .world
        .spawn((
            base.clone(),
            Interactable,
            InteractionDef {
                hover: Some(StyleOverride {
                    color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .id();

    move_to(&mut world, Vec2::new(900.0, 900.0)); // baseline
    move_to(&mut world, Vec2::new(100.0, 100.0)); // hover in
    move_to(&mut world, Vec2::new(900.0, 900.0)); // hover out

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(state.current, InteractionStateKind::Default);
    let quad = world.world.get::<QuadState>(e).unwrap();
    assert_eq!(quad.color, base.color);
}

#[test]
fn pressed_takes_precedence_over_hover() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((
            quad_at(100.0, 100.0),
            Interactable,
            InteractionDef {
                hover: Some(StyleOverride {
                    color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
                    ..Default::default()
                }),
                pressed: Some(StyleOverride {
                    color: Some(Vec4::new(1.0, 0.0, 0.0, 1.0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .id();

    move_to(&mut world, Vec2::new(900.0, 900.0)); // baseline
                                                  // Pressing also implies hovering (pointer is over the entity), but
                                                  // Pressed must win the precedence.
    press_at(&mut world, Vec2::new(100.0, 100.0));

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(state.current, InteractionStateKind::Pressed);
    let quad = world.world.get::<QuadState>(e).unwrap();
    assert_eq!(quad.color, Vec4::new(1.0, 0.0, 0.0, 1.0));
}

#[test]
fn disabled_takes_precedence_over_everything() {
    let mut world = ProteusWorld::new();
    let e = world
        .world
        .spawn((
            quad_at(100.0, 100.0),
            InteractionDef {
                disabled: Some(StyleOverride {
                    color: Some(Vec4::new(0.5, 0.5, 0.5, 1.0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
            Disabled,
        ))
        .id();

    // No Interactable here — disabled resolution doesn't depend on
    // hit-testing at all, only on the Disabled marker.
    world.update(SETTLE_DT); // baseline frame
    world.update(SETTLE_DT); // resolve

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(state.current, InteractionStateKind::Disabled);
    let quad = world.world.get::<QuadState>(e).unwrap();
    assert_eq!(quad.color, Vec4::new(0.5, 0.5, 0.5, 1.0));
}

#[test]
fn undeclared_override_falls_back_to_declared_unchanged() {
    let mut world = ProteusWorld::new();
    let base = quad_at(100.0, 100.0);
    let e = world
        .world
        .spawn((
            base.clone(),
            Interactable,
            InteractionDef::default(), // no hover override declared at all
        ))
        .id();

    move_to(&mut world, Vec2::new(900.0, 900.0)); // baseline
    move_to(&mut world, Vec2::new(100.0, 100.0)); // hover in, no override

    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(state.current, InteractionStateKind::Hover);
    let quad = world.world.get::<QuadState>(e).unwrap();
    assert_eq!(
        *quad, base,
        "with no hover override declared, the target must equal the declared state exactly"
    );
}

#[test]
fn interaction_style_system_does_not_touch_a_transitioning_entity() {
    let mut world = ProteusWorld::new();
    let base = quad_at(100.0, 100.0);
    let e = world
        .world
        .spawn((
            base.clone(),
            Interactable,
            InteractionDef {
                hover: Some(StyleOverride {
                    color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ))
        .id();

    move_to(&mut world, Vec2::new(900.0, 900.0)); // baseline: InteractionState inserted

    // Force the entity into a big signal-driven transition.
    world.world.entity_mut(e).insert(Lifecycle::Transitioning);

    move_to(&mut world, Vec2::new(100.0, 100.0)); // would-be hover-in frame

    assert!(
        world.world.get::<TransitionRequest>(e).is_none(),
        "interaction_style_system must not compete with a live Lifecycle::Transitioning entity"
    );
    let state = world.world.get::<InteractionState>(e).unwrap();
    assert_eq!(
        state.current,
        InteractionStateKind::Default,
        "InteractionState must be left untouched while the entity is Transitioning"
    );
}
