//! Integration tests for the M12.1 signal system: `SignalRegistry`,
//! `signal::set`/`signal_dispatch_system`, `TransitionDropped` reporting, and
//! `CommandQueue`/`flush_commands_system`.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    component::{Lifecycle, TransitionRequest, Visibility},
    flush_commands_system,
    schedule::CommandQueue,
    signal::{
        create_signal, destroy_signal, register_signal_hooks, set, signal_dispatch_system,
        DropReason, DroppedSignals, PendingSignalSets, SignalRegistry,
    },
    transition::TransitionConfig,
    QuadState,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<SignalRegistry>();
    world.init_resource::<PendingSignalSets>();
    world.init_resource::<DroppedSignals>();
    world.init_resource::<CommandQueue>();
    // Must run before any OwnedSignals component can exist in an archetype —
    // same requirement as ProteusWorld::new()'s own call.
    register_signal_hooks(&mut world);
    world
}

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

fn cfg() -> TransitionConfig {
    TransitionConfig {
        duration: 0.3,
        delay: 0.0,
        easing: proteus_ui::linear,
    }
}

fn run<M>(world: &mut World, system: impl IntoSystem<(), (), M> + 'static) {
    let mut sched = Schedule::default();
    sched.add_systems(system);
    sched.run(world);
}

// ---------------------------------------------------------------------------
// SignalRegistry / create_signal / destroy_signal
// ---------------------------------------------------------------------------

#[test]
fn create_signal_registers_it() {
    let mut world = make_world();
    let id = create_signal(&mut world, None);
    assert!(world.resource::<SignalRegistry>().exists(id));
    assert_eq!(world.resource::<SignalRegistry>().owner(id), None);
}

#[test]
fn destroy_signal_removes_it() {
    let mut world = make_world();
    let id = create_signal(&mut world, None);
    destroy_signal(&mut world, id);
    assert!(!world.resource::<SignalRegistry>().exists(id));
}

#[test]
fn owned_signal_records_owner() {
    let mut world = make_world();
    let owner = world.spawn_empty().id();
    let id = create_signal(&mut world, Some(owner));
    assert_eq!(world.resource::<SignalRegistry>().owner(id), Some(owner));
}

#[test]
fn owned_signal_destroyed_when_owner_despawned() {
    let mut world = make_world();
    let owner = world.spawn_empty().id();
    let id = create_signal(&mut world, Some(owner));
    assert!(world.resource::<SignalRegistry>().exists(id));

    world.despawn(owner);

    assert!(
        !world.resource::<SignalRegistry>().exists(id),
        "despawning the owner must destroy its owned signal"
    );
}

#[test]
fn owned_signal_destroys_only_its_own_signals() {
    let mut world = make_world();
    let owner_a = world.spawn_empty().id();
    let owner_b = world.spawn_empty().id();
    let id_a = create_signal(&mut world, Some(owner_a));
    let id_b = create_signal(&mut world, Some(owner_b));

    world.despawn(owner_a);

    assert!(!world.resource::<SignalRegistry>().exists(id_a));
    assert!(
        world.resource::<SignalRegistry>().exists(id_b),
        "despawning owner_a must not touch owner_b's signal"
    );
}

// ---------------------------------------------------------------------------
// signal::set + signal_dispatch_system — happy path
// ---------------------------------------------------------------------------

#[test]
fn fresh_dispatch_inserts_transition_request_from_source_state() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    world.flush();

    let req = world
        .get::<TransitionRequest>(to)
        .expect("to entity should have a TransitionRequest inserted");
    assert_eq!(req.to.color, blue().color);
    let from_state = req
        .from_state
        .as_ref()
        .expect("fresh dispatch must set an explicit from_state");
    assert_eq!(from_state.color, red().color);

    assert!(
        world.resource::<DroppedSignals>().entries.is_empty(),
        "a valid request must not be dropped"
    );
}

#[test]
fn dispatch_hides_the_from_entity() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    world.flush();

    assert!(
        !world.get::<Visibility>(from).unwrap().visible,
        "from entity must go invisible once the morph starts — it is the exit"
    );
}

// ---------------------------------------------------------------------------
// signal::set + signal_dispatch_system — drop cases
// ---------------------------------------------------------------------------

#[test]
fn dispatch_drops_when_signal_not_found() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    destroy_signal(&mut world, signal); // now stale
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedSignals>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::SignalNotFound);
}

#[test]
fn dispatch_drops_when_to_entity_not_found() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let stale_to = world.spawn_empty().id();
    world.despawn(stale_to);
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, stale_to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);

    let drops = &world.resource::<DroppedSignals>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotFound);
}

#[test]
fn dispatch_drops_when_from_entity_not_found() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let stale_from = world.spawn_empty().id();
    world.despawn(stale_from);

    set(&mut world, signal, to, stale_from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);

    let drops = &world.resource::<DroppedSignals>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotFound);
}

#[test]
fn dispatch_drops_when_from_not_visible() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::HIDDEN))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedSignals>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotVisible);
}

#[test]
fn dispatch_drops_when_already_transitioning_and_not_interruptible() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedSignals>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::AlreadyTransitioning);
}

#[test]
fn dispatch_interrupts_when_already_transitioning_and_interruptible() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, signal, to, from, blue(), cfg(), true);
    run(&mut world, signal_dispatch_system);
    world.flush();

    let req = world
        .get::<TransitionRequest>(to)
        .expect("interruptible retarget must still insert a TransitionRequest");
    assert!(
        req.from_state.is_none(),
        "retarget must leave from_state=None so transition_setup_system snapshots \
         to's own current mid-flight QuadState, not from's"
    );
    assert!(world.resource::<DroppedSignals>().entries.is_empty());
}

#[test]
fn dropped_signals_clears_previous_frame_results() {
    let mut world = make_world();
    let signal = create_signal(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    // Frame 1: dropped (already transitioning, not interruptible).
    set(&mut world, signal, to, from, blue(), cfg(), false);
    run(&mut world, signal_dispatch_system);
    assert_eq!(world.resource::<DroppedSignals>().entries.len(), 1);

    // Frame 2: nothing queued — stale drop from frame 1 must not linger.
    run(&mut world, signal_dispatch_system);
    assert!(world.resource::<DroppedSignals>().entries.is_empty());
}

// ---------------------------------------------------------------------------
// CommandQueue / flush_commands_system
// ---------------------------------------------------------------------------

#[test]
fn command_queue_applies_pushed_mutation_on_flush() {
    let mut world = make_world();
    let entity = world.spawn((red(), Lifecycle::Idle)).id();

    world.resource_mut::<CommandQueue>().push(move |world| {
        world.get_mut::<QuadState>(entity).unwrap().corner_radius = 42.0;
    });

    // Not applied yet — push() only enqueues.
    assert_eq!(world.get::<QuadState>(entity).unwrap().corner_radius, 0.0);

    run(&mut world, flush_commands_system);

    assert_eq!(world.get::<QuadState>(entity).unwrap().corner_radius, 42.0);
}

#[test]
fn command_queue_applies_in_fifo_order() {
    let mut world = make_world();
    let entity = world.spawn((red(), Lifecycle::Idle)).id();

    world.resource_mut::<CommandQueue>().push(move |world| {
        world.get_mut::<QuadState>(entity).unwrap().corner_radius = 1.0;
    });
    world.resource_mut::<CommandQueue>().push(move |world| {
        let current = world.get::<QuadState>(entity).unwrap().corner_radius;
        world.get_mut::<QuadState>(entity).unwrap().corner_radius = current + 1.0;
    });

    run(&mut world, flush_commands_system);

    assert_eq!(
        world.get::<QuadState>(entity).unwrap().corner_radius,
        2.0,
        "second closure must observe the first's mutation — FIFO order"
    );
}

#[test]
fn command_queue_is_empty_after_flush() {
    let mut world = make_world();
    world.insert_resource(CallCount(0));
    world.resource_mut::<CommandQueue>().push(|world| {
        world.resource_mut::<CallCount>().0 += 1;
    });

    run(&mut world, flush_commands_system);
    run(&mut world, flush_commands_system); // second flush: queue should be empty

    assert_eq!(
        world.resource::<CallCount>().0,
        1,
        "a flushed command must not run again on the next flush"
    );
}

#[derive(Resource, Default)]
struct CallCount(u32);
