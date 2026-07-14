//! ECS world and system schedule for Proteus.
//!
//! [`ProteusWorld`] wraps a `bevy_ecs` `World` + `Schedule` and wires up the
//! full system order from Phase B of PLANNING.md:
//!
//! ```text
//! flush_commands       drain deferred mutations from last tick
//! input                process pointer / keyboard events  [stub M2]
//! navigation           directional focus movement         [stub M2]
//! transition_setup     TransitionRequest → ActiveTransition
//! transition_tick      advance t, lerp QuadState
//! transition_complete  t=1.0 → fire event, restore Idle
//! visibility           cascade Visibility changes         [stub M2]
//! opacity              cascade effective opacity          [stub M2]
//! bake                 offscreen texture composites       [stub M2]
//! render               build instance buffer, draw        [stub M2]
//! ```
//!
//! The schedule is fixed and linear — each stage must complete before the next
//! begins. This makes reasoning about per-frame state straightforward.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ApplyDeferred;

use crate::input::{hit_test_system, HoveredEntity, InteractionEvents, PointerInput};
use crate::topology::{
    group_transition_complete_system, n_to_one_setup_system, one_to_n_setup_system,
};
use crate::transition::{
    transition_complete_system, transition_setup_system, transition_tick_system,
    CompletedTransitions, FrameTime,
};

// ---------------------------------------------------------------------------
// System sets — define the canonical stage order
// ---------------------------------------------------------------------------

/// Labels for the sequential stages in the Proteus frame loop.
///
/// Systems added without an explicit set run last. Add all real and stub
/// systems to one of these sets to keep ordering deterministic.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProteusSet {
    /// Apply deferred `Commands` queued during the previous frame.
    FlushCommands,
    /// Process pointer and keyboard input events.
    Input,
    /// Handle directional and tab navigation.
    Navigation,
    /// Convert `TransitionRequest` components into `ActiveTransition`.
    TransitionSetup,
    /// Advance `t`, lerp `QuadState`.
    TransitionTick,
    /// Detect `t = 1.0`, fire `TransitionComplete`, clean up.
    TransitionComplete,
    /// Finalize group transitions when all virtual entities complete.
    GroupTransitionComplete,
    /// Cascade `Visibility` changes down the hierarchy.
    Visibility,
    /// Compute effective opacity down the hierarchy.
    Opacity,
    /// Offscreen texture bake composites.
    Bake,
    /// Build the GPU instance buffer and submit the draw call.
    Render,
}

// ---------------------------------------------------------------------------
// Stub systems for unimplemented stages
// ---------------------------------------------------------------------------
// These do nothing but hold the stage slot so the ordering constraints are
// in place before the real implementations land in later milestones.

fn stub_navigation_system() {}
fn stub_visibility_system() {}
fn stub_opacity_system() {}
fn stub_bake_system() {}
fn stub_render_system() {}

// ---------------------------------------------------------------------------
// ProteusWorld
// ---------------------------------------------------------------------------

/// The top-level ECS runtime. One instance per Proteus application.
///
/// The shell (native or WASM) holds a `ProteusWorld` and calls `update(dt)`
/// once per frame with the elapsed wall-clock seconds.
pub struct ProteusWorld {
    pub world: World,
    pub schedule: Schedule,
}

impl ProteusWorld {
    /// Create and initialize the world with all resources and the full schedule.
    pub fn new() -> Self {
        let mut world = World::new();

        // --- Resources ---
        world.init_resource::<FrameTime>();
        world.init_resource::<CompletedTransitions>();
        world.init_resource::<PointerInput>();
        world.init_resource::<InteractionEvents>();
        world.init_resource::<HoveredEntity>();

        // --- Schedule ---
        let schedule = build_schedule();

        Self { world, schedule }
    }

    /// Advance one frame by `delta_secs` wall-clock seconds.
    ///
    /// Call this from the render loop after acquiring the swap-chain frame and
    /// before encoding the GPU commands.
    pub fn update(&mut self, delta_secs: f32) {
        // Inject the frame delta before running systems.
        self.world.resource_mut::<FrameTime>().delta_secs = delta_secs;
        self.schedule.run(&mut self.world);
    }
}

impl Default for ProteusWorld {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Schedule construction — separated so tests can call it directly
// ---------------------------------------------------------------------------

/// Build the Proteus system schedule with correct stage ordering.
///
/// Exported so integration tests can construct a minimal world without
/// going through `ProteusWorld::new()`.
pub fn build_schedule() -> Schedule {
    let mut schedule = Schedule::default();

    // Chain all sets in the canonical order — each set runs to completion
    // before the next begins.
    schedule.configure_sets(
        (
            ProteusSet::FlushCommands,
            ProteusSet::Input,
            ProteusSet::Navigation,
            ProteusSet::TransitionSetup,
            ProteusSet::TransitionTick,
            ProteusSet::TransitionComplete,
            ProteusSet::GroupTransitionComplete,
            ProteusSet::Visibility,
            ProteusSet::Opacity,
            ProteusSet::Bake,
            ProteusSet::Render,
        )
            .chain(),
    );

    // Drain bevy_ecs deferred commands that accumulated during the last frame.
    schedule.add_systems(ApplyDeferred.in_set(ProteusSet::FlushCommands));

    // M7: real hit-test system replaces the input stub.
    schedule.add_systems(hit_test_system.in_set(ProteusSet::Input));
    // Stub systems — hold their slot until real implementations land.
    schedule.add_systems(stub_navigation_system.in_set(ProteusSet::Navigation));
    schedule.add_systems(stub_visibility_system.in_set(ProteusSet::Visibility));
    schedule.add_systems(stub_opacity_system.in_set(ProteusSet::Opacity));
    schedule.add_systems(stub_bake_system.in_set(ProteusSet::Bake));
    schedule.add_systems(stub_render_system.in_set(ProteusSet::Render));

    // Real transition systems — the heart of M2.
    schedule.add_systems(transition_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule.add_systems(transition_tick_system.in_set(ProteusSet::TransitionTick));
    schedule.add_systems(transition_complete_system.in_set(ProteusSet::TransitionComplete));

    // Group topology systems — M3.
    // Setup systems run in the same TransitionSetup slot; ordering within the
    // set is undefined but both are independent of each other.
    schedule.add_systems(one_to_n_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule.add_systems(n_to_one_setup_system.in_set(ProteusSet::TransitionSetup));
    schedule
        .add_systems(group_transition_complete_system.in_set(ProteusSet::GroupTransitionComplete));

    schedule
}
