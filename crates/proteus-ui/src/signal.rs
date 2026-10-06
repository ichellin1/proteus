//! Signals: 1→1 transitions from one entity into another.
//!
//! ```text
//! signal::set(world, id, to, from, target, config, interruptible)
//!         │  queues a PendingSignalSet; the world isn't changed yet
//!         ▼
//! PendingSignalSets
//!         │  signal_dispatch_system, next tick, just before transition setup
//!         ▼
//! valid?  ── no ──► DroppedSignals
//!         │ yes
//!         ▼
//! a TransitionRequest on `to`, which transition_setup_system starts
//! ```
//!
//! At this level the caller passes `target`, the geometry `to` ends at.
//! `proteus-sdk` looks it up from the component's declared geometry.
//!
//! A signal with an owner entity is destroyed when the owner is, through the
//! [`OwnedSignals`] hook. A signal without one lasts until [`destroy_signal`].
//!
//! Every request that can't run is recorded in [`DroppedSignals`].

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

/// Every signal that exists. A resource of [`crate::schedule::ProteusWorld`].
#[derive(Resource, Default)]
pub struct SignalRegistry {
    signals: SlotMap<SignalId, SignalEntry>,
}

impl SignalRegistry {
    /// Adds a signal. Use [`create_signal`] instead, which also records the
    /// owner's [`OwnedSignals`].
    pub fn create(&mut self, owner: Option<Entity>) -> SignalId {
        self.signals.insert(SignalEntry { owner })
    }

    /// Removes a signal. Later [`set`] calls on it are dropped with
    /// [`DropReason::SignalNotFound`].
    pub fn destroy(&mut self, id: SignalId) {
        self.signals.remove(id);
    }

    /// Whether the signal `id` exists.
    pub fn exists(&self, id: SignalId) -> bool {
        self.signals.contains_key(id)
    }

    /// The entity that owns the signal `id`, if any.
    pub fn owner(&self, id: SignalId) -> Option<Entity> {
        self.signals.get(id).and_then(|s| s.owner)
    }
}

// ---------------------------------------------------------------------------
// OwnedSignals: destroying an entity's signals with it
// ---------------------------------------------------------------------------

/// The signals an entity owns, which are destroyed with it.
///
/// [`create_signal`] adds it to the owner. Its removal hook, from
/// [`register_signal_hooks`], destroys every signal listed, and runs both when
/// the component is removed and when the entity is destroyed.
#[derive(Component, Debug, Clone, Default)]
pub struct OwnedSignals(pub Vec<SignalId>);

/// Registers the hook that destroys an entity's owned signals. Call once,
/// before any `OwnedSignals` exists, since `bevy_ecs` panics otherwise.
/// `ProteusWorld::new` does this.
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

/// Creates a signal. If `owner` is given, the signal is destroyed along with
/// that entity; otherwise it lasts until [`destroy_signal`].
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

/// Destroys a signal. Later [`set`] calls on it are dropped with
/// [`DropReason::SignalNotFound`].
pub fn destroy_signal(world: &mut World, id: SignalId) {
    world.resource_mut::<SignalRegistry>().destroy(id);
}

/// Adds this module's resources to `world`. Called once, from
/// `ProteusWorld::new`.
pub(crate) fn init_resources(world: &mut World) {
    world.init_resource::<SignalRegistry>();
    world.init_resource::<PendingSignalSets>();
    world.init_resource::<DroppedSignals>();
}

// ---------------------------------------------------------------------------
// set()
// ---------------------------------------------------------------------------

/// One queued [`set`] call, which [`signal_dispatch_system`] applies on the
/// next tick. Public because that system is; create it with [`set`].
#[derive(Debug, Clone)]
pub struct PendingSignalSet {
    /// The signal.
    pub signal: SignalId,
    /// The entity to transition into.
    pub to: Entity,
    /// The entity to transition from.
    pub from: Entity,
    /// The geometry `to` ends at.
    pub target: QuadState,
    /// How the transition is timed.
    pub config: TransitionConfig,
    /// Whether to restart a transition already running on `to`.
    pub interruptible: bool,
}

/// The queued [`set`] calls, applied by [`signal_dispatch_system`] each tick.
#[derive(Resource, Default)]
pub struct PendingSignalSets(Vec<PendingSignalSet>);

/// Requests a transition of `to` into `target`, starting from `from`'s current
/// geometry.
///
/// This only queues the request; [`signal_dispatch_system`] applies it on the
/// next tick. From code running inside a system, which has no `&mut World`,
/// queue a closure that calls this with
/// [`crate::schedule::CommandQueue::push`].
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
// Dropped requests
// ---------------------------------------------------------------------------

/// Why a [`set`] request couldn't run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// The signal doesn't exist: it was destroyed, or never created.
    SignalNotFound,
    /// `to` or `from` has been destroyed.
    EntityNotFound,
    /// `to` is already transitioning, and the request wasn't `interruptible`.
    AlreadyTransitioning,
    /// `from` is hidden, so there is nothing visible to transition from.
    EntityNotVisible,
}

/// A [`set`] request that couldn't run.
#[derive(Debug, Clone)]
pub struct TransitionDropped {
    /// The signal.
    pub signal: SignalId,
    /// The request's `to` entity.
    pub to: Entity,
    /// The request's `from` entity.
    pub from: Entity,
    /// Why it couldn't run.
    pub reason: DropReason,
}

/// The requests dropped this tick, recorded by [`signal_dispatch_system`].
#[derive(Resource, Default)]
pub struct DroppedSignals {
    /// The dropped requests.
    pub entries: Vec<TransitionDropped>,
}

impl DroppedSignals {
    /// Takes this tick's dropped requests, leaving the list empty.
    pub fn drain(&mut self) -> Vec<TransitionDropped> {
        std::mem::take(&mut self.entries)
    }
}

// ---------------------------------------------------------------------------
// signal_dispatch_system
// ---------------------------------------------------------------------------

/// Applies the queued [`set`] calls: each valid one becomes a
/// [`TransitionRequest`] on `to`, and each invalid one is recorded in
/// [`DroppedSignals`]. Runs just before
/// [`crate::transition::transition_setup_system`].
///
/// A valid request hides `from` and shows `to`: `to` takes over from `from`'s
/// geometry, so `from` has nothing left to show.
///
/// A request on a `to` that is already transitioning, with `interruptible`,
/// starts again from wherever `to` is. Otherwise the transition starts from
/// `from`'s current geometry.
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
