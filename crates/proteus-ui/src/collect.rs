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
//! Virtual entities created during group transitions may have a `BakedText` component
//! propagated from their source entity.  In that case the text overlay instance has its
//! `opacity` reduced proportionally to the transition progress so the source text
//! dissolves smoothly rather than snapping off at the end.
//!
//! ## Drop shadow
//!
//! When a [`DropShadow`] component is present on an entity it is applied to the
//! **background** instance only.  The text overlay instance always has
//! `shadow_color.a = 0` (no shadow) so that the shadow does not double-render
//! beneath the glyph layer.
//!
//! ## Glow (M8.6)
//!
//! When a [`Glow`] component is present and no [`DropShadow`] is present,
//! the glow is encoded into the same `shadow_params`/`shadow_color` slots with
//! a zero offset (producing a symmetric halo).  If both are present, `DropShadow`
//! takes precedence.
//!
//! ## Video (M9)
//!
//! When a [`crate::VideoPlayer`] component is present the background instance's
//! `atlas_page` is set to `2` and the UV is set to cover the full `video_atlas`
//! texture.  The entity's `QuadState::color` acts as a tint; use `Vec4::ONE`
//! (white) for unfiltered video.  Any `BakedText` overlay is still emitted on
//! top as a second instance so labels can float above the video.
//!
//! ## Visibility
//!
//! Entities with [`Visibility::HIDDEN`] are excluded from the output. Entities with
//! no `Visibility` component at all are treated as visible (the default).

use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{QuadInstance, QuadPipeline};

use crate::{
    effects::{DropShadow, Glow},
    video::VideoPlayer,
    ActiveTransition, BakedText, QuadState, Text, Virtual, Visibility,
};

// ---------------------------------------------------------------------------
// quad_state_to_instance
// ---------------------------------------------------------------------------

/// Convert a [`QuadState`], optional [`BakedText`], optional [`DropShadow`],
/// and optional [`Glow`] into a [`QuadInstance`].
///
/// - If `baked` is `None` the UV fields address the white-pixel sentinel in
///   `main_atlas`, so the quad renders as a solid colour.
/// - If `baked` is `Some` the UV fields address the text glyph region in
///   `main_atlas`, so the quad renders the baked text overlay.
/// - If `shadow` is `Some` the shadow fields are populated from it; `glow` is
///   ignored (DropShadow takes precedence).
/// - If `shadow` is `None` and `glow` is `Some`, the glow is encoded into the
///   same shadow slots with a zero offset, producing a symmetric halo.  The halo
///   color is taken from `Glow::color` and is independent of the entity's fill.
///
/// In the two-instance model this is called **twice** per text entity:
/// once with `baked = None, shadow = Some(...), glow = Some(...)` for the
/// background, and once with `baked = Some, shadow = None, glow = None` for
/// the text overlay.
pub fn quad_state_to_instance(
    qs: &QuadState,
    baked: Option<&BakedText>,
    shadow: Option<&DropShadow>,
    glow: Option<&Glow>,
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
        None => match glow {
            Some(g) => (
                [0.0, 0.0, g.radius, 0.0],
                [g.color.x, g.color.y, g.color.z, g.color.w * g.intensity],
            ),
            None => ([0.0f32; 4], [0.0f32; 4]),
        },
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
    // then drop it before calling world.get() for BakedText / Text / DropShadow / Glow.
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
        // (a) Solid-color (or video) background quad — shadow or glow applied here.
        //     DropShadow takes precedence over Glow when both are present.
        //     When the entity has a VideoPlayer component the quad samples from
        //     atlas_page 2 (video_atlas) with UV covering the full texture.
        let shadow = world.get::<DropShadow>(e);
        let glow = world.get::<Glow>(e);
        let mut bg_inst = quad_state_to_instance(&qs, None, shadow, glow);
        if world.get::<VideoPlayer>(e).is_some() {
            bg_inst.atlas_page   = 2;
            bg_inst.uv_offset    = [0.0, 0.0];
            bg_inst.uv_scale     = [1.0, 1.0];
        }
        out.push(bg_inst);
        // (b) Text overlay — only for entities with baked glyph data.
        //     No shadow/glow on the overlay: it sits on top of the background (which
        //     already casts the shadow/glow), so doubling it would look wrong.
        if let Some(b) = world.get::<BakedText>(e) {
            let text_color = world.get::<Text>(e).map(|t| t.color).unwrap_or(Vec4::ONE);
            let mut text_qs = qs.clone();
            text_qs.color = text_color;
            let mut text_inst = quad_state_to_instance(&text_qs, Some(b), None, None);

            // Virtual entities carry the *source* entity's text during a group
            // transition.  Fade it out in sync with the geometry so that the
            // texels dissolve rather than snapping off at completion.
            // We mirror the same eased-t computation that transition_tick_system
            // uses so the text fade tracks the geometry exactly.
            if world.get::<Virtual>(e).is_some() {
                if let Some(active) = world.get::<ActiveTransition>(e) {
                    let raw_t = if active.delay_remaining > 0.0 {
                        0.0
                    } else {
                        (active.elapsed / active.config.duration).min(1.0)
                    };
                    let eased_t = (active.config.easing)(raw_t);
                    text_inst.opacity = 1.0 - eased_t;
                }
            }

            out.push(text_inst);
        }
    }
    out
}
