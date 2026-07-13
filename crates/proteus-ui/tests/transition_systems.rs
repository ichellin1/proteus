//! Integration tests for the M2 transition systems.
//!
//! These tests spin up a real `bevy_ecs` `World` and run systems against it,
//! verifying timing, state mutation, and lifecycle bookkeeping.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    component::{Lifecycle, QuadState, TransitionRequest},
    transition::{
        ease_in_quad, linear, transition_complete_system, transition_setup_system,
        transition_tick_system, ActiveTransition, CompletedTransitions, FrameTime,
        TransitionConfig,
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn red() -> QuadState {
    QuadState {
        position: Vec3::ZERO,
        size: Vec2::new(100.0, 100.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 0.0, 0.0, 1.0),
        corner_radius: 0.0,
    }
}

fn blue() -> QuadState {
    QuadState {
        position: Vec3::new(200.0, 0.0, 0.0),
        size: Vec2::new(200.0, 200.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        corner_radius: 8.0,
    }
}

fn config(duration: f32) -> TransitionConfig {
    TransitionConfig {
        duration,
        delay: 0.0,
        easing: linear,
    }
}

/// Minimal world for transition system tests.
fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<FrameTime>();
    world.init_resource::<CompletedTransitions>();
    world
}

fn set_dt(world: &mut World, dt: f32) {
    world.resource_mut::<FrameTime>().delta_secs = dt;
}

/// Run a single system once against the world.
fn run<M>(world: &mut World, system: impl IntoSystem<(), (), M> + 'static) {
    let mut sched = Schedule::default();
    sched.add_systems(system);
    sched.run(world);
}

// ---------------------------------------------------------------------------
// transition_setup_system
// ---------------------------------------------------------------------------

#[test]
fn setup_converts_request_to_active_transition() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                config: config(1.0),
                from_state: None,
            },
        ))
        .id();

    run(&mut world, transition_setup_system);

    // Request should be removed.
    assert!(world.get::<TransitionRequest>(entity).is_none());
    // ActiveTransition should be present.
    assert!(world.get::<ActiveTransition>(entity).is_some());
    // Lifecycle should now be Transitioning.
    assert_eq!(
        *world.get::<Lifecycle>(entity).unwrap(),
        Lifecycle::Transitioning
    );
}

#[test]
fn setup_snapshots_from_state_correctly() {
    let mut world = make_world();
    let from = red();
    let to = blue();
    let entity = world
        .spawn((
            from.clone(),
            Lifecycle::Idle,
            TransitionRequest {
                to: to.clone(),
                config: config(0.5),
                from_state: None,
            },
        ))
        .id();

    run(&mut world, transition_setup_system);

    let active = world.get::<ActiveTransition>(entity).unwrap();
    assert_eq!(active.from.color, from.color);
    assert_eq!(active.to.color, to.color);
    assert!((active.config.duration - 0.5).abs() < 1e-6);
}

// ---------------------------------------------------------------------------
// transition_tick_system
// ---------------------------------------------------------------------------

#[test]
fn tick_advances_elapsed_by_dt() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Transitioning,
            ActiveTransition::new(red(), blue(), config(1.0)),
        ))
        .id();

    set_dt(&mut world, 0.25);
    run(&mut world, transition_tick_system);

    let active = world.get::<ActiveTransition>(entity).unwrap();
    assert!((active.elapsed - 0.25).abs() < 1e-6);
    assert!(!active.is_complete);
}

#[test]
fn tick_lerps_quad_state_proportionally() {
    let mut world = make_world();
    // 1-second linear transition, red → blue
    let entity = world
        .spawn((
            red(),
            Lifecycle::Transitioning,
            ActiveTransition::new(red(), blue(), config(1.0)),
        ))
        .id();

    // Advance exactly half the duration.
    set_dt(&mut world, 0.5);
    run(&mut world, transition_tick_system);

    let state = world.get::<QuadState>(entity).unwrap();
    // Position should be halfway between 0 and 200.
    assert!(
        (state.position.x - 100.0).abs() < 1e-3,
        "x={}",
        state.position.x
    );
    // corner_radius halfway: 0 → 8 ⇒ 4
    assert!((state.corner_radius - 4.0).abs() < 1e-3);
}

#[test]
fn tick_easing_changes_lerp_output() {
    // Same setup twice, different easing. At t=0.5, ease_in_quad gives 0.25
    // so the lerped position.x should be 0.25 * 200 = 50, not 100.
    let cfg = TransitionConfig {
        duration: 1.0,
        delay: 0.0,
        easing: ease_in_quad,
    };

    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Transitioning,
            ActiveTransition::new(red(), blue(), cfg),
        ))
        .id();

    set_dt(&mut world, 0.5);
    run(&mut world, transition_tick_system);

    let state = world.get::<QuadState>(entity).unwrap();
    assert!(
        (state.position.x - 50.0).abs() < 1e-2,
        "x={}",
        state.position.x
    );
}

#[test]
fn tick_with_delay_burns_delay_before_advancing_elapsed() {
    let cfg = TransitionConfig {
        duration: 1.0,
        delay: 0.5,
        easing: linear,
    };
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Transitioning,
            ActiveTransition::new(red(), blue(), cfg),
        ))
        .id();

    // First tick: 0.2 s — entirely in delay.
    set_dt(&mut world, 0.2);
    run(&mut world, transition_tick_system);
    {
        let active = world.get::<ActiveTransition>(entity).unwrap();
        assert!(
            (active.delay_remaining - 0.3).abs() < 1e-5,
            "delay_remaining={}",
            active.delay_remaining
        );
        assert!(active.elapsed < 1e-6, "elapsed should still be 0");
    }

    // Second tick: 0.4 s — burns remaining 0.3 delay, 0.1 into elapsed.
    set_dt(&mut world, 0.4);
    run(&mut world, transition_tick_system);
    {
        let active = world.get::<ActiveTransition>(entity).unwrap();
        assert!(active.delay_remaining < 1e-6, "delay should be exhausted");
        assert!(
            (active.elapsed - 0.1).abs() < 1e-4,
            "elapsed={}",
            active.elapsed
        );
    }
}

#[test]
fn tick_clamps_t_at_one_and_sets_is_complete() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Transitioning,
            ActiveTransition::new(red(), blue(), config(0.3)),
        ))
        .id();

    // Overshoot the duration by 2×.
    set_dt(&mut world, 0.6);
    run(&mut world, transition_tick_system);

    let active = world.get::<ActiveTransition>(entity).unwrap();
    assert!(active.is_complete, "should be marked complete");
    // Final state should snap to the `to` target.
    let state = world.get::<QuadState>(entity).unwrap();
    assert!((state.position.x - blue().position.x).abs() < 1e-4);
    assert!((state.corner_radius - blue().corner_radius).abs() < 1e-4);
}

// ---------------------------------------------------------------------------
// transition_complete_system
// ---------------------------------------------------------------------------

#[test]
fn complete_records_entity_in_completed_transitions() {
    let mut world = make_world();
    let mut active = ActiveTransition::new(red(), blue(), config(1.0));
    active.is_complete = true;
    let entity = world.spawn((red(), Lifecycle::Transitioning, active)).id();

    run(&mut world, transition_complete_system);

    let completed = world.resource::<CompletedTransitions>();
    assert_eq!(completed.entities.len(), 1);
    assert_eq!(completed.entities[0], entity);
}

#[test]
fn complete_restores_lifecycle_to_idle() {
    let mut world = make_world();
    let mut active = ActiveTransition::new(red(), blue(), config(1.0));
    active.is_complete = true;
    let entity = world.spawn((red(), Lifecycle::Transitioning, active)).id();

    run(&mut world, transition_complete_system);

    assert_eq!(*world.get::<Lifecycle>(entity).unwrap(), Lifecycle::Idle);
}

#[test]
fn complete_removes_active_transition_component() {
    let mut world = make_world();
    let mut active = ActiveTransition::new(red(), blue(), config(1.0));
    active.is_complete = true;
    let entity = world.spawn((red(), Lifecycle::Transitioning, active)).id();

    run(&mut world, transition_complete_system);
    // Commands are deferred — flush them.
    world.flush();

    assert!(
        world.get::<ActiveTransition>(entity).is_none(),
        "ActiveTransition should be removed after completion"
    );
}

#[test]
fn complete_clears_previous_frame_results() {
    // Pre-populate CompletedTransitions with a stale entity id.
    let mut world = make_world();
    let stale = world.spawn_empty().id();
    {
        let mut c = world.resource_mut::<CompletedTransitions>();
        c.entities.push(stale);
    }
    // Spawn a non-complete entity so the system runs but fires nothing.
    world.spawn((
        red(),
        Lifecycle::Transitioning,
        ActiveTransition::new(red(), blue(), config(1.0)),
    ));

    run(&mut world, transition_complete_system);

    let completed = world.resource::<CompletedTransitions>();
    assert!(
        completed.entities.is_empty(),
        "stale entries should be cleared"
    );
}

// ---------------------------------------------------------------------------
// End-to-end: setup → tick → complete
// ---------------------------------------------------------------------------

#[test]
fn full_transition_fires_complete_after_sufficient_ticks() {
    let mut world = make_world();
    // 0.2-second linear transition.
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                config: config(0.2),
                from_state: None,
            },
        ))
        .id();

    // Phase 1: setup converts the request.
    run(&mut world, transition_setup_system);
    assert_eq!(
        *world.get::<Lifecycle>(entity).unwrap(),
        Lifecycle::Transitioning
    );

    // Phase 2: tick with 0.1 s — halfway, not yet complete.
    set_dt(&mut world, 0.1);
    run(&mut world, transition_tick_system);
    assert!(!world.get::<ActiveTransition>(entity).unwrap().is_complete);

    run(&mut world, transition_complete_system);
    assert!(world.resource::<CompletedTransitions>().entities.is_empty());
    assert_eq!(
        *world.get::<Lifecycle>(entity).unwrap(),
        Lifecycle::Transitioning
    );

    // Phase 3: tick with another 0.2 s — overshoots, marks complete.
    set_dt(&mut world, 0.2);
    run(&mut world, transition_tick_system);
    assert!(world.get::<ActiveTransition>(entity).unwrap().is_complete);

    // Phase 4: complete system fires.
    run(&mut world, transition_complete_system);
    assert_eq!(
        world.resource::<CompletedTransitions>().entities,
        vec![entity]
    );
    assert_eq!(*world.get::<Lifecycle>(entity).unwrap(), Lifecycle::Idle);

    // Flush deferred removes.
    world.flush();
    assert!(world.get::<ActiveTransition>(entity).is_none());
}
