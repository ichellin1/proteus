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

// ---------------------------------------------------------------------------
// Adversarial / boundary tests
// ---------------------------------------------------------------------------

/// A near-zero duration (1e-5 s) is below the `1e-4` clamp floor in
/// `ActiveTransition::new`.  The stored duration must be clamped up to `1e-4`,
/// and any realistic frame dt (≥ 1/240 s ≈ 4 ms) must satisfy `raw_t >= 1.0`
/// on the very first tick — completing the transition immediately without NaN.
///
/// Note: `duration = 0.0` would trigger a `debug_assert!` in
/// `ActiveTransition::new` (intentional loud-caller-error warning).  We use
/// `1e-5` here — positive so the assert is silent, but well below the clamp
/// floor so we still exercise the clamping and instant-completion paths.
#[test]
fn near_zero_duration_transition_completes_in_first_tick() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                config: config(1e-5), // tiny positive — clamped to 1e-4 internally
                from_state: None,
            },
        ))
        .id();

    run(&mut world, transition_setup_system);

    // Stored duration must have been clamped to at least 1e-4.
    let stored_duration = world
        .get::<ActiveTransition>(entity)
        .unwrap()
        .config
        .duration;
    assert!(
        (stored_duration - 1e-4).abs() < 1e-9,
        "duration below 1e-4 must be clamped to exactly 1e-4, got {stored_duration}"
    );

    // One tick with a realistic 60 Hz frame time — massively overshoots 0.1 ms.
    let one_frame_dt = 1.0 / 60.0; // ~16.7 ms >> 0.1 ms
    set_dt(&mut world, one_frame_dt);
    run(&mut world, transition_tick_system);

    let active = world.get::<ActiveTransition>(entity).unwrap();
    assert!(
        active.is_complete,
        "near-zero-duration transition must complete within the first tick"
    );
    // Final QuadState must snap to the `to` target with no NaN or Inf.
    let state = world.get::<QuadState>(entity).unwrap();
    assert!(
        state.position.x.is_finite(),
        "position must not be NaN or Inf after near-zero-duration transition"
    );
    assert!(
        (state.position.x - blue().position.x).abs() < 1e-3,
        "state must snap to `to` target, got x={}",
        state.position.x
    );
}

/// Inserting a new `TransitionRequest` while a transition is in-flight
/// (retargeting) must snapshot the **current mid-flight `QuadState`** as the
/// new from-state — not the original from-state.  This ensures smooth
/// motion: the animation starts from wherever it was interrupted, not from
/// its original start position.
#[test]
fn retargeting_midtransition_starts_from_current_state() {
    // A third QuadState to retarget to — distinct from red() and blue().
    let green = QuadState {
        position: Vec3::new(400.0, 0.0, 0.0),
        size: Vec2::new(50.0, 50.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.0, 1.0, 0.0, 1.0),
        corner_radius: 0.0,
    };

    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                config: config(1.0), // 1-second linear red → blue
                from_state: None,
            },
        ))
        .id();

    // Phase 1: convert the request to an active transition.
    run(&mut world, transition_setup_system);

    // Phase 2: advance exactly 0.5 s — QuadState is now mid-flight at t=0.5.
    set_dt(&mut world, 0.5);
    run(&mut world, transition_tick_system);

    // Verify we are at the expected midpoint (position.x = 100).
    let mid_state = world.get::<QuadState>(entity).unwrap().clone();
    assert!(
        (mid_state.position.x - 100.0).abs() < 1e-3,
        "expected mid-flight x=100, got {}",
        mid_state.position.x
    );

    // Phase 3: retarget — insert a new TransitionRequest while still in-flight.
    world.entity_mut(entity).insert(TransitionRequest {
        to: green.clone(),
        config: config(1.0),
        from_state: None, // from_state=None means "snapshot current QuadState"
    });

    // Phase 4: setup system runs — should replace the ActiveTransition,
    // snapshotting the mid-flight QuadState as the new from-state.
    run(&mut world, transition_setup_system);

    let new_active = world.get::<ActiveTransition>(entity).unwrap();

    // from-state must match the mid-flight snapshot, not the original red().
    assert!(
        (new_active.from.position.x - 100.0).abs() < 1e-3,
        "retargeted from.position.x must be the mid-flight value (100), got {}",
        new_active.from.position.x
    );

    // to-state must be the new target (green).
    assert!(
        (new_active.to.position.x - 400.0).abs() < 1e-3,
        "retargeted to.position.x must be green (400), got {}",
        new_active.to.position.x
    );

    // elapsed resets to zero for the fresh transition.
    assert!(
        new_active.elapsed < 1e-6,
        "elapsed must reset to 0 on retarget, got {}",
        new_active.elapsed
    );
}
