//! Signal system — the developer-facing trigger layer for transitions (M12.1).
//!
//! ## Data flow
//!
//! ```text
//! signal::set(world, id, to, from, target, config, interruptible)
//!         │  pushes a PendingSignalSet — world is not mutated synchronously
//!         ▼
//! PendingSignalSets  (Resource)
//!         │  signal_dispatch_system drains this each frame, in
//!         │  ProteusSet::SignalDispatch — immediately before TransitionSetup
//!         ▼
//! validated?  ──── no ───► DroppedSignals (Resource)
//!         │ yes
//!         ▼
//! TransitionRequest inserted on `to` — the existing, already-correct
//! transition_setup_system (transition.rs) takes it from there.
//! ```
//!
//! ## Scope note
//!
//! `target` — the geometry `to` should morph into — is supplied by the caller
//! explicitly, matching what [`crate::component::TransitionRequest`] already
//! requires. Resolving it automatically from a component's own *declared*
//! rest state (the JS-API shape sketched in PLANNING.md's Phase A, where
//! `signal.set([to.id(), from.id()])` needs no separate target argument) needs
//! a place to store that declared state per entity — a component-declaration
//! concept that belongs to the generic app API (M12.3's `proteus-sdk` crate),
//! not to this ECS-level primitive.
//!
//! ## Ownership
//!
//! A signal created with `owner: Some(entity)` is destroyed automatically when
//! that entity despawns (via [`OwnedSignals`]'s despawn hook — same pattern as
//! `texture_ref.rs`'s `TextureRef` ref-counting). A signal created with
//! `owner: None` lives until [`destroy_signal`] is called explicitly.
//!
//! ## `TransitionDropped` reporting
//!
//! Every declined request is recorded in [`DroppedSignals`], always populated
//! (no `cfg(debug_assertions)` gate at this layer — cheap, matches
//! [`crate::input::InteractionEvents`] and
//! [`crate::transition::CompletedTransitions`]). PLANNING.md's two-tier
//! dev-automatic / release-opt-in *handler* distinction is a cost concern
//! about per-signal callback dispatch, which belongs to the SDK layer
//! (M12.3+) built on top of this resource — not to collecting the drops
//! themselves.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;
use slotmap::{new_key_type, SlotMap};

use crate::component::{Lifecycle, QuadState, TransitionRequest, Visibility};
use crate::transition::TransitionConfig;

new_key_type! {
    /// Opaque handle to a registered signal.
    pub struct SignalId;
}

// ---------------------------------------------------------------------------
// SignalRegistry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct SignalEntry {
    owner: Option<Entity>,
}

/// Registered signals. One instance lives as a `Resource` on
/// [`crate::schedule::ProteusWorld`].
#[derive(Resource, Default)]
pub struct SignalRegistry {
    signals: SlotMap<SignalId, SignalEntry>,
}

impl SignalRegistry {
    /// Register a new signal. Prefer [`create_signal`] over calling this
    /// directly — it also wires up [`OwnedSignals`] for an owned signal.
    pub fn create(&mut self, owner: Option<Entity>) -> SignalId {
        self.signals.insert(SignalEntry { owner })
    }

    /// Remove a signal from the registry. Further [`set`] calls referencing
    /// `id` are dropped with [`DropReason::SignalNotFound`].
    pub fn destroy(&mut self, id: SignalId) {
        self.signals.remove(id);
    }

    /// True if `id` is currently registered.
    pub fn exists(&self, id: SignalId) -> bool {
        self.signals.contains_key(id)
    }

    /// The owner entity `id` was created with, if any.
    pub fn owner(&self, id: SignalId) -> Option<Entity> {
        self.signals.get(id).and_then(|s| s.owner)
    }
}

// ---------------------------------------------------------------------------
// OwnedSignals — despawn-cleanup for owned signals
// ---------------------------------------------------------------------------

/// Tracks which [`SignalId`]s an entity owns, for automatic cleanup on
/// despawn.
///
/// Attached to the owner entity by [`create_signal`] when `owner` is `Some`.
/// Its `on_remove` hook (wired by [`register_signal_hooks`]) destroys every
/// id it lists — `on_remove` fires on explicit component removal *and* on
/// despawn, so despawning the owner destroys its signals with it. Same
/// pattern `texture_ref.rs` uses for `TextureRef`'s ref-count hook.
#[derive(Component, Debug, Clone, Default)]
pub struct OwnedSignals(pub Vec<SignalId>);

/// Register `OwnedSignals`'s despawn-cleanup hook. Call once, before any
/// `OwnedSignals` is ever inserted — `bevy_ecs` panics if hooks are
/// registered after the component already exists in an archetype. Same
/// requirement and call site (`ProteusWorld::new()`) as
/// `texture_ref::register_texture_ref_hooks`.
pub fn register_signal_hooks(world: &mut World) {
    world
        .register_component_hooks::<OwnedSignals>()
        .on_remove(|mut world, ctx| {
            let Some(owned) = world.get::<OwnedSignals>(ctx.entity) else {
                return;
            };
            let ids = owned.0.clone();
            if let Some(mut registry) = world.get_resource_mut::<SignalRegistry>() {
                for id in ids {
                    registry.destroy(id);
                }
            }
        });
}

/// Create a new signal in `world`'s [`SignalRegistry`].
///
/// When `owner` is `Some`, the signal is destroyed automatically when the
/// owner entity despawns — the caller never needs to call [`destroy_signal`]
/// for component-scoped signals. When `owner` is `None`, the signal lives
/// until [`destroy_signal`] is called explicitly.
pub fn create_signal(world: &mut World, owner: Option<Entity>) -> SignalId {
    let id = world.resource_mut::<SignalRegistry>().create(owner);
    if let Some(owner) = owner {
        world
            .entity_mut(owner)
            .entry::<OwnedSignals>()
            .or_default()
            .into_mut()
            .0
            .push(id);
    }
    id
}

/// Explicitly destroy a signal. Further [`set`] calls referencing `id` are
/// dropped with [`DropReason::SignalNotFound`].
pub fn destroy_signal(world: &mut World, id: SignalId) {
    world.resource_mut::<SignalRegistry>().destroy(id);
}

/// Initialize every resource this module owns. Called once from
/// `ProteusWorld::new()`, alongside [`register_signal_hooks`] — a single
/// entry point rather than making the caller init each resource individually.
pub(crate) fn init_resources(world: &mut World) {
    world.init_resource::<SignalRegistry>();
    world.init_resource::<PendingSignalSets>();
    world.init_resource::<DroppedSignals>();
}

// ---------------------------------------------------------------------------
// set() — the developer-facing entry point
// ---------------------------------------------------------------------------

/// One queued `set()` call, applied by [`signal_dispatch_system`] on the next
/// frame.
///
/// `pub`, like [`crate::transition::ActiveTransition`] and
/// [`crate::transition::CompletedTransitions`] — internal machinery a
/// well-formed `pub fn signal_dispatch_system` system needs its `ResMut`
/// parameter type to be at least as visible as the function itself, not
/// something callers are expected to construct directly (use [`set`]).
#[derive(Debug, Clone)]
pub struct PendingSignalSet {
    pub signal: SignalId,
    pub to: Entity,
    pub from: Entity,
    pub target: QuadState,
    pub config: TransitionConfig,
    pub interruptible: bool,
}

/// Queue of pending [`set`] calls, drained by [`signal_dispatch_system`] each
/// frame.
#[derive(Resource, Default)]
pub struct PendingSignalSets(Vec<PendingSignalSet>);

/// Declare a transition: `to` should morph into `target`, appearing to
/// originate from `from`'s current geometry. Mirrors the TypeScript API's
/// `signal.set([to, from], config)`, with `target` as the extra ECS-level
/// argument described in this module's doc.
///
/// This only enqueues the request — `world` is not mutated synchronously.
/// [`signal_dispatch_system`] (runs in
/// [`crate::schedule::ProteusSet::SignalDispatch`], immediately before
/// transition setup) processes it on the next [`crate::schedule::ProteusWorld::update`]
/// call.
///
/// Calling this from inside an interaction callback — i.e. while some other
/// system is mid-execution — is unsound (it needs `&mut World`, which
/// callbacks don't have). Route through
/// [`crate::schedule::CommandQueue::push`] instead: push a closure that calls
/// `signal::set`, applied safely at the start of next frame.
#[allow(clippy::too_many_arguments)]
pub fn set(
    world: &mut World,
    signal: SignalId,
    to: Entity,
    from: Entity,
    target: QuadState,
    config: TransitionConfig,
    interruptible: bool,
) {
    world
        .resource_mut::<PendingSignalSets>()
        .0
        .push(PendingSignalSet {
            signal,
            to,
            from,
            target,
            config,
            interruptible,
        });
}

// ---------------------------------------------------------------------------
// TransitionDropped reporting
// ---------------------------------------------------------------------------

/// Why [`signal_dispatch_system`] declined to start a requested transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// `signal` doesn't exist in the registry (never created, or already destroyed).
    SignalNotFound,
    /// `to` or `from` no longer exists (despawned since the request was queued).
    EntityNotFound,
    /// `to` is already `Lifecycle::Transitioning` and the request didn't set `interruptible`.
    AlreadyTransitioning,
    /// `from` exists but is explicitly `Visibility { visible: false }` — nothing to visually
    /// originate the morph from.
    EntityNotVisible,
}

/// One request [`signal_dispatch_system`] declined to act on.
#[derive(Debug, Clone)]
pub struct TransitionDropped {
    pub signal: SignalId,
    pub to: Entity,
    pub from: Entity,
    pub reason: DropReason,
}

/// Requests dropped this frame. Cleared and repopulated every
/// [`signal_dispatch_system`] run — same drain-per-frame convention as
/// [`crate::input::InteractionEvents`] and
/// [`crate::transition::CompletedTransitions`].
#[derive(Resource, Default)]
pub struct DroppedSignals {
    pub entries: Vec<TransitionDropped>,
}

impl DroppedSignals {
    /// Take all of this frame's drops, leaving the internal list empty.
    pub fn drain(&mut self) -> Vec<TransitionDropped> {
        std::mem::take(&mut self.entries)
    }
}

// ---------------------------------------------------------------------------
// signal_dispatch_system
// ---------------------------------------------------------------------------

/// Drains [`PendingSignalSets`] (queued by [`set`]) and, for each request that
/// validates, inserts a [`TransitionRequest`] on `to` — bridging to the
/// existing transition machinery in `transition.rs` rather than duplicating
/// it. Requests that fail validation are recorded in [`DroppedSignals`]
/// instead.
///
/// Runs in [`crate::schedule::ProteusSet::SignalDispatch`], immediately
/// before [`crate::transition::transition_setup_system`].
///
/// `from`'s `Visibility` is set to `false` as part of dispatching a valid
/// request — "the morph is the exit," per PLANNING.md's Phase B: `to` carries
/// the entire visual from `from`'s geometry to its own, so `from` has nothing
/// left to show once the transition starts.
///
/// Retargeting: if `to` is already `Transitioning` and the request set
/// `interruptible`, the inserted `TransitionRequest` leaves `from_state` as
/// `None` — `transition_setup_system` then snapshots `to`'s own current
/// (mid-flight) `QuadState` as the new origin, exactly like a direct
/// `TransitionRequest` retarget (see
/// `tests/transition_systems.rs::retargeting_midtransition_starts_from_current_state`).
/// A fresh (non-retarget) dispatch instead sets `from_state` explicitly to
/// `from`'s current `QuadState`.
pub fn signal_dispatch_system(
    mut commands: Commands,
    registry: Res<SignalRegistry>,
    mut pending: ResMut<PendingSignalSets>,
    mut dropped: ResMut<DroppedSignals>,
    lifecycles: Query<&Lifecycle>,
    quad_states: Query<&QuadState>,
    visibilities: Query<Option<&Visibility>>,
) {
    dropped.entries.clear();

    for req in pending.0.drain(..) {
        macro_rules! drop_with {
            ($reason:expr) => {{
                dropped.entries.push(TransitionDropped {
                    signal: req.signal,
                    to: req.to,
                    from: req.from,
                    reason: $reason,
                });
                continue;
            }};
        }

        if !registry.exists(req.signal) {
            drop_with!(DropReason::SignalNotFound);
        }

        let Ok(from_state) = quad_states.get(req.from) else {
            drop_with!(DropReason::EntityNotFound);
        };
        if quad_states.get(req.to).is_err() {
            drop_with!(DropReason::EntityNotFound);
        }

        let from_visible = visibilities
            .get(req.from)
            .ok()
            .flatten()
            .is_none_or(|v| v.visible);
        if !from_visible {
            drop_with!(DropReason::EntityNotVisible);
        }

        let already_transitioning = lifecycles
            .get(req.to)
            .map(|l| *l == Lifecycle::Transitioning)
            .unwrap_or(false);
        if already_transitioning && !req.interruptible {
            drop_with!(DropReason::AlreadyTransitioning);
        }

        let from_state_override = if already_transitioning {
            None
        } else {
            Some(from_state.clone())
        };

        commands.entity(req.to).insert(TransitionRequest {
            to: req.target,
            config: req.config,
            from_state: from_state_override,
        });
        commands
            .entity(req.from)
            .insert(Visibility { visible: false });
        // Reveal after the hide, so `set(x, x)` leaves `x` visible and
        // degenerates into an `animate_to` rather than hiding it.
        commands.entity(req.to).insert(Visibility::VISIBLE);
    }
}
