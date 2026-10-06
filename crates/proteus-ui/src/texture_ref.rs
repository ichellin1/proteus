//! [`TextureRef`]: counts which entities use each texture in the main atlas.
//!
//! Add a `TextureRef` alongside `BakedText`, `BakedImage` or `BakedComposite`,
//! right after registering the atlas region. Component hooks, from
//! [`register_texture_ref_hooks`], keep the registry's reference count right:
//! adding one increments it, and replacing, removing or destroying it
//! decrements it. The `on_replace` hook covers all three, since it runs before
//! `on_remove` as well as on replacement.
//!
//! Don't call `TextureRegistry::incref` or `decref` directly.

use bevy_ecs::prelude::*;
use bevy_ecs::world::World;

use proteus_render::{QuadPipeline, TextureId};

// ---------------------------------------------------------------------------
// TextureRef component
// ---------------------------------------------------------------------------

/// One reference, held by this entity, to a texture in the main atlas. While
/// any entity holds one, the texture isn't evicted.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureRef(pub TextureId);

/// Registers the hooks that count texture references. Call once, before any
/// `TextureRef` exists, since `bevy_ecs` panics otherwise. `ProteusWorld::new`
/// does this.
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

/// Marks every texture in use as used this tick, so the least recently used
/// are evicted first. Does nothing until the GPU resources exist.
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
