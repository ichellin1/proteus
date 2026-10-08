//! Transition channels: named channels for 1→1 transitions from one entity
//! into another.
//!
//! ```text
//! channel::set(world, id, to, from, target, config, interruptible)
//!         │  queues a PendingChannelSet; the world isn't changed yet
//!         ▼
//! PendingChannelSets
//!         │  channel_dispatch_system, next tick, just before transition setup
//!         ▼
//! valid?  ── no ──► DroppedRequests
//!         │ yes
//!         ▼
//! a TransitionRequest on `to`, which transition_setup_system starts
//! ```
//!
//! At this level the caller passes `target`, the geometry `to` ends at.
//! `proteus-sdk` looks it up from the component's declared geometry.
//!
//! A channel with an owner entity is destroyed when the owner is, through the
//! [`OwnedChannels`] hook. A channel without one lasts until [`destroy_channel`].
//!
//! Every request that can't run is recorded in [`DroppedRequests`].

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;
use slotmap::{new_key_type, SlotMap};

use crate::component::{Lifecycle, QuadState, TransitionRequest, Visibility};
use crate::transition::TransitionConfig;

new_key_type! {
    /// Opaque handle to a registered channel.
    pub struct TransitionChannelId;
}

// ---------------------------------------------------------------------------
// ChannelRegistry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct ChannelEntry {
    owner: Option<Entity>,
}

/// Every channel that exists. A resource of [`crate::schedule::ProteusWorld`].
#[derive(Resource, Default)]
pub struct ChannelRegistry {
    channels: SlotMap<TransitionChannelId, ChannelEntry>,
}

impl ChannelRegistry {
    /// Adds a channel. Use [`create_channel`] instead, which also records the
    /// owner's [`OwnedChannels`].
    pub fn create(&mut self, owner: Option<Entity>) -> TransitionChannelId {
        self.channels.insert(ChannelEntry { owner })
    }

    /// Removes a channel. Later [`set`] calls on it are dropped with
    /// [`DropReason::ChannelNotFound`].
    pub fn destroy(&mut self, id: TransitionChannelId) {
        self.channels.remove(id);
    }

    /// Whether the channel `id` exists.
    pub fn exists(&self, id: TransitionChannelId) -> bool {
        self.channels.contains_key(id)
    }

    /// The entity that owns the channel `id`, if any.
    pub fn owner(&self, id: TransitionChannelId) -> Option<Entity> {
        self.channels.get(id).and_then(|s| s.owner)
    }
}

// ---------------------------------------------------------------------------
// OwnedChannels: destroying an entity's channels with it
// ---------------------------------------------------------------------------

/// The channels an entity owns, which are destroyed with it.
///
/// [`create_channel`] adds it to the owner. Its removal hook, from
/// [`register_channel_hooks`], destroys every channel listed, and runs both when
/// the component is removed and when the entity is destroyed.
#[derive(Component, Debug, Clone, Default)]
pub struct OwnedChannels(pub Vec<TransitionChannelId>);

/// Registers the hook that destroys an entity's owned channels. Call once,
/// before any `OwnedChannels` exists, since `bevy_ecs` panics otherwise.
/// `ProteusWorld::new` does this.
pub fn register_channel_hooks(world: &mut World) {
    world
        .register_component_hooks::<OwnedChannels>()
        .on_remove(|mut world, ctx| {
            let Some(owned) = world.get::<OwnedChannels>(ctx.entity) else {
                return;
            };
            let ids = owned.0.clone();
            if let Some(mut registry) = world.get_resource_mut::<ChannelRegistry>() {
                for id in ids {
                    registry.destroy(id);
                }
            }
        });
}

/// Creates a transition channel. If `owner` is given, the channel is destroyed along with
/// that entity; otherwise it lasts until [`destroy_channel`].
pub fn create_channel(world: &mut World, owner: Option<Entity>) -> TransitionChannelId {
    let id = world.resource_mut::<ChannelRegistry>().create(owner);
    if let Some(owner) = owner {
        world
            .entity_mut(owner)
            .entry::<OwnedChannels>()
            .or_default()
            .into_mut()
            .0
            .push(id);
    }
    id
}

/// Destroys a channel. Later [`set`] calls on it are dropped with
/// [`DropReason::ChannelNotFound`].
pub fn destroy_channel(world: &mut World, id: TransitionChannelId) {
    world.resource_mut::<ChannelRegistry>().destroy(id);
}

/// Adds this module's resources to `world`. Called once, from
/// `ProteusWorld::new`.
pub(crate) fn init_resources(world: &mut World) {
    world.init_resource::<ChannelRegistry>();
    world.init_resource::<PendingChannelSets>();
    world.init_resource::<DroppedRequests>();
}

// ---------------------------------------------------------------------------
// set()
// ---------------------------------------------------------------------------

/// One queued [`set`] call, which [`channel_dispatch_system`] applies on the
/// next tick. Public because that system is; create it with [`set`].
#[derive(Debug, Clone)]
pub struct PendingChannelSet {
    /// The channel.
    pub channel: TransitionChannelId,
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

/// The queued [`set`] calls, applied by [`channel_dispatch_system`] each tick.
#[derive(Resource, Default)]
pub struct PendingChannelSets(Vec<PendingChannelSet>);

/// Requests a transition of `to` into `target`, starting from `from`'s current
/// geometry.
///
/// This only queues the request; [`channel_dispatch_system`] applies it on the
/// next tick. From code running inside a system, which has no `&mut World`,
/// queue a closure that calls this with
/// [`crate::schedule::CommandQueue::push`].
#[allow(clippy::too_many_arguments)]
pub fn set(
    world: &mut World,
    channel: TransitionChannelId,
    to: Entity,
    from: Entity,
    target: QuadState,
    config: TransitionConfig,
    interruptible: bool,
) {
    world
        .resource_mut::<PendingChannelSets>()
        .0
        .push(PendingChannelSet {
            channel,
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
    /// The channel doesn't exist: it was destroyed, or never created.
    ChannelNotFound,
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
    /// The channel.
    pub channel: TransitionChannelId,
    /// The request's `to` entity.
    pub to: Entity,
    /// The request's `from` entity.
    pub from: Entity,
    /// Why it couldn't run.
    pub reason: DropReason,
}

/// The requests dropped this tick, recorded by [`channel_dispatch_system`].
#[derive(Resource, Default)]
pub struct DroppedRequests {
    /// The dropped requests.
    pub entries: Vec<TransitionDropped>,
}

impl DroppedRequests {
    /// Takes this tick's dropped requests, leaving the list empty.
    pub fn drain(&mut self) -> Vec<TransitionDropped> {
        std::mem::take(&mut self.entries)
    }
}

// ---------------------------------------------------------------------------
// channel_dispatch_system
// ---------------------------------------------------------------------------

/// Applies the queued [`set`] calls: each valid one becomes a
/// [`TransitionRequest`] on `to`, and each invalid one is recorded in
/// [`DroppedRequests`]. Runs just before
/// [`crate::transition::transition_setup_system`].
///
/// A valid request hides `from` and shows `to`: `to` takes over from `from`'s
/// geometry, so `from` has nothing left to show.
///
/// A request on a `to` that is already transitioning, with `interruptible`,
/// starts again from wherever `to` is. Otherwise the transition starts from
/// `from`'s current geometry.
pub fn channel_dispatch_system(
    mut commands: Commands,
    registry: Res<ChannelRegistry>,
    mut pending: ResMut<PendingChannelSets>,
    mut dropped: ResMut<DroppedRequests>,
    lifecycles: Query<&Lifecycle>,
    quad_states: Query<&QuadState>,
    visibilities: Query<Option<&Visibility>>,
) {
    dropped.entries.clear();

    for req in pending.0.drain(..) {
        macro_rules! drop_with {
            ($reason:expr) => {{
                dropped.entries.push(TransitionDropped {
                    channel: req.channel,
                    to: req.to,
                    from: req.from,
                    reason: $reason,
                });
                continue;
            }};
        }

        if !registry.exists(req.channel) {
            drop_with!(DropReason::ChannelNotFound);
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
