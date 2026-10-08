// Tests of channels: `ChannelRegistry`, `channel::set` and
// `channel_dispatch_system`, dropped requests, and
// `CommandQueue`/`flush_commands_system`.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_ui::{
    channel::{
        channel_dispatch_system, create_channel, destroy_channel, register_channel_hooks, set,
        ChannelRegistry, DropReason, DroppedRequests, PendingChannelSets,
    },
    component::{Lifecycle, TransitionRequest, Visibility},
    flush_commands_system,
    schedule::CommandQueue,
    transition::TransitionConfig,
    QuadState,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_world() -> World {
    let mut world = World::new();
    world.init_resource::<ChannelRegistry>();
    world.init_resource::<PendingChannelSets>();
    world.init_resource::<DroppedRequests>();
    world.init_resource::<CommandQueue>();
    // Must run before any OwnedChannels component can exist in an archetype —
    // same requirement as ProteusWorld::new()'s own call.
    register_channel_hooks(&mut world);
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
        easing: proteus_ui::Easing::Linear,
    }
}

fn run<M>(world: &mut World, system: impl IntoSystem<(), (), M> + 'static) {
    let mut sched = Schedule::default();
    sched.add_systems(system);
    sched.run(world);
}

// ---------------------------------------------------------------------------
// ChannelRegistry / create_channel / destroy_channel
// ---------------------------------------------------------------------------

#[test]
fn create_channel_registers_it() {
    let mut world = make_world();
    let id = create_channel(&mut world, None);
    assert!(world.resource::<ChannelRegistry>().exists(id));
    assert_eq!(world.resource::<ChannelRegistry>().owner(id), None);
}

#[test]
fn destroy_channel_removes_it() {
    let mut world = make_world();
    let id = create_channel(&mut world, None);
    destroy_channel(&mut world, id);
    assert!(!world.resource::<ChannelRegistry>().exists(id));
}

#[test]
fn owned_channel_records_owner() {
    let mut world = make_world();
    let owner = world.spawn_empty().id();
    let id = create_channel(&mut world, Some(owner));
    assert_eq!(world.resource::<ChannelRegistry>().owner(id), Some(owner));
}

#[test]
fn owned_channel_destroyed_when_owner_despawned() {
    let mut world = make_world();
    let owner = world.spawn_empty().id();
    let id = create_channel(&mut world, Some(owner));
    assert!(world.resource::<ChannelRegistry>().exists(id));

    world.despawn(owner);

    assert!(
        !world.resource::<ChannelRegistry>().exists(id),
        "despawning the owner must destroy its owned channel"
    );
}

#[test]
fn owned_channel_destroys_only_its_own_channels() {
    let mut world = make_world();
    let owner_a = world.spawn_empty().id();
    let owner_b = world.spawn_empty().id();
    let id_a = create_channel(&mut world, Some(owner_a));
    let id_b = create_channel(&mut world, Some(owner_b));

    world.despawn(owner_a);

    assert!(!world.resource::<ChannelRegistry>().exists(id_a));
    assert!(
        world.resource::<ChannelRegistry>().exists(id_b),
        "despawning owner_a must not touch owner_b's channel"
    );
}

// ---------------------------------------------------------------------------
// channel::set + channel_dispatch_system — happy path
// ---------------------------------------------------------------------------

#[test]
fn fresh_dispatch_inserts_transition_request_from_source_state() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
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
        world.resource::<DroppedRequests>().entries.is_empty(),
        "a valid request must not be dropped"
    );
}

#[test]
fn dispatch_hides_the_from_entity() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    world.flush();

    assert!(
        !world.get::<Visibility>(from).unwrap().visible,
        "from entity must go invisible once the morph starts — it is the exit"
    );
}

// A component transitioning into itself stays visible: the request is an
// `animate_to` to its target, not a hide.
#[test]
fn dispatch_from_an_entity_into_itself_leaves_it_visible() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let entity = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, entity, entity, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    world.flush();

    assert!(world.get::<Visibility>(entity).unwrap().visible);
    assert!(world.get::<TransitionRequest>(entity).is_some());
}

// ---------------------------------------------------------------------------
// channel::set + channel_dispatch_system — drop cases
// ---------------------------------------------------------------------------

#[test]
fn dispatch_drops_when_channel_not_found() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    destroy_channel(&mut world, channel); // now stale
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedRequests>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::ChannelNotFound);
}

#[test]
fn dispatch_drops_when_to_entity_not_found() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let stale_to = world.spawn_empty().id();
    world.despawn(stale_to);
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, stale_to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);

    let drops = &world.resource::<DroppedRequests>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotFound);
}

#[test]
fn dispatch_drops_when_from_entity_not_found() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let stale_from = world.spawn_empty().id();
    world.despawn(stale_from);

    set(&mut world, channel, to, stale_from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);

    let drops = &world.resource::<DroppedRequests>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotFound);
}

#[test]
fn dispatch_drops_when_from_not_visible() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::HIDDEN))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedRequests>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::EntityNotVisible);
}

#[test]
fn dispatch_drops_when_already_transitioning_and_not_interruptible() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    world.flush();

    assert!(world.get::<TransitionRequest>(to).is_none());
    let drops = &world.resource::<DroppedRequests>().entries;
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].reason, DropReason::AlreadyTransitioning);
}

#[test]
fn dispatch_interrupts_when_already_transitioning_and_interruptible() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    set(&mut world, channel, to, from, blue(), cfg(), true);
    run(&mut world, channel_dispatch_system);
    world.flush();

    let req = world
        .get::<TransitionRequest>(to)
        .expect("interruptible retarget must still insert a TransitionRequest");
    assert!(
        req.from_state.is_none(),
        "retarget must leave from_state=None so transition_setup_system snapshots \
         to's own current mid-flight QuadState, not from's"
    );
    assert!(world.resource::<DroppedRequests>().entries.is_empty());
}

#[test]
fn dropped_requests_clears_previous_frame_results() {
    let mut world = make_world();
    let channel = create_channel(&mut world, None);
    let to = world
        .spawn((blue(), Lifecycle::Transitioning, Visibility::VISIBLE))
        .id();
    let from = world
        .spawn((red(), Lifecycle::Idle, Visibility::VISIBLE))
        .id();

    // Frame 1: dropped (already transitioning, not interruptible).
    set(&mut world, channel, to, from, blue(), cfg(), false);
    run(&mut world, channel_dispatch_system);
    assert_eq!(world.resource::<DroppedRequests>().entries.len(), 1);

    // Frame 2: nothing queued — stale drop from frame 1 must not linger.
    run(&mut world, channel_dispatch_system);
    assert!(world.resource::<DroppedRequests>().entries.is_empty());
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
