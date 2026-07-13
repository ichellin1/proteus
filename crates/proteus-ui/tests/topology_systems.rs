//! Integration tests for M3 topology systems.
//!
//! Tests cover:
//! - `one_to_n_setup_system` (bake and slice strategies)
//! - `n_to_one_setup_system` (slice strategy)
//! - `group_transition_complete_system`
//! - Button → list → button round trip

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    component::{Lifecycle, TransitionRequest, Virtual, Visibility},
    topology::{
        group_transition_complete_system, horizontal_slices, n_to_one_setup_system,
        one_to_n_setup_system, ActiveGroupTransition, GroupSource, GroupTarget, NToOneRequest,
        OneToNRequest, SplitStrategy,
    },
    transition::{
        linear, transition_tick_system, ActiveTransition, CompletedTransitions, FrameTime,
        TransitionConfig,
    },
    QuadState,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<FrameTime>();
    world.init_resource::<CompletedTransitions>();
    world
}

fn set_dt(world: &mut World, dt: f32) {
    world.resource_mut::<FrameTime>().delta_secs = dt;
}

fn run<M>(world: &mut World, system: impl IntoSystem<(), (), M> + 'static) {
    let mut sched = Schedule::default();
    sched.add_systems(system);
    sched.run(world);
}

fn red() -> QuadState {
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

fn default_cfg() -> TransitionConfig {
    TransitionConfig {
        duration: 0.5,
        delay: 0.0,
        easing: linear,
    }
}

/// Count entities in the world that carry a given component.
fn count_with<C: Component>(world: &mut World) -> usize {
    let mut q = world.query::<&C>();
    q.iter(world).count()
}

/// Spawn N target entities (small blue squares at evenly-spaced positions).
fn spawn_targets(world: &mut World, n: usize) -> Vec<Entity> {
    (0..n)
        .map(|i| {
            world
                .spawn((
                    QuadState {
                        position: Vec3::new(-200.0 + i as f32 * 100.0, 150.0, 0.5),
                        size: Vec2::new(60.0, 60.0),
                        color: Vec4::new(0.0, 0.5, 1.0, 1.0),
                        ..Default::default()
                    },
                    Lifecycle::Idle,
                ))
                .id()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// horizontal_slices unit-level sanity checks (quick smoke tests)
// ---------------------------------------------------------------------------

#[test]
fn horizontal_slices_total_width_equals_source() {
    let src = red();
    let slices = horizontal_slices(&src, 5);
    let total_w: f32 = slices.iter().map(|s| s.size.x).sum();
    assert!((total_w - src.size.x).abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// OneToNRequest — Bake strategy
// ---------------------------------------------------------------------------

#[test]
fn bake_1_to_n_hides_source() {
    let mut world = make_world();

    let targets = spawn_targets(&mut world, 5);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    let source = world
        .spawn((
            red(),
            Lifecycle::Idle,
            OneToNRequest {
                targets: group_targets,
                default_config: default_cfg(),
                child_behavior: None,
                strategy: SplitStrategy::Bake,
            },
        ))
        .id();

    run(&mut world, one_to_n_setup_system);
    world.flush(); // apply deferred commands (Visibility insert, Lifecycle insert)

    // Source must be hidden and Idle (no active transition on source itself).
    let vis = world
        .get::<Visibility>(source)
        .expect("Visibility should be set");
    assert!(!vis.visible, "source should be hidden after bake 1→N setup");
    assert_eq!(*world.get::<Lifecycle>(source).unwrap(), Lifecycle::Idle);
}

#[test]
fn bake_1_to_n_gives_each_target_a_transition_request() {
    let mut world = make_world();

    let targets = spawn_targets(&mut world, 5);
    let source_state = red();
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    let _source = world
        .spawn((
            source_state.clone(),
            Lifecycle::Idle,
            OneToNRequest {
                targets: group_targets,
                default_config: default_cfg(),
                child_behavior: None,
                strategy: SplitStrategy::Bake,
            },
        ))
        .id();

    run(&mut world, one_to_n_setup_system);
    world.flush(); // apply deferred inserts

    // Each target should have a TransitionRequest with from_state = source geometry.
    for &target_entity in &targets {
        let req = world
            .get::<TransitionRequest>(target_entity)
            .expect("target should have TransitionRequest after bake setup");
        let from = req.from_state.as_ref().expect("from_state should be Some");
        assert_eq!(from.position, source_state.position);
        assert_eq!(from.size, source_state.size);
    }
}

#[test]
fn bake_1_to_n_no_virtual_entities() {
    let mut world = make_world();

    let targets = spawn_targets(&mut world, 5);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: default_cfg(),
            child_behavior: None,
            strategy: SplitStrategy::Bake,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    assert_eq!(
        count_with::<Virtual>(&mut world),
        0,
        "bake should produce no virtual entities"
    );
}

#[test]
fn bake_1_to_n_child_behavior_overrides_config() {
    fn stagger(idx: usize, _total: usize) -> TransitionConfig {
        TransitionConfig {
            duration: 0.1 + idx as f32 * 0.1,
            delay: 0.0,
            easing: linear,
        }
    }

    let mut world = make_world();
    let targets = spawn_targets(&mut world, 3);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: default_cfg(),
            child_behavior: Some(stagger),
            strategy: SplitStrategy::Bake,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Target 0 → duration 0.1, target 1 → 0.2, target 2 → 0.3
    for (i, &target_entity) in targets.iter().enumerate() {
        let req = world.get::<TransitionRequest>(target_entity).unwrap();
        let expected_duration = 0.1 + i as f32 * 0.1;
        assert!(
            (req.config.duration - expected_duration).abs() < 1e-5,
            "target {i}: expected duration {expected_duration}, got {}",
            req.config.duration
        );
    }
}

// ---------------------------------------------------------------------------
// OneToNRequest — Slice strategy
// ---------------------------------------------------------------------------

#[test]
fn slice_1_to_n_creates_n_virtual_entities() {
    let mut world = make_world();

    let n = 5;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: default_cfg(),
            child_behavior: None,
            strategy: SplitStrategy::Slice,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    assert_eq!(
        count_with::<Virtual>(&mut world),
        n,
        "slice 1→N should spawn exactly {n} virtual entities"
    );
}

#[test]
fn slice_1_to_n_hides_source_and_targets() {
    let mut world = make_world();

    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    let source = world
        .spawn((
            red(),
            Lifecycle::Idle,
            OneToNRequest {
                targets: group_targets,
                default_config: default_cfg(),
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            },
        ))
        .id();

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Source must be hidden.
    assert!(
        !world.get::<Visibility>(source).unwrap().visible,
        "source should be hidden"
    );
    // All targets must be hidden during the slice transition.
    for &t in &targets {
        assert!(
            !world.get::<Visibility>(t).unwrap().visible,
            "target should be hidden during slice transition"
        );
    }
}

#[test]
fn slice_1_to_n_source_has_active_group_transition() {
    let mut world = make_world();
    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    let source = world
        .spawn((
            red(),
            Lifecycle::Idle,
            OneToNRequest {
                targets: group_targets,
                default_config: default_cfg(),
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            },
        ))
        .id();

    run(&mut world, one_to_n_setup_system);
    world.flush();

    let coordinator = world
        .get::<ActiveGroupTransition>(source)
        .expect("source should carry ActiveGroupTransition");
    assert_eq!(coordinator.total, n);
    assert_eq!(coordinator.reveal_on_complete.len(), n);
}

#[test]
fn slice_1_to_n_virtuals_have_active_transitions() {
    let mut world = make_world();
    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: default_cfg(),
            child_behavior: None,
            strategy: SplitStrategy::Slice,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Every virtual entity must have an ActiveTransition.
    let n_with_active = {
        let mut q = world.query_filtered::<&ActiveTransition, With<Virtual>>();
        q.iter(&world).count()
    };
    assert_eq!(n_with_active, n);
}

// ---------------------------------------------------------------------------
// Slice group transition: tick → complete lifecycle
// ---------------------------------------------------------------------------

#[test]
fn slice_1_to_n_complete_reveals_targets_and_despawns_virtuals() {
    let mut world = make_world();
    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    let _source = world
        .spawn((
            red(),
            Lifecycle::Idle,
            OneToNRequest {
                targets: group_targets,
                default_config: default_cfg(),
                child_behavior: None,
                strategy: SplitStrategy::Slice,
            },
        ))
        .id();

    // Setup.
    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Overshoot the transition — all virtuals should complete.
    set_dt(&mut world, 2.0);
    run(&mut world, transition_tick_system);

    // Check all virtual ActiveTransitions are marked complete.
    {
        let mut q = world.query_filtered::<&ActiveTransition, With<Virtual>>();
        for at in q.iter(&world) {
            assert!(at.is_complete, "virtual should be complete after overshoot");
        }
    }

    // Group complete system should finalize.
    run(&mut world, group_transition_complete_system);
    world.flush();

    // All target entities should now be visible.
    for &t in &targets {
        let vis = world
            .get::<Visibility>(t)
            .expect("target should have Visibility");
        assert!(
            vis.visible,
            "target should be visible after group completion"
        );
    }

    // All virtual entities should be despawned.
    assert_eq!(
        count_with::<Virtual>(&mut world),
        0,
        "all virtual entities should be despawned after group completion"
    );
}

#[test]
fn slice_1_to_n_partial_complete_does_not_finalize() {
    // If only some virtuals complete, the group should NOT finalize yet.
    let mut world = make_world();
    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: TransitionConfig {
                duration: 1.0,
                delay: 0.0,
                easing: linear,
            },
            child_behavior: None,
            strategy: SplitStrategy::Slice,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Manually mark only the FIRST virtual as complete.
    {
        let mut q = world.query_filtered::<Entity, With<Virtual>>();
        let entities: Vec<Entity> = q.iter(&world).collect();
        if let Some(&first) = entities.first() {
            world
                .get_mut::<ActiveTransition>(first)
                .unwrap()
                .is_complete = true;
        }
    }

    run(&mut world, group_transition_complete_system);
    world.flush();

    // Targets should still be hidden — group not done.
    for &t in &targets {
        let vis = world
            .get::<Visibility>(t)
            .expect("target should have Visibility");
        assert!(
            !vis.visible,
            "target should remain hidden while transition is partial"
        );
    }
    assert_eq!(
        count_with::<Virtual>(&mut world),
        n,
        "virtuals should not be despawned yet"
    );
}

// ---------------------------------------------------------------------------
// NToOneRequest — Slice strategy
// ---------------------------------------------------------------------------

#[test]
fn n_to_one_hides_sources_and_dest() {
    let mut world = make_world();

    let n = 3;
    let source_states: Vec<QuadState> = (0..n)
        .map(|i| QuadState {
            position: Vec3::new(-200.0 + i as f32 * 200.0, 0.0, 0.5),
            size: Vec2::new(60.0, 60.0),
            color: Vec4::new(0.0, 0.5, 1.0, 1.0),
            ..Default::default()
        })
        .collect();

    let source_entities: Vec<Entity> = source_states
        .iter()
        .map(|s| world.spawn((s.clone(), Lifecycle::Idle)).id())
        .collect();

    let sources: Vec<GroupSource> = source_entities
        .iter()
        .zip(source_states.iter())
        .map(|(&e, s)| GroupSource {
            entity: e,
            state: s.clone(),
        })
        .collect();

    let dest = world
        .spawn((
            red(),
            Lifecycle::Idle,
            NToOneRequest {
                sources,
                default_config: default_cfg(),
                child_behavior: None,
            },
        ))
        .id();

    run(&mut world, n_to_one_setup_system);
    world.flush();

    assert!(
        !world.get::<Visibility>(dest).unwrap().visible,
        "destination should be hidden during N→1"
    );
    for &src in &source_entities {
        assert!(
            !world.get::<Visibility>(src).unwrap().visible,
            "source should be hidden during N→1"
        );
    }
}

#[test]
fn n_to_one_creates_n_virtual_entities() {
    let mut world = make_world();
    let n = 3;
    let sources: Vec<GroupSource> = (0..n)
        .map(|i| {
            let s = QuadState {
                position: Vec3::new(-100.0 + i as f32 * 100.0, 0.0, 0.5),
                size: Vec2::new(60.0, 60.0),
                ..Default::default()
            };
            let e = world.spawn((s.clone(), Lifecycle::Idle)).id();
            GroupSource {
                entity: e,
                state: s,
            }
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        NToOneRequest {
            sources,
            default_config: default_cfg(),
            child_behavior: None,
        },
    ));

    run(&mut world, n_to_one_setup_system);
    world.flush();

    assert_eq!(count_with::<Virtual>(&mut world), n);
}

#[test]
fn n_to_one_complete_reveals_dest() {
    let mut world = make_world();
    let n = 3;
    let sources: Vec<GroupSource> = (0..n)
        .map(|i| {
            let s = QuadState {
                position: Vec3::new(-100.0 + i as f32 * 100.0, 0.0, 0.5),
                size: Vec2::new(60.0, 60.0),
                ..Default::default()
            };
            let e = world.spawn((s.clone(), Lifecycle::Idle)).id();
            GroupSource {
                entity: e,
                state: s,
            }
        })
        .collect();

    let dest = world
        .spawn((
            red(),
            Lifecycle::Idle,
            NToOneRequest {
                sources,
                default_config: default_cfg(),
                child_behavior: None,
            },
        ))
        .id();

    run(&mut world, n_to_one_setup_system);
    world.flush();

    // Overshoot — all virtuals complete.
    set_dt(&mut world, 2.0);
    run(&mut world, transition_tick_system);
    run(&mut world, group_transition_complete_system);
    world.flush();

    assert!(
        world.get::<Visibility>(dest).unwrap().visible,
        "destination should be visible after N→1 completes"
    );
    assert_eq!(count_with::<Virtual>(&mut world), 0);
}

// ---------------------------------------------------------------------------
// Round trip: button → list (1→N slice) → button (N→1 slice)
// ---------------------------------------------------------------------------

#[test]
fn round_trip_button_list_button() {
    let mut world = make_world();

    // ---- Scene setup ----
    // Button: single quad (starts visible).
    let button_state = QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(200.0, 60.0),
        color: Vec4::new(0.2, 0.5, 1.0, 1.0),
        ..Default::default()
    };
    let button = world
        .spawn((button_state.clone(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    // List items: 3 quads (start hidden — will be shown after 1→N transition).
    let list_states: Vec<QuadState> = (0..3)
        .map(|i| QuadState {
            position: Vec3::new(-130.0 + i as f32 * 130.0, 120.0, 0.5),
            size: Vec2::new(100.0, 80.0),
            color: Vec4::new(0.3, 0.8, 0.3, 1.0),
            ..Default::default()
        })
        .collect();
    let list_entities: Vec<Entity> = list_states
        .iter()
        .map(|s| {
            world
                .spawn((s.clone(), Lifecycle::Idle, Visibility::HIDDEN))
                .id()
        })
        .collect();

    // ----  Phase 1: button (1→N slice) → list items ----
    let list_targets: Vec<GroupTarget> = list_entities
        .iter()
        .zip(list_states.iter())
        .map(|(&e, s)| GroupTarget {
            entity: e,
            state: s.clone(),
        })
        .collect();

    world.entity_mut(button).insert(OneToNRequest {
        targets: list_targets,
        default_config: TransitionConfig {
            duration: 0.3,
            delay: 0.0,
            easing: linear,
        },
        child_behavior: None,
        strategy: SplitStrategy::Slice,
    });

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Phase 1 assertions — setup complete.
    assert!(
        !world.get::<Visibility>(button).unwrap().visible,
        "button hidden"
    );
    assert_eq!(
        count_with::<Virtual>(&mut world),
        3,
        "3 virtual entities created"
    );
    assert!(
        world.get::<ActiveGroupTransition>(button).is_some(),
        "button is coordinator"
    );

    // Overshoot the transition.
    set_dt(&mut world, 1.0);
    run(&mut world, transition_tick_system);
    run(&mut world, group_transition_complete_system);
    world.flush();

    // Phase 1 completion assertions.
    assert_eq!(count_with::<Virtual>(&mut world), 0, "virtuals despawned");
    for &li in &list_entities {
        assert!(
            world.get::<Visibility>(li).unwrap().visible,
            "list item should be visible"
        );
    }
    assert!(
        world.get::<ActiveGroupTransition>(button).is_none(),
        "coordinator state removed"
    );
    assert_eq!(*world.get::<Lifecycle>(button).unwrap(), Lifecycle::Idle);

    // Total entity count: button + 3 list items = 4. No leaks.
    // (Virtuals were created and destroyed during Phase 1.)
    let all_entities: Vec<Entity> = {
        let mut q = world.query::<Entity>();
        q.iter(&world).collect()
    };
    assert_eq!(all_entities.len(), 4, "no entity leaks after Phase 1");

    // ---- Phase 2: list items (N→1 slice) → button ----
    let sources: Vec<GroupSource> = list_entities
        .iter()
        .zip(list_states.iter())
        .map(|(&e, _s)| GroupSource {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(), // snapshot current state
        })
        .collect();

    world.entity_mut(button).insert(NToOneRequest {
        sources,
        default_config: TransitionConfig {
            duration: 0.3,
            delay: 0.0,
            easing: linear,
        },
        child_behavior: None,
    });

    run(&mut world, n_to_one_setup_system);
    world.flush();

    // Phase 2 setup assertions.
    assert!(
        !world.get::<Visibility>(button).unwrap().visible,
        "button hidden during N→1"
    );
    for &li in &list_entities {
        assert!(
            !world.get::<Visibility>(li).unwrap().visible,
            "list item hidden during N→1"
        );
    }
    assert_eq!(
        count_with::<Virtual>(&mut world),
        3,
        "3 virtuals created for N→1"
    );

    // Overshoot the transition.
    set_dt(&mut world, 1.0);
    run(&mut world, transition_tick_system);
    run(&mut world, group_transition_complete_system);
    world.flush();

    // Phase 2 completion assertions.
    assert_eq!(count_with::<Virtual>(&mut world), 0, "virtuals despawned");
    assert!(
        world.get::<Visibility>(button).unwrap().visible,
        "button should be visible after N→1"
    );
    for &li in &list_entities {
        assert!(
            !world.get::<Visibility>(li).unwrap().visible,
            "list items should remain hidden"
        );
    }
    assert_eq!(*world.get::<Lifecycle>(button).unwrap(), Lifecycle::Idle);

    // Entity count still 4. No leaks from Phase 2 either.
    let all_entities: Vec<Entity> = {
        let mut q = world.query::<Entity>();
        q.iter(&world).collect()
    };
    assert_eq!(all_entities.len(), 4, "no entity leaks after Phase 2");
}

// ---------------------------------------------------------------------------
// ChildBehaviorFn — slice strategy per-child config
// ---------------------------------------------------------------------------

#[test]
fn slice_child_behavior_sets_per_virtual_duration() {
    fn per_child(idx: usize, _total: usize) -> TransitionConfig {
        TransitionConfig {
            duration: 0.1 * (idx + 1) as f32, // 0.1, 0.2, 0.3
            delay: 0.0,
            easing: linear,
        }
    }

    let mut world = make_world();
    let n = 3;
    let targets = spawn_targets(&mut world, n);
    let group_targets: Vec<GroupTarget> = targets
        .iter()
        .map(|&e| GroupTarget {
            entity: e,
            state: world.get::<QuadState>(e).unwrap().clone(),
        })
        .collect();

    world.spawn((
        red(),
        Lifecycle::Idle,
        OneToNRequest {
            targets: group_targets,
            default_config: default_cfg(),
            child_behavior: Some(per_child),
            strategy: SplitStrategy::Slice,
        },
    ));

    run(&mut world, one_to_n_setup_system);
    world.flush();

    // Collect durations from all virtual ActiveTransitions (order may vary).
    let mut durations: Vec<f32> = {
        let mut q = world.query_filtered::<&ActiveTransition, With<Virtual>>();
        q.iter(&world).map(|at| at.config.duration).collect()
    };
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap());

    assert!((durations[0] - 0.1).abs() < 1e-5, "shortest duration = 0.1");
    assert!((durations[1] - 0.2).abs() < 1e-5, "mid duration = 0.2");
    assert!((durations[2] - 0.3).abs() < 1e-5, "longest duration = 0.3");
}
