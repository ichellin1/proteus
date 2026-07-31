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
//! visibility           cascade Visibility → EffectiveVisibility  [real since M10]
//! opacity              cascade Opacity → EffectiveOpacity        [real since M10]
//! cascade_flush        ApplyDeferred so Bake/Render see this frame's cascades [M10]
//! bake                 static composite baking (M10.5) / offscreen texture composites
//! bake_flush           ApplyDeferred so Render sees this frame's bake [M10.5]
//! render               build instance buffer, draw        [stub M2]
//! ```
//!
//! The schedule is fixed and linear — each stage must complete before the next
//! begins. This makes reasoning about per-frame state straightforward.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ApplyDeferred;

use crate::bake::bake_system;
use crate::hierarchy::{opacity_system, visibility_system};
use crate::input::{hit_test_system, HoveredEntity, InteractionEvents, PointerInput};
use crate::texture_ref::{register_texture_ref_hooks, touch_texture_refs_system};
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
    /// Apply the deferred `Commands` `Visibility`/`Opacity` cascades queued
    /// this frame, so `Bake`/`Render` read fresh (not last-frame-stale)
    /// `EffectiveVisibility`/`EffectiveOpacity`.
    CascadeFlush,
    /// Static composite baking (M10.5) — collapses a `Baked` subtree into a
    /// single textured quad.
    Bake,
    /// Apply the deferred `Commands` `bake_system` queued this frame (insert
    /// `BakedComposite`, despawn baked children, neutralize the root's own
    /// visual-effect components), so `Render` sees this frame's bake instead
    /// of last frame's — same reasoning as `CascadeFlush`.
    BakeFlush,
    /// Build the GPU instance buffer and submit the draw call.
    Render,
}

// ---------------------------------------------------------------------------
// Stub systems for unimplemented stages
// ---------------------------------------------------------------------------
// These do nothing but hold the stage slot so the ordering constraints are
// in place before the real implementations land in later milestones.

fn stub_navigation_system() {}
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
    /// Just the Visibility/Opacity cascade (+ its deferred-command flush),
    /// see [`ProteusWorld::refresh_cascades`].
    cascade_schedule: Schedule,
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

        // M11: TextureRef's ref-counting hooks must be registered before any
        // TextureRef component can exist in an archetype — bevy_ecs panics
        // otherwise. Doing this here, at world construction, guarantees that.
        register_texture_ref_hooks(&mut world);

        // --- Schedule ---
        let schedule = build_schedule();
        let cascade_schedule = build_cascade_schedule();

        Self {
            world,
            schedule,
            cascade_schedule,
        }
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

    /// Re-runs just the Visibility → `EffectiveVisibility` and Opacity →
    /// `EffectiveOpacity` cascades (plus their deferred-command flush) — not
    /// the full per-frame `schedule` (no input/transition/bake re-run).
    ///
    /// `update()`'s own cascade pass reflects `Visibility`/`Opacity` as they
    /// stood *before* this frame's game logic ran. A shell's post-`update()`
    /// code (e.g. a state machine's `settle()` step) routinely mutates
    /// `Visibility` directly afterward — without a second cascade pass,
    /// `collect_instances` (which prefers the cascaded `EffectiveVisibility`
    /// over raw `Visibility`, see its module doc) would render that mutation
    /// one frame late, showing whatever was cascaded before it. Call this
    /// after all such per-frame mutations, immediately before
    /// `collect_instances`.
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
            ProteusSet::CascadeFlush,
            ProteusSet::Bake,
            ProteusSet::BakeFlush,
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
    schedule.add_systems(stub_render_system.in_set(ProteusSet::Render));

    // M10.5: real bake system replaces the stub. Writes via Commands
    // (insert BakedComposite, despawn baked children, neutralize the root's
    // own visual-effect components), so BakeFlush's ApplyDeferred must run
    // before Render reads the result — otherwise it'd see last frame's stale
    // (unbaked) state.
    schedule.add_systems(bake_system.in_set(ProteusSet::Bake));
    // M11: bumps every live TextureRef's LRU recency once per frame. No
    // ordering dependency with bake_system — eviction only ever considers
    // zero-ref entries, and touch only ever updates referenced ones, so the
    // two systems never contend over the same entry.
    schedule.add_systems(touch_texture_refs_system.in_set(ProteusSet::Bake));
    schedule.add_systems(ApplyDeferred.in_set(ProteusSet::BakeFlush));

    // M10: real cascade systems replace the visibility/opacity stubs. Both
    // write via `Commands` (deferred), so `CascadeFlush`'s `ApplyDeferred`
    // must run before `Bake`/`Render` read the result — otherwise they'd see
    // last frame's stale `EffectiveVisibility`/`EffectiveOpacity`.
    schedule.add_systems(visibility_system.in_set(ProteusSet::Visibility));
    schedule.add_systems(opacity_system.in_set(ProteusSet::Opacity));
    schedule.add_systems(ApplyDeferred.in_set(ProteusSet::CascadeFlush));

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

/// Build the standalone Visibility/Opacity cascade schedule used by
/// [`ProteusWorld::refresh_cascades`] — the same two systems `build_schedule`
/// runs in its `Visibility`/`Opacity` sets, plus their `ApplyDeferred` flush,
/// with no input/transition/bake stages around them.
fn build_cascade_schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.add_systems((visibility_system, opacity_system, ApplyDeferred).chain());
    schedule
}
