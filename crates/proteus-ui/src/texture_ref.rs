//! Texture references: which entities use each texture in the main atlas.
//!
//! An entity holds one reference per kind of content it draws from the atlas,
//! since it can draw several at once, such as text over an image:
//!
//! - [`TextTextureRef`] alongside `BakedText`,
//! - [`ImageTextureRef`] alongside `BakedImage`,
//! - [`CompositeTextureRef`] alongside `BakedComposite`.
//!
//! Add the reference right after registering the atlas region. Component
//! hooks, from [`register_texture_ref_hooks`], keep the registry's reference
//! count right: adding one increments it, and replacing, removing or
//! destroying it decrements it. The `on_replace` hook covers all three, since
//! it runs before `on_remove` as well as on replacement.
//!
//! Don't call `TextureRegistry::incref` or `decref` directly.

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use bevy_ecs::world::World;

use proteus_render::{QuadPipeline, TextureId};

// ---------------------------------------------------------------------------
// Reference components
// ---------------------------------------------------------------------------

/// This entity's reference to the texture of its `BakedText`. While any
/// entity holds a reference to a texture, it isn't evicted.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextTextureRef(pub TextureId);

/// This entity's reference to the texture of its `BakedImage`. While any
/// entity holds a reference to a texture, it isn't evicted.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageTextureRef(pub TextureId);

/// This entity's reference to the texture of its `BakedComposite`. While any
/// entity holds a reference to a texture, it isn't evicted.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositeTextureRef(pub TextureId);

/// What the hooks and the recency system need from each reference component.
trait TextureRefComponent: Component<Mutability = Mutable> + Copy {
    fn texture_id(self) -> TextureId;
}

impl TextureRefComponent for TextTextureRef {
    fn texture_id(self) -> TextureId {
        self.0
    }
}

impl TextureRefComponent for ImageTextureRef {
    fn texture_id(self) -> TextureId {
        self.0
    }
}

impl TextureRefComponent for CompositeTextureRef {
    fn texture_id(self) -> TextureId {
        self.0
    }
}

/// Registers the hooks that count texture references. Call once, before any
/// reference exists, since `bevy_ecs` panics otherwise. `ProteusWorld::new`
/// does this.
pub fn register_texture_ref_hooks(world: &mut World) {
    register_hooks::<TextTextureRef>(world);
    register_hooks::<ImageTextureRef>(world);
    register_hooks::<CompositeTextureRef>(world);
}

fn register_hooks<R: TextureRefComponent>(world: &mut World) {
    world
        .register_component_hooks::<R>()
        .on_insert(|mut world, ctx| {
            let Some(&r) = world.get::<R>(ctx.entity) else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.texture_registry.incref(r.texture_id());
            }
        })
        .on_replace(|mut world, ctx| {
            let Some(&r) = world.get::<R>(ctx.entity) else {
                return;
            };
            if let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() {
                pipeline.texture_registry.decref(r.texture_id());
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
    text_refs: Query<&TextTextureRef>,
    image_refs: Query<&ImageTextureRef>,
    composite_refs: Query<&CompositeTextureRef>,
) {
    let Some(pipeline) = pipeline.as_deref_mut() else {
        return;
    };
    pipeline.texture_registry.advance_frame();
    let ids = text_refs
        .iter()
        .map(|r| r.0)
        .chain(image_refs.iter().map(|r| r.0))
        .chain(composite_refs.iter().map(|r| r.0));
    for id in ids {
        pipeline.texture_registry.touch(id);
    }
}
