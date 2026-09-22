//! [`SpawnOrder`] — a monotonically increasing stamp recording relative
//! creation order, auto-assigned via `ComponentHooks` whenever [`QuadState`]
//! is first added to an entity (M13.8 fix).
//!
//! `collect_instances`' "last spawned = on top" draw-order promise was, in
//! practice, only ever backed by whatever order the underlying `bevy_ecs`
//! query happened to iterate — stable *within* one archetype, but never
//! meaningfully ordered relative to a *different* archetype (confirmed by a
//! from-scratch two-entity repro: a non-interactive entity spawned first,
//! then an interactive one spawned second, iterated in the wrong relative
//! order from the very first frame — see PLANNING.md's M13.8 section and
//! `proteus-ui/tests/render_instances.rs`'s
//! `spawning_into_an_existing_archetype_does_not_reorder_a_different_archetype`).
//! `SpawnOrder` gives `collect_instances` an explicit, archetype-independent
//! tie-breaker for entities that share the same `z` — the common case, since
//! most callers never set a nonzero `z` at all.
//!
//! Stamped via `on_add`, not `on_insert`, specifically: `on_add` only fires
//! the *first* time a component type is attached to an entity, never again
//! on a later `.insert()` replacing the same entity's `QuadState` (e.g. the
//! transition system overwriting a live entity's geometry every frame while
//! it animates) — so an actively-transitioning entity's `SpawnOrder` stays
//! fixed at its true creation moment, never "now."
//!
//! `Entity` itself was considered as the tie-breaker instead of a dedicated
//! component (no new component needed, `Entity` is already fetched by
//! `collect_instances`'s query) — rejected because `Entity` indices recycle
//! after despawn: a long-running app that repeatedly destroys and rebuilds
//! entities (exactly M13.8's own POC's own pattern) can reuse a freed index
//! for a *newer* entity, which could then sort *before* something spawned
//! in between. `SpawnOrder`'s counter is a plain `u64` that only ever
//! increases and is never reused, so it has no equivalent edge case.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;

use crate::component::QuadState;

/// This entity's position in creation order, relative to every other entity
/// ever stamped in the same `World` — lower means created earlier. Written
/// once, automatically, when `QuadState` is first added; never changes
/// afterward (see this module's top doc for why `on_add`, not `on_insert`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SpawnOrder(pub u64);

/// The next value [`SpawnOrder`] will be stamped with — a plain incrementing
/// counter, never reused (unlike `Entity` indices, which recycle on
/// despawn — see this module's top doc).
#[derive(Resource, Default)]
struct NextSpawnOrder(u64);

/// Register `SpawnOrder`'s auto-stamping hook. Call once, before any
/// `QuadState` is ever inserted — `bevy_ecs` panics if hooks are registered
/// after the component already exists in an archetype. `ProteusWorld::new()`
/// calls this during world construction, before any entity can exist —
/// mirrors [`crate::texture_ref::register_texture_ref_hooks`]'s own
/// convention exactly.
///
/// Confirmed (via a standalone scratch `bevy_ecs` binary, not assumed) that
/// a component inserted through `DeferredWorld::commands()` inside an
/// `on_add` hook is visible immediately after `World::spawn()` returns, with
/// no explicit `world.flush()` needed — `World::spawn` flushes its own
/// command queue as part of completing the spawn.
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
