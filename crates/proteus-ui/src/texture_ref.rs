//! `TextureRef` — ref-counting bridge between an ECS component and
//! `proteus_render::TextureRegistry` (M11).
//!
//! Insert alongside `BakedText`/`BakedImage`/`BakedComposite` immediately after registering a
//! `main_atlas` region via `TextureRegistry::register_static` — never on its own. `ComponentHooks`
//! (registered once, in [`register_texture_ref_hooks`], called from `ProteusWorld::new()`) keep the
//! registry's ref count in sync automatically: `on_insert` increments, `on_replace` decrements.
//! `on_replace` fires before the new value overwrites the old one (with access to the old value)
//! and always runs before `on_remove` — so it alone covers explicit reassignment
//! (`.insert()` again), explicit `.remove::<TextureRef>()`, *and* despawn uniformly, without three
//! separate hook implementations.
//!
//! Callers never call `TextureRegistry::incref`/`decref` directly.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;

use proteus_render::{QuadPipeline, TextureId};

// ---------------------------------------------------------------------------
// TextureRef component
// ---------------------------------------------------------------------------

/// Marks that an entity's baked visual owns one reference to `0` in the shared `TextureRegistry`
/// (reached via `QuadPipeline::texture_registry`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureRef(pub TextureId);

/// Register `TextureRef`'s ref-counting hooks. Call once, before any `TextureRef` is ever
/// inserted — `bevy_ecs` panics if hooks are registered after the component already exists in an
/// archetype. `ProteusWorld::new()` calls this during world construction, before any entity can
/// exist.
pub fn register_texture_ref_hooks(world: &mut World) {
    world
        .register_component_hooks::<TextureRef>()
        .on_insert(|mut world, ctx| {
            let Some(&TextureRef(id)) = world.get::<TextureRef>(ctx.entity) else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.texture_registry.incref(id);
            }
        })
        .on_replace(|mut world, ctx| {
            let Some(&TextureRef(id)) = world.get::<TextureRef>(ctx.entity) else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.texture_registry.decref(id);
            }
        });
}

// ---------------------------------------------------------------------------
// Recency system
// ---------------------------------------------------------------------------

/// Bumps every live `TextureRef`'s recency once per frame, for the registry's LRU eviction
/// ordering. Graceful no-op when `QuadPipeline` isn't present yet (the same convention every other
/// GPU-touching system in this crate follows — e.g. `bake_system`).
pub fn touch_texture_refs_system(
    mut pipeline: Option<ResMut<QuadPipeline>>,
    texture_refs: Query<&TextureRef>,
) {
    let Some(pipeline) = pipeline.as_deref_mut() else {
        return;
    };
    pipeline.texture_registry.advance_frame();
    for &TextureRef(id) in texture_refs.iter() {
        pipeline.texture_registry.touch(id);
    }
}
