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
//! ## Border
//!
//! When a [`Border`] component is present its `width`/`color`/`offset` are copied
//! directly into the instance's `border_width`/`border_color`/`border_offset`
//! fields — independent of shadow/glow, so all three can be present at once.
//!
//! ## Baked texture crossfade (two-sided)
//!
//! When a [`BakedTexture`] component is present, its `from_*` fields become
//! the background instance's `base_uv_offset`/`base_uv_scale` and its `to_*`
//! fields become `uv_offset`/`uv_scale` (with `atlas_page` forced to `1` —
//! `transition_atlas` holds both sides). `crossfade_t` is driven from the
//! entity's own [`ActiveTransition`] progress each frame. Used by
//! transition-bake virtual slices (see `topology`) so a slice crossfades
//! texel-for-texel from an actual cropped snapshot of the source entity's
//! rendered appearance to an actual snapshot of its target's rendered
//! appearance — shape, border, *and* text on both ends — rather than a
//! flat-color approximation on either side.
//!
//! ## Video (M9)
//!
//! When a [`crate::VideoPlayer`] component is present the background instance's
//! `atlas_page` is set to `2` and the UV is set to cover the full `video_atlas`
//! texture.  The entity's `QuadState::color` acts as a tint; use `Vec4::ONE`
//! (white) for unfiltered video.  Any `BakedText` overlay is still emitted on
//! top as a second instance so labels can float above the video.
//!
//! ## Static image (M9.7)
//!
//! When a [`crate::BakedImage`] component is present, its `uv_offset`/`uv_scale`
//! (into `main_atlas`, `atlas_page` 0 — the default, so no page switch needed)
//! replace the background instance's white-pixel-sentinel UV. Unlike video/
//! baked-texture, this is a one-time static mapping — no per-frame recompute.
//! `QuadState::color` still tints the image; use `Vec4::ONE` for unfiltered.
//!
//! **When both are present on the same entity** (e.g. a tile carrying its
//! permanent box-cover `BakedImage` that also gets a `VideoPlayer` once
//! activated), see [`crate::VideoCrossfade`] (M9.8) — it drives a live blend
//! between the two rather than either flatly overriding the other. Their
//! UVs point at different atlases (`main_atlas` vs `video_atlas`), so naively
//! applying both without going through `VideoCrossfade` is a bug, not an
//! effect: the video texture gets sampled through a UV window sized for the
//! image, showing a small, blown-up fragment instead of the full frame.
//!
//! ## Visibility
//!
//! Entities with [`Visibility::HIDDEN`] are excluded from the output. Entities with
//! no `Visibility` component at all are treated as visible (the default).

use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{QuadInstance, QuadPipeline};

use crate::{
    effects::{Border, DropShadow, Glow},
    video::{VideoCrossfade, VideoPlayer},
    ActiveTransition, BakedImage, BakedText, QuadState, Text, Virtual, Visibility,
};

// ---------------------------------------------------------------------------
// BakedTexture
// ---------------------------------------------------------------------------

/// A two-sided background-instance crossfade: UV regions within
/// `transition_atlas` holding baked snapshots of *both* a virtual slice's
/// origin and its destination appearance.
///
/// Used by transition-bake virtual slices (see `topology::one_to_n_setup_system`
/// / `n_to_one_setup_system`): the source entity is baked once and sliced into
/// N thirds (the `from_*` side, different per slice), and each target entity is
/// baked once in full (the `to_*` side, one whole snapshot per slice — shape,
/// border, *and* text). Each frame, [`push_entity_instances`] reads the
/// entity's own [`ActiveTransition`] to compute `crossfade_t`, so the slice
/// crossfades texel-for-texel from the source's cropped appearance to the
/// target's real appearance — not just geometry, and not a flat-color
/// approximation on either end.
///
/// `own_alloc` is whichever side of the bake is unique to this specific
/// virtual (the target bake for 1→N, the source bake for N→1) — freed when
/// this virtual despawns. The side shared across all virtuals in the group
/// (the one common source bake for 1→N, the one common destination bake for
/// N→1) is *not* stored here — see `ActiveGroupTransition::shared_alloc`,
/// freed once when the whole group completes.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BakedTexture {
    /// Normalised UV origin of the from-side, within `transition_atlas`.
    pub from_uv_offset: [f32; 2],
    /// Normalised UV extent of the from-side, within `transition_atlas`.
    pub from_uv_scale: [f32; 2],
    /// Normalised UV origin of the to-side, within `transition_atlas`.
    pub to_uv_offset: [f32; 2],
    /// Normalised UV extent of the to-side, within `transition_atlas`.
    pub to_uv_scale: [f32; 2],
    /// This virtual's own bake allocation — freed on despawn.
    pub own_alloc: proteus_render::TransitionAllocId,
}

// ---------------------------------------------------------------------------
// quad_state_to_instance
// ---------------------------------------------------------------------------

/// Convert a [`QuadState`], optional [`BakedText`], optional [`DropShadow`],
/// optional [`Glow`], and optional [`Border`] into a [`QuadInstance`].
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
/// - If `border` is `Some` its fields are copied straight into the instance's
///   border slots, independent of shadow/glow.
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
    border: Option<&Border>,
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
                // Clamp effective alpha to [0, 1]: values above 1.0 would invert
                // alpha blending in the shader (perceived negative transparency).
                [
                    g.color.x,
                    g.color.y,
                    g.color.z,
                    (g.color.w * g.intensity).min(1.0),
                ],
            ),
            None => ([0.0f32; 4], [0.0f32; 4]),
        },
    };

    let (border_width, border_color, border_offset) = match border {
        Some(b) => (b.width, b.color.to_array(), b.offset),
        None => (0.0, [0.0, 0.0, 0.0, 0.0], 0.0),
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
        // Matches every crossfade user before this field existed — the only
        // one is BakedTexture, whose from-side always lives in
        // transition_atlas. push_entity_instances overrides this for the
        // video+image live-crossfade case (see its VideoPlayer/BakedImage
        // handling below).
        base_atlas_page: 1,
        border_width,
        border_color,
        border_offset,
        shadow_params,
        shadow_color,
    }
}

// ---------------------------------------------------------------------------
// collect_instances
// ---------------------------------------------------------------------------

/// Append one entity's instance(s) — background, plus a text overlay if it
/// has baked glyph data — to `out`. Shared by [`collect_instances`] (which
/// already has `qs` from its batched query) and [`collect_entity_instances`]
/// (a single-entity convenience wrapper for callers outside the per-frame
/// render loop, e.g. baking a source entity's appearance for a transition).
fn push_entity_instances(world: &World, e: Entity, qs: &QuadState, out: &mut Vec<QuadInstance>) {
    // (a) Solid-color (or video/baked-texture) background quad — shadow or
    //     glow applied here. DropShadow takes precedence over Glow when both
    //     are present.
    let shadow = world.get::<DropShadow>(e);
    let glow = world.get::<Glow>(e);
    let border = world.get::<Border>(e);
    let mut bg_inst = quad_state_to_instance(qs, None, shadow, glow, border);
    // Video and static-image UV routing. When only one of VideoPlayer/
    // BakedImage is present it's a flat assignment (no blending). When both
    // are present (M9.8 — e.g. a tile with permanent box-cover art that also
    // gets a VideoPlayer once activated), VideoCrossfade's video_t drives a
    // live blend between them: the "to"/"from" sides swap depending on which
    // way video_t is headed, so playing forward (image → video) and reverse
    // (video → image) both use the same crossfade_t = video_t formula rather
    // than needing an inverted easing curve for one direction. The caller
    // (not collect_instances) decides video_t each frame — only it knows
    // whether a transition is running forward or backward.
    match (
        world.get::<VideoPlayer>(e).is_some(),
        world.get::<BakedImage>(e),
    ) {
        (true, Some(image)) => {
            let video_t = world
                .get::<VideoCrossfade>(e)
                .map(|c| c.video_t.clamp(0.0, 1.0))
                .unwrap_or(1.0); // no VideoCrossfade — show video fully, as before M9.8
            if video_t >= 0.9999 {
                bg_inst.atlas_page = 2;
                bg_inst.uv_offset = [0.0, 0.0];
                bg_inst.uv_scale = [1.0, 1.0];
            } else if video_t <= 0.0001 {
                bg_inst.uv_offset = image.uv_offset;
                bg_inst.uv_scale = image.uv_scale;
            } else {
                // to-side: video (video_atlas); from-side: box art (main_atlas).
                bg_inst.atlas_page = 2;
                bg_inst.uv_offset = [0.0, 0.0];
                bg_inst.uv_scale = [1.0, 1.0];
                bg_inst.base_atlas_page = 0;
                bg_inst.base_uv_offset = image.uv_offset;
                bg_inst.base_uv_scale = image.uv_scale;
                bg_inst.crossfade_t = video_t;
            }
        }
        (true, None) => {
            bg_inst.atlas_page = 2;
            bg_inst.uv_offset = [0.0, 0.0];
            bg_inst.uv_scale = [1.0, 1.0];
        }
        (false, Some(image)) => {
            // A static image is a one-time UV mapping into main_atlas
            // (atlas_page 0, already the default) — no per-frame recompute,
            // unlike video.
            bg_inst.uv_offset = image.uv_offset;
            bg_inst.uv_scale = image.uv_scale;
        }
        (false, None) => {}
    }
    // A BakedTexture drives a two-sided crossfade: base_uv/scale point at the
    // source's baked snapshot, uv/scale (atlas_page 1) point at the target's
    // own baked snapshot, and crossfade_t tracks this entity's own
    // ActiveTransition progress (mirrors the eased-t computation used for the
    // text-overlay fade below). Clamped to a tiny epsilon above zero rather
    // than allowed to land on exactly 0.0 — the shader skips the crossfade
    // branch entirely at crossfade_t == 0.0 (a zero-cost fast path when no
    // bake is in play), which would otherwise show the to-side for one frame
    // before any elapsed time has accumulated.
    if let Some(bt) = world.get::<BakedTexture>(e) {
        bg_inst.atlas_page = 1; // transition_atlas — holds both baked snapshots
        bg_inst.uv_offset = bt.to_uv_offset;
        bg_inst.uv_scale = bt.to_uv_scale;
        bg_inst.base_uv_offset = bt.from_uv_offset;
        bg_inst.base_uv_scale = bt.from_uv_scale;
        bg_inst.crossfade_t = world
            .get::<ActiveTransition>(e)
            .map(|active| {
                let raw_t = if active.delay_remaining > 0.0 {
                    0.0
                } else {
                    (active.elapsed / active.config.duration).min(1.0)
                };
                (active.config.easing)(raw_t).max(0.0001)
            })
            .unwrap_or(1.0); // no active transition — show the to-side fully
    }
    out.push(bg_inst);

    // (b) Text overlay — only for entities with baked glyph data.
    //     No shadow/glow on the overlay: it sits on top of the background (which
    //     already casts the shadow/glow), so doubling it would look wrong.
    if let Some(b) = world.get::<BakedText>(e) {
        let text_color = world.get::<Text>(e).map(|t| t.color).unwrap_or(Vec4::ONE);
        let mut text_qs = qs.clone();
        text_qs.color = text_color;
        // Size the overlay to the glyph run's actual footprint (centered on
        // the parent, since anchor is unchanged) rather than stretching it
        // to fill the parent's full geometry.
        text_qs.size = b.pixel_size.into();
        // The parent's corner_radius doesn't apply to the overlay — it was
        // harmless when the overlay matched the parent's size, but on the
        // overlay's now much smaller glyph-sized quad an inherited radius
        // (e.g. a circular button's) can exceed the quad's own half-size,
        // collapsing the rounded-rect SDF down to a sliver around the
        // center and clipping away most of the text.
        text_qs.corner_radius = 0.0;
        let mut text_inst = quad_state_to_instance(&text_qs, Some(b), None, None, None);

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
        push_entity_instances(world, e, &qs, &mut out);
    }
    out
}

/// Build the [`QuadInstance`]s for a single entity, ignoring [`Visibility`].
///
/// A convenience wrapper around the same per-entity logic [`collect_instances`]
/// uses, for callers that need one entity's instances outside the normal
/// per-frame render loop — e.g. baking a source entity's rendered appearance
/// into a texture before a Slice group transition (see
/// `QuadPipeline::bake_instances_to_main_atlas`). Returns an empty vec if the
/// entity has no [`QuadState`].
pub fn collect_entity_instances(world: &World, entity: Entity) -> Vec<QuadInstance> {
    let mut out = Vec::new();
    if let Some(qs) = world.get::<QuadState>(entity) {
        let qs = qs.clone();
        push_entity_instances(world, entity, &qs, &mut out);
    }
    out
}
