//! [`SpawnOrder`]: the order entities were created in, stamped automatically
//! when an entity first gets a [`QuadState`].
//!
//! Drawing and hit testing use it to order entities with the same `z`, which
//! is most of them: the one created last is on top. The order `bevy_ecs`
//! stores entities in can't be used for this, since it has no reliable
//! relationship to creation order.
//!
//! It is stamped in the `on_add` hook, which runs only the first time an
//! entity gets a `QuadState`, not each time a transition replaces it.
//!
//! `Entity` isn't used for this because entity indices are reused after an
//! entity is destroyed, so a newer entity could sort before an older one.
//! `SpawnOrder` only increases.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;

use crate::component::QuadState;

/// This entity's place in creation order: lower was created earlier. Set once,
/// when the entity first gets a `QuadState`, and never changed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpawnOrder(pub u64);

/// The next [`SpawnOrder`] value. It only increases.
#[derive(Resource, Default)]
struct NextSpawnOrder(u64);

/// Registers the hook that stamps [`SpawnOrder`]. Call once, before any
/// `QuadState` exists, since `bevy_ecs` panics otherwise. `ProteusWorld::new`
/// does this.
///
/// The hook adds `SpawnOrder` through commands, which `World::spawn` applies
/// before it returns, so it is present as soon as the entity is.
pub fn register_spawn_order_hooks(world: &mut World) {
    world.init_resource::<NextSpawnOrder>();
    world
        .register_component_hooks::<QuadState>()
        .on_add(|mut world, ctx| {
            let next = {
                let mut counter = world.resource_mut::<NextSpawnOrder>();
                let value = counter.0;
                counter.0 += 1;
                value
            };
            world.commands().entity(ctx.entity).insert(SpawnOrder(next));
        });
}
