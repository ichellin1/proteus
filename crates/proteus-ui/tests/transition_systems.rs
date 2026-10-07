// Tests of the transition systems, run against a real `bevy_ecs` world:
// timing, geometry, and lifecycle.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    component::{Lifecycle, QuadState, TransitionRequest},
    transition::{
        transition_complete_system, transition_setup_system, transition_tick_system,
        ActiveTransition, CompletedTransitions, Easing, FrameTime, TransitionConfig,
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
        easing: Easing::Linear,
    }
}

// Minimal world for transition system tests.
fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<FrameTime>();
    world.init_resource::<CompletedTransitions>();
    world
}

fn set_dt(world: &mut World, dt: f32) {
    world.resource_mut::<FrameTime>().delta_secs = dt;
}

// Run a single system once against the world.
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
    // Same setup twice, different easing. At t=0.5, EaseInQuad gives 0.25
    // so the lerped position.x should be 0.25 * 200 = 50, not 100.
    let cfg = TransitionConfig {
        duration: 1.0,
        delay: 0.0,
        easing: Easing::EaseInQuad,
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
        easing: Easing::Linear,
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

// A duration of 0 means instant, and a negative or NaN one is treated as 0:
// each completes on the next tick, even one with no time step, at the target
// and without a panic.
#[test]
fn zero_negative_and_nan_durations_complete_on_the_next_tick() {
    for duration in [0.0, -1.0, f32::NAN] {
        let mut world = make_world();
        let entity = world
            .spawn((
                red(),
                Lifecycle::Idle,
                TransitionRequest {
                    to: blue(),
                    config: config(duration),
                    from_state: None,
                },
            ))
            .id();

        run(&mut world, transition_setup_system);
        set_dt(&mut world, 0.0);
        run(&mut world, transition_tick_system);

        let active = world.get::<ActiveTransition>(entity).unwrap();
        assert!(active.is_complete, "duration {duration}: complete");
        assert_eq!(
            active.config.duration, 0.0,
            "duration {duration}: stored as 0"
        );
        let state = world.get::<QuadState>(entity).unwrap();
        assert_eq!(
            state.position,
            blue().position,
            "duration {duration}: at the target"
        );
    }
}

// An instant transition still waits for its delay.
#[test]
fn an_instant_transition_waits_for_its_delay() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                config: TransitionConfig {
                    delay: 0.1,
                    ..config(0.0)
                },
                from_state: None,
            },
        ))
        .id();
    run(&mut world, transition_setup_system);

    set_dt(&mut world, 0.05);
    run(&mut world, transition_tick_system);
    assert!(!world.get::<ActiveTransition>(entity).unwrap().is_complete);

    run(&mut world, transition_tick_system);
    assert!(world.get::<ActiveTransition>(entity).unwrap().is_complete);
}

// Inserting a new `TransitionRequest` while a transition is in-flight
// (retargeting) must snapshot the **current mid-flight `QuadState`** as the
// new from-state — not the original from-state.  This ensures smooth
// motion: the animation starts from wherever it was interrupted, not from
// its original start position.
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

// ---------------------------------------------------------------------------
// A `from_state` applies as soon as the transition is set up
// ---------------------------------------------------------------------------

// A transition with a `from_state` must move the entity there as soon as it is
// set up, and keep it there through any `delay`.
//
// A channel uses `from_state` so that `to` starts from `from`'s geometry, and a
// `PerTarget` split so that every target starts from the source. If the entity
// only moved on the first tick of the transition, it would sit at its old
// position, usually its final one, through the delay, then jump back to the
// start: for a staggered split, the reverse of what's intended.
#[test]
fn a_delayed_transition_sits_at_its_from_state_for_the_whole_delay() {
    let cfg = TransitionConfig {
        duration: 1.0,
        delay: 0.5,
        easing: Easing::Linear,
    };
    let mut world = make_world();

    // The entity currently sits at `blue()` — its own resting state, and also
    // where this transition is heading. The transition is declared to start
    // from `red()` instead.
    let entity = world
        .spawn((
            blue(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                from_state: Some(red()),
                config: cfg,
            },
        ))
        .id();

    run(&mut world, transition_setup_system);
    world.flush();

    assert_eq!(
        world.get::<QuadState>(entity).unwrap().position,
        red().position,
        "setup must place the entity at `from_state` immediately — otherwise it \
         renders at its pre-transition position until the first lerping tick"
    );

    // Tick entirely within the delay: still parked at `from`, not drifting and
    // not back at its old position.
    set_dt(&mut world, 0.2);
    run(&mut world, transition_tick_system);
    assert_eq!(
        world.get::<QuadState>(entity).unwrap().position,
        red().position,
        "must stay at `from_state` for the whole delay"
    );

    // Burn the rest of the delay plus a little: now it should finally move.
    set_dt(&mut world, 0.4);
    run(&mut world, transition_tick_system);
    let moved = world.get::<QuadState>(entity).unwrap().position;
    assert_ne!(
        moved,
        red().position,
        "once the delay is exhausted the lerp should start"
    );
}

// Without a delay: the tick a transition is set up must already draw `from`,
// not the entity's old position. The transition is only picked up the tick
// after it is set up, so otherwise the entity shows where it was for one frame:
// for a channel's `to` entity, a flash of the end state before it moves.
#[test]
fn a_transition_renders_its_from_state_on_the_frame_it_is_set_up() {
    let mut world = make_world();
    let entity = world
        .spawn((
            blue(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                from_state: Some(red()),
                config: config(1.0),
            },
        ))
        .id();

    run(&mut world, transition_setup_system);
    world.flush();

    assert_eq!(
        world.get::<QuadState>(entity).unwrap().position,
        red().position,
        "the setup frame itself must show `from`, not the stale pre-transition state"
    );
}

// The no-op half of the contract: with `from_state: None` the origin *is* the
// entity's current state, so setup must leave it exactly where it is. Most
// transitions take this path: `animate_to`, interaction styles, and the
// Row/Column/Grid group paths.
#[test]
fn setup_without_a_from_state_leaves_the_entity_where_it_is() {
    let mut world = make_world();
    let entity = world
        .spawn((
            red(),
            Lifecycle::Idle,
            TransitionRequest {
                to: blue(),
                from_state: None,
                config: config(1.0),
            },
        ))
        .id();

    run(&mut world, transition_setup_system);
    world.flush();

    assert_eq!(
        world.get::<QuadState>(entity).unwrap().position,
        red().position,
        "no declared origin means the entity is already at its origin"
    );
}
