//! Components held in a line of text.
//!
//! A text's span can hold a component in place of text, with
//! [`Span::component`](crate::Span::component). The text keeps a gap the
//! span's size in its line, which wraps as a word does. Each tick,
//! [`inline_components_system`] places the component in its gap:
//! - It makes the component a child of the text's component, as `add_child`
//!   does, since the gap's position only means something relative to the text.
//! - It sets the component's position, so the component's anchor sits at the
//!   same point of its gap: its center at the gap's center by default. The
//!   text owns that position. It's set again whenever the text, the fonts, the
//!   display scale or either anchor changes.
//! - It hides a component whose gap the text doesn't show, with
//!   [`HiddenByText`]: one cut off by the text's maximum number of lines, or in
//!   an ellipsis that isn't shown, and every one in a text that waits for a
//!   font. The app's own [`Visibility`](crate::Visibility) for it is kept.
//!
//! The text is laid out in the app, with the fonts in [`Fonts`], at the bake
//! scale, so the gap and the component line up in the same frame, before the
//! text is baked. A component no longer in its text stays a child where it was
//! last placed.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::bake::BakeScale;
use crate::component::{DeclaredGeometry, QuadState};
use crate::interaction::InteractionState;
use crate::text::{lay_out_text, page_side, Fonts, LaidOutText, Text};

/// A text's components, laid out, kept on the text's entity. The text is laid
/// out again only when it, the fonts, the bake scale or the atlas page size
/// change.
#[derive(Component, Debug, Clone)]
pub struct InlineLayout {
    /// The text as it was laid out.
    text: Text,
    /// [`Fonts::version`] when it was laid out.
    fonts_version: u64,
    /// The bake scale it was laid out at.
    scale: f32,
    /// The atlas page's side it was laid out for.
    max: u32,
    /// The laid-out text, or `None` while it waits for a font or draws
    /// nothing.
    laid_out: Option<LaidOutText>,
    /// The components the text holds and places, shown or not.
    components: Vec<Entity>,
}

/// Hides a component held in a text that doesn't show it: its gap is cut off,
/// or its text waits for a font. [`crate::visibility_system`] treats it as
/// hidden, along with its children, and the app's own visibility for it is
/// kept.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct HiddenByText;

/// Places each component held in a text in its gap, and hides the ones the
/// text doesn't show. See the module docs.
pub fn inline_components_system(world: &mut World) {
    let changed: Vec<Entity> = world
        .query_filtered::<(Entity, &Text), (Changed<Text>, Without<InlineLayout>)>()
        .iter(world)
        .filter(|(_, text)| !text.components().is_empty())
        .map(|(entity, _)| entity)
        .collect();
    let laid_out: Vec<Entity> = world
        .query_filtered::<Entity, With<InlineLayout>>()
        .iter(world)
        .collect();
    if changed.is_empty() && laid_out.is_empty() {
        return;
    }
    let scale = world.get_resource::<BakeScale>().map_or(1.0, |s| s.0);
    let max = page_side(world);
    world.get_resource_or_insert_with(Fonts::default);
    for text in changed.into_iter().chain(laid_out) {
        lay_out(world, text, scale, max);
        place(world, text);
    }
}

/// Lays `text`'s components out, if it, the fonts, the scale or the page size
/// changed since they were last laid out, and makes them its children.
fn lay_out(world: &mut World, entity: Entity, scale: f32, max: u32) {
    let fonts_version = world.resource::<Fonts>().version();
    let Some(text) = world.get::<Text>(entity).cloned() else {
        if let Some(old) = world.entity_mut(entity).take::<InlineLayout>() {
            release(world, &old.components);
        }
        return;
    };
    if let Some(old) = world.get::<InlineLayout>(entity) {
        if old.text == text
            && old.fonts_version == fonts_version
            && old.scale == scale
            && old.max == max
        {
            return;
        }
    }
    let old = world
        .get::<InlineLayout>(entity)
        .map(|old| old.components.clone())
        .unwrap_or_default();
    let components: Vec<Entity> = text
        .components()
        .into_iter()
        .filter(|&component| adopt(world, entity, component))
        .collect();
    let gone: Vec<Entity> = old
        .into_iter()
        .filter(|c| !components.contains(c))
        .collect();
    release(world, &gone);
    if components.is_empty() {
        world.entity_mut(entity).remove::<InlineLayout>();
        return;
    }
    let laid_out = world
        .resource_scope(|_, mut fonts: Mut<Fonts>| lay_out_text(&mut fonts, &text, scale, max))
        .flatten();
    world.entity_mut(entity).insert(InlineLayout {
        text,
        fonts_version,
        scale,
        max,
        laid_out,
        components,
    });
}

/// Makes `component` a child of `text`, the entity whose text holds it, and
/// returns whether it can be placed there. A component that no longer exists
/// can't, and neither can `text` itself or one of its ancestors, which would
/// be its own ancestor, with a warning.
fn adopt(world: &mut World, text: Entity, component: Entity) -> bool {
    if world.get_entity(component).is_err() {
        return false;
    }
    let mut ancestor = Some(text);
    while let Some(entity) = ancestor {
        if entity == component {
            log::warn!(
                "the text of entity {text:?} holds entity {component:?}, which is the text's \
                 own component or one of its parents: it's left where it is. A text can hold \
                 only a component that isn't one of its parents"
            );
            return false;
        }
        ancestor = world.get::<ChildOf>(entity).map(ChildOf::parent);
    }
    if world.get::<ChildOf>(component).map(ChildOf::parent) != Some(text) {
        world.entity_mut(component).insert(ChildOf(text));
    }
    true
}

/// Shows again each of `components`, which a text no longer holds, as the app
/// left it.
fn release(world: &mut World, components: &[Entity]) {
    for &component in components {
        if let Ok(mut entity) = world.get_entity_mut(component) {
            entity.remove::<HiddenByText>();
        }
    }
}

/// Places each of `text`'s components in its gap, by the text's anchor and
/// the component's own, and hides the ones the text doesn't show.
fn place(world: &mut World, text: Entity) {
    let Some(layout) = world.get::<InlineLayout>(text) else {
        return;
    };
    let components = layout.components.clone();
    let laid_out = layout.laid_out.clone();
    let text_anchor = world
        .get::<QuadState>(text)
        .map_or(Vec2::splat(0.5), |q| q.anchor);
    for component in components {
        let gap = laid_out.as_ref().and_then(|l| {
            let (_, position, size) = l.components.iter().find(|c| c.0 == component)?;
            Some((l.size, *position, *size))
        });
        let Ok(mut entity) = world.get_entity_mut(component) else {
            continue;
        };
        let Some((text_size, position, size)) = gap else {
            if !entity.contains::<HiddenByText>() {
                entity.insert(HiddenByText);
            }
            continue;
        };
        if entity.contains::<HiddenByText>() {
            entity.remove::<HiddenByText>();
        }
        // The text's box is drawn with its anchor at the component's position,
        // the point a child is placed from. y is up here, and down in the box.
        let anchor = entity
            .get::<QuadState>()
            .map_or(Vec2::splat(0.5), |q| q.anchor);
        let point = position + anchor * size;
        let at = Vec2::new(
            point.x - text_anchor.x * text_size.x,
            text_anchor.y * text_size.y - point.y,
        );
        move_to(&mut entity, at);
    }
}

/// Moves `entity` to `at`, from its parent's anchor, where it rests: its
/// geometry, its declared geometry and the one its interaction styles apply
/// on top of. Nothing changes if it's there already.
fn move_to(entity: &mut EntityWorldMut, at: Vec2) {
    let rests_at = entity
        .get::<DeclaredGeometry>()
        .map(|d| d.0.position.truncate())
        .or_else(|| entity.get::<QuadState>().map(|q| q.position.truncate()));
    if rests_at == Some(at) {
        return;
    }
    if let Some(mut declared) = entity.get_mut::<DeclaredGeometry>() {
        declared.0.position = at.extend(declared.0.position.z);
    }
    if let Some(mut interaction) = entity.get_mut::<InteractionState>() {
        interaction.declared.position = at.extend(interaction.declared.position.z);
    }
    if let Some(mut quad) = entity.get_mut::<QuadState>() {
        quad.position = at.extend(quad.position.z);
    }
}
