//! The ECS world and the order its systems run in each tick.
//!
//! [`ProteusWorld`] holds a `bevy_ecs` `World` and `Schedule`. Each tick runs
//! these stages, in order, each finishing before the next starts:
//!
//! ```text
//! flush_commands       apply mutations queued during the last tick
//! input                hit-test the pointer and record input events
//! interaction_style    start style transitions for hover, press, focus and disabled
//! navigation           keyboard focus movement (placeholder, does nothing yet)
//! channel_dispatch      turn channel requests into transition requests
//! transition_setup     start requested transitions, including splits and merges
//! transition_tick      advance transitions and interpolate geometry
//! transition_complete  finish transitions that have reached the end
//! group_complete       finish splits and merges whose pieces have all arrived
//! visibility           work out each entity's visibility from its ancestors'
//! opacity              work out each entity's opacity from its ancestors'
//! cascade_flush        apply those results before baking reads them
//! bake                 bake components marked for baking
//! bake_flush           apply the bake results
//! render               placeholder; drawing is done outside the schedule
//! ```

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ApplyDeferred;

use crate::bake::bake_system;
use crate::channel::{self, channel_dispatch_system, register_channel_hooks};
use crate::hierarchy::{opacity_system, visibility_system};
use crate::input::{
    hit_test_system, FocusState, HoveredEntity, InteractionEvents, PointerInput, PressedEntity,
};
use crate::interaction::interaction_style_system;
use crate::spawn_order::register_spawn_order_hooks;
use crate::texture_ref::{register_texture_ref_hooks, touch_texture_refs_system};
use crate::topology::{
    group_transition_complete_system, n_to_one_setup_system, one_to_n_setup_system,
    register_transition_alloc_hooks, TransitionAtlasSize,
};
use crate::transition::{
    transition_complete_system, transition_setup_system, transition_tick_system,
    CompletedTransitions, FrameTime,
};

// ---------------------------------------------------------------------------
// System sets — define the canonical stage order
// ---------------------------------------------------------------------------

/// The stages of a tick, in the order they run. See the module docs.
///
/// A system added without one of these sets runs last, so every system is
/// added to one, which keeps the order fixed.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProteusSet {
    /// Apply deferred `Commands` queued during the previous frame.
    FlushCommands,
    /// Hit-test the pointer and record this tick's input events.
    Input,
    /// Work out which interaction style applies to each component, and start
    /// a short transition to it.
    InteractionStyle,
    /// Keyboard focus movement. A placeholder that does nothing yet.
    Navigation,
    /// Turn pending `channel::set` calls into `TransitionRequest` components.
    ChannelDispatch,
    /// Convert `TransitionRequest` components into `ActiveTransition`.
    TransitionSetup,
    /// Advance `t`, lerp `QuadState`.
    TransitionTick,
    /// Finish transitions that have reached `t = 1.0` and record them in
    /// `CompletedTransitions`.
    TransitionComplete,
    /// Finalize group transitions when all virtual entities complete.
    GroupTransitionComplete,
    /// Cascade `Visibility` changes down the hierarchy.
    Visibility,
    /// Compute effective opacity down the hierarchy.
    Opacity,
    /// Apply the visibility and opacity results, so baking sees this tick's
    /// values rather than last tick's.
    CascadeFlush,
    /// Bake each component marked `Baked`, with its children, into one
    /// texture.
    Bake,
    /// Apply the bake results, so they are drawn this tick.
    BakeFlush,
    /// A placeholder. Drawing happens after the tick, outside the schedule.
    Render,
}

// ---------------------------------------------------------------------------
// CommandQueue: mutations deferred to the start of the next tick
// ---------------------------------------------------------------------------

/// A deferred mutation.
///
/// `Sync` because a `bevy_ecs` resource must be, even though the queue is only
/// used from one thread. A closure that captures only plain data, such as an
/// `Entity`, is `Sync`.
type BoxedCommand = Box<dyn FnOnce(&mut World) + Send + Sync>;

/// Mutations to apply to the world at the start of the next tick.
///
/// Code that runs during a system can't change the world directly. It pushes a
/// closure here instead, and [`flush_commands_system`] applies every queued
/// closure first thing next tick, before any other system runs.
#[derive(Resource, Default)]
pub struct CommandQueue {
    pending: Vec<BoxedCommand>,
}

impl CommandQueue {
    /// Queues a mutation to apply at the start of the next tick.
    pub fn push(&mut self, cmd: impl FnOnce(&mut World) + Send + Sync + 'static) {
        self.pending.push(Box::new(cmd));
    }
}

/// Applies every mutation in [`CommandQueue`], in the order they were queued.
///
/// Runs before the `ApplyDeferred` in [`ProteusSet::FlushCommands`], so that a
/// queued closure that uses `Commands` is also applied before input runs.
pub fn flush_commands_system(world: &mut World) {
    let pending = std::mem::take(&mut world.resource_mut::<CommandQueue>().pending);
    for cmd in pending {
        cmd(world);
    }
}

// ---------------------------------------------------------------------------
// Stub systems for unimplemented stages
// ---------------------------------------------------------------------------
// Placeholders that hold their stage's place in the order.

fn stub_navigation_system() {}
fn stub_render_system() {}

// ---------------------------------------------------------------------------
// ProteusWorld
// ---------------------------------------------------------------------------

/// The ECS world and its schedule: everything a Proteus app's state is made
/// of. [`ProteusWorld::update`] runs one tick.
pub struct ProteusWorld {
    /// The ECS world.
    pub world: World,
    /// The systems run each tick.
    pub schedule: Schedule,
    // Only the visibility and opacity systems; see `refresh_cascades`.
    cascade_schedule: Schedule,
}

impl ProteusWorld {
    /// Creates the world with every resource and hook it needs, and the
    /// schedule.
    pub fn new() -> Self {
        let mut world = World::new();

        // --- Resources ---
        world.init_resource::<FrameTime>();
        world.init_resource::<CompletedTransitions>();
        world.init_resource::<PointerInput>();
        world.init_resource::<InteractionEvents>();
        world.init_resource::<HoveredEntity>();
        world.init_resource::<PressedEntity>();
        world.init_resource::<FocusState>();
        world.init_resource::<CommandQueue>();
        // A default, replaced with the configured size when a renderer is
        // created.
        world.init_resource::<TransitionAtlasSize>();
        channel::init_resources(&mut world);

        // Component hooks must be registered before any entity has the
        // component, or bevy_ecs panics, so register them all here:
        // texture reference counting, removing a component's owned
        // channels when it is destroyed, stamping `SpawnOrder`, and returning
        // transition-atlas space when its owner goes away.
        register_texture_ref_hooks(&mut world);
        register_channel_hooks(&mut world);
        register_spawn_order_hooks(&mut world);
        register_transition_alloc_hooks(&mut world);

        // --- Schedule ---
        let schedule = build_schedule();
        let cascade_schedule = build_cascade_schedule();

        Self {
            world,
            schedule,
            cascade_schedule,
        }
    }

    /// Runs one tick, advancing time by `delta_secs` seconds.
    pub fn update(&mut self, delta_secs: f32) {
        // Inject the frame delta before running systems.
        self.world.resource_mut::<FrameTime>().delta_secs = delta_secs;
        self.schedule.run(&mut self.world);
    }

    /// Recomputes effective visibility and opacity without running a full
    /// tick.
    ///
    /// [`ProteusWorld::update`] computes them before app code runs, so a change
    /// the app makes afterwards would be drawn one frame late. Call this after
    /// such changes and before `collect_instances`.
    pub fn refresh_cascades(&mut self) {
        self.cascade_schedule.run(&mut self.world);
    }
}

impl Default for ProteusWorld {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Schedule construction
// ---------------------------------------------------------------------------

/// Builds the schedule, with every system in its stage. Public so that tests
/// can build a world without [`ProteusWorld::new`].
pub fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();

    // Each set runs to completion before the next begins.
    schedule.configure_sets(
        (
            ProteusSet::FlushCommands,
            ProteusSet::Input,
            ProteusSet::InteractionStyle,
            ProteusSet::Navigation,
            ProteusSet::ChannelDispatch,
            ProteusSet::TransitionSetup,
            ProteusSet::TransitionTick,
            ProteusSet::TransitionComplete,
            ProteusSet::GroupTransitionComplete,
            ProteusSet::Visibility,
            ProteusSet::Opacity,
            ProteusSet::CascadeFlush,
            ProteusSet::Bake,
            ProteusSet::BakeFlush,
            ProteusSet::Render,
        )
            .chain(),
    );

    // Apply the CommandQueue first, then bevy_ecs's own deferred Commands,
    // which include any a queued closure issued.
    schedule.add_systems(
        (flush_commands_system, ApplyDeferred)
            .chain()
            .in_set(ProteusSet::FlushCommands),
    );

    schedule.add_systems(hit_test_system.in_set(ProteusSet::Input));
    schedule.add_systems(interaction_style_system.in_set(ProteusSet::InteractionStyle));
    schedule.add_systems(stub_navigation_system.in_set(ProteusSet::Navigation));
    schedule.add_systems(channel_dispatch_system.in_set(ProteusSet::ChannelDispatch));
    schedule.add_systems(stub_render_system.in_set(ProteusSet::Render));

    // bake_system writes through Commands, so BakeFlush applies them before
    // anything reads the result.
    schedule.add_systems(bake_system.in_set(ProteusSet::Bake));
    // Marks every texture in use as recently used, for eviction. Independent
    // of bake_system: eviction only considers unreferenced textures, and this
    // only touches referenced ones.
    schedule.add_systems(touch_texture_refs_system.in_set(ProteusSet::Bake));
    schedule.add_systems(ApplyDeferred.in_set(ProteusSet::BakeFlush));

    // Both write through Commands, so CascadeFlush applies them before baking
    // reads the results.
    schedule.add_systems(visibility_system.in_set(ProteusSet::Visibility));
    schedule.add_systems(opacity_system.in_set(ProteusSet::Opacity));
    schedule.add_systems(ApplyDeferred.in_set(ProteusSet::CascadeFlush));

    // Transitions.
    schedule.add_systems(transition_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule.add_systems(transition_tick_system.in_set(ProteusSet::TransitionTick));
    schedule.add_systems(transition_complete_system.in_set(ProteusSet::TransitionComplete));

    // Splits and merges. Their setup systems share TransitionSetup; their
    // order within it is undefined, and they don't depend on each other.
    schedule.add_systems(one_to_n_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule.add_systems(n_to_one_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule
        .add_systems(group_transition_complete_system.in_set(ProteusSet::GroupTransitionComplete));

    schedule
}

/// Builds the schedule [`ProteusWorld::refresh_cascades`] runs: only the
/// visibility and opacity systems, and applying their results.
fn build_cascade_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems((visibility_system, opacity_system, ApplyDeferred).chain());
    schedule
}
