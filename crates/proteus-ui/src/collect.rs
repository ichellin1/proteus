//! Instance collection — bridges ECS `QuadState` to GPU-ready `QuadInstance` data.
//!
//! The render loop's job is a two-step bridge:
//!
//! 1. [`collect_instances`] — reads the ECS world and returns a `Vec<QuadInstance>`
//!    that represents every visible entity, ready for the GPU.
//! 2. The shell passes that vec to `QuadPipeline::upload_instances` and then draws.
//!
//! ## Two instances per text entity
//!
//! An entity with a [`BakedText`] component emits **two** `QuadInstance`s:
//!
//! | Layer | UV | Color |
//! |---|---|---|
//! | Background | `WHITE_PIXEL_UV` | `QuadState::color` (solid fill) |
//! | Text overlay | `BakedText::uv_offset / uv_scale` | `Text::color` (defaults to white) |
//!
//! Plain quads (no `BakedText`) emit one instance: the solid-color background only.
//! Virtual entities created during group transitions also have no `BakedText`, so
//! they correctly emit a single instance.
//!
//! ## Drop shadow
//!
//! When a [`DropShadow`] component is present on an entity it is applied to the
//! **background** instance only.  The text overlay instance always has
//! `shadow_color.a = 0` (no shadow) so that the shadow does not double-render
//! beneath the glyph layer.
//!
//! ## Visibility
//!
//! Entities with [`Visibility::HIDDEN`] are excluded from the output. Entities with
//! no `Visibility` component at all are treated as visible (the default).

use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{QuadInstance, QuadPipeline};

use crate::{effects::DropShadow, BakedText, QuadState, Text, Visibility};

// ---------------------------------------------------------------------------
// quad_state_to_instance
// ---------------------------------------------------------------------------

/// Convert a [`QuadState`], optional [`BakedText`], and optional [`DropShadow`]
/// into a [`QuadInstance`].
///
/// - If `baked` is `None` the UV fields address the white-pixel sentinel in
///   `main_atlas`, so the quad renders as a solid colour.
/// - If `baked` is `Some` the UV fields address the text glyph region in
///   `main_atlas`, so the quad renders the baked text overlay.
/// - If `shadow` is `Some` the shadow fields are populated; `shadow_color.a`
///   controls whether the shader actually renders a shadow.
///
/// In the two-instance model this is called **twice** per text entity:
/// once with `baked = None, shadow = Some(...)` for the background, and once
/// with `baked = Some, shadow = None` for the text overlay.
pub fn quad_state_to_instance(
    qs: &QuadState,
    baked: Option<&BakedText>,
    shadow: Option<&DropShadow>,
) -> QuadInstance {
    let (uv_offset, uv_scale) = match baked {
        Some(b) => (b.uv_offset, b.uv_scale),
        None => (
            QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            QuadPipeline::WHITE_PIXEL_UV_SCALE,
        ),
    };

    let (shadow_params, shadow_color) = match shadow {
        Some(s) => (
            [s.offset.x, s.offset.y, s.softness, s.spread],
            s.color.to_array(),
        ),
        None => ([0.0f32; 4], [0.0f32; 4]),
    };

    QuadInstance {
        position: qs.position.to_array(),
        size: qs.size.to_array(),
        rotation: qs.rotation,
        scale: qs.scale,
        anchor: qs.anchor.to_array(),
        color: qs.color.to_array(),
        opacity: 1.0,
        corner_radius: qs.corner_radius,
        uv_offset,
        uv_scale,
        atlas_page: 0,
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params,
        shadow_color,
    }
}

// ---------------------------------------------------------------------------
// collect_instances
// ---------------------------------------------------------------------------

/// Collect all visible [`QuadInstance`]s from the ECS world.
///
/// Call this once per frame after `ProteusWorld::update()`, then pass the
/// returned vec to [`QuadPipeline::upload_instances`].
///
/// Ordering within the returned vec matches the order entities were inserted
/// into the world. For text entities the background instance always precedes
/// the text overlay instance.
pub fn collect_instances(world: &mut World) -> Vec<QuadInstance> {
    // Collect (entity, QuadState, Option<visible>) while holding the query borrow,
    // then drop it before calling world.get() for BakedText / Text / DropShadow.
    let states: Vec<(Entity, QuadState, Option<bool>)> = {
        let mut q = world.query::<(Entity, &QuadState, Option<&Visibility>)>();
        q.iter(world)
            .map(|(e, qs, vis)| (e, qs.clone(), vis.map(|v| v.visible)))
            .collect()
    };

    let mut out = Vec::new();
    for (e, qs, vis) in states {
        // Entities with no Visibility component default to visible.
        if !vis.is_none_or(|v| v) {
            continue;
        }
        // (a) Solid-color background quad — shadow applied here, if present.
        let shadow = world.get::<DropShadow>(e);
        out.push(quad_state_to_instance(&qs, None, shadow));
        // (b) Text overlay — only for entities with baked glyph data.
        //     No shadow on the overlay: it sits on top of the background (which
        //     already casts the shadow), so doubling it would look wrong.
        if let Some(b) = world.get::<BakedText>(e) {
            let text_color = world.get::<Text>(e).map(|t| t.color).unwrap_or(Vec4::ONE);
            let mut text_qs = qs.clone();
            text_qs.color = text_color;
            out.push(quad_state_to_instance(&text_qs, Some(b), None));
        }
    }
    out
}
