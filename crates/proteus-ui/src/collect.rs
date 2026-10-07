//! Turns the ECS world into the list of quads to draw.
//!
//! [`collect_instances`] reads every visible entity and returns a
//! `Vec<QuadInstance>`, which the renderer uploads with
//! `QuadPipeline::upload_instances` and draws.
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
//! An entity without `BakedText` emits only the background. A virtual piece of a
//! split or merge may carry its source's `BakedText`; its text fades out as the
//! transition progresses, rather than vanishing at the end.
//!
//! ## Drop shadow
//!
//! A [`DropShadow`] applies to the background instance only, so the shadow isn't
//! drawn twice beneath the text.
//!
//! ## Glow
//!
//! A [`Glow`] uses the same shadow fields with no offset, which gives an even
//! halo. If an entity has both, the drop shadow is drawn.
//!
//! ## Border
//!
//! A [`Border`]'s fields are copied into the instance's border fields. It is
//! independent of shadows and glows, so an entity can have all three.
//!
//! ## Fading between two bakes
//!
//! A [`BakedTexture`] gives the background two images in the transition atlas,
//! and `crossfade_t`, from the entity's [`ActiveTransition`], fades from one to
//! the other. Pieces of a split or merge use it to fade from a slice of the
//! source's appearance to the target's, including shape, border and text.
//!
//! ## Video
//!
//! A [`crate::VideoPlayer`] makes the background show the whole video
//! texture. `QuadState::color` tints it; use `Vec4::ONE` for none. Text is still
//! drawn on top.
//!
//! ## Image
//!
//! A [`crate::BakedImage`] makes the background show its region of the main
//! atlas, on its page. `QuadState::color` tints it; use `Vec4::ONE` for none.
//!
//! An entity with both an image and video fades between them with
//! [`crate::VideoCrossfade`]. The two are in different atlases, so without it
//! the video would be sampled through the image's coordinates and show only a
//! fragment.
//!
//! ## Baked components
//!
//! A [`crate::bake::BakedComposite`] makes the background show the baked
//! component, like an image. Baking also removed the entity's own color,
//! corner radius, border, glow and shadow, so nothing is drawn twice.
//!
//! ## Visibility
//!
//! Hidden entities are left out. An entity with no `Visibility` is visible.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use glam::Vec4;

use proteus_render::{
    pack_atlas_page, QuadInstance, QuadPipeline, ATLAS_SELECTOR_MAIN, ATLAS_SELECTOR_TRANSITION,
    ATLAS_SELECTOR_VIDEO,
};

use crate::{
    bake::BakedComposite,
    effects::{Border, DropShadow, Glow},
    hierarchy::{compose_with_parent, EffectiveOpacity, EffectiveVisibility, Opacity},
    spawn_order::SpawnOrder,
    video::{VideoCrossfade, VideoPlayer},
    ActiveTransition, BakedImage, BakedText, QuadState, Text, Virtual, Visibility,
};

// ---------------------------------------------------------------------------
// BakedTexture
// ---------------------------------------------------------------------------

/// The two images a virtual piece of a split or merge fades between, as
/// regions of the transition atlas.
///
/// For a split, `from_*` is the piece's slice of the source's bake, and `to_*`
/// is its target's whole bake; a merge is the reverse. The piece fades from one
/// to the other as its transition progresses.
///
/// `own_alloc` is the bake that belongs to this piece alone, freed when it is
/// removed. The bake all the pieces share is
/// `ActiveGroupTransition::shared_alloc`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BakedTexture {
    /// Texture coordinates of the start image's top-left.
    pub from_uv_offset: [f32; 2],
    /// Size of the start image, in texture coordinates.
    pub from_uv_scale: [f32; 2],
    /// Texture coordinates of the end image's top-left.
    pub to_uv_offset: [f32; 2],
    /// Size of the end image, in texture coordinates.
    pub to_uv_scale: [f32; 2],
    /// The bake that belongs to this piece alone, freed when it is removed.
    pub own_alloc: proteus_render::TransitionAllocId,
}

// ---------------------------------------------------------------------------
// quad_state_to_instance
// ---------------------------------------------------------------------------

/// Converts a [`QuadState`] and its optional text, drop shadow, glow and border
/// into a [`QuadInstance`].
///
/// - Without `baked`, the quad samples the atlas's white pixel, so it draws as
///   a solid color. With it, the quad draws the text.
/// - A `shadow` fills the shadow fields, and `glow` is then ignored.
/// - Without a `shadow`, a `glow` fills the same fields with no offset, an even
///   halo in `Glow::color`.
/// - A `border` fills the border fields, independent of the others.
///
/// For an entity with text, it is called twice: once for the background, with
/// its shadow or glow, and once for the text, with neither.
pub fn quad_state_to_instance(
    qs: &QuadState,
    baked: Option<&BakedText>,
    shadow: Option<&DropShadow>,
    glow: Option<&Glow>,
    border: Option<&Border>,
) -> QuadInstance {
    let (uv_offset, uv_scale, atlas_page) = match baked {
        Some(b) => (
            b.uv_offset,
            b.uv_scale,
            pack_atlas_page(ATLAS_SELECTOR_MAIN, b.page),
        ),
        None => (
            QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            QuadPipeline::WHITE_PIXEL_UV_SCALE,
            // The white pixel is on page 0 of the main atlas only.
            pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
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
                // Clamp to 0..=1: alpha above 1 would invert blending.
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
        atlas_page,
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        // The transition atlas, where a `BakedTexture`'s start image is.
        // `push_entity_instances` changes it for fading between an image and
        // video.
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
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

/// Appends one entity's instances to `out`: its background, and its text if it
/// has any.
fn push_entity_instances(world: &World, e: Entity, qs: &QuadState, out: &mut Vec<QuadInstance>) {
    // The cascaded opacity if computed, else the entity's own, for tests that
    // skip the schedule, else opaque.
    let effective_opacity = world
        .get::<EffectiveOpacity>(e)
        .map(|o| o.0)
        .unwrap_or_else(|| world.get::<Opacity>(e).map(|o| o.0).unwrap_or(1.0));

    // The background, with the shadow or glow. A drop shadow wins over a
    // glow.
    let shadow = world.get::<DropShadow>(e);
    let glow = world.get::<Glow>(e);
    let border = world.get::<Border>(e);
    let mut bg_inst = quad_state_to_instance(qs, None, shadow, glow, border);
    bg_inst.opacity = effective_opacity;
    // Video, an image, or both. With both, `VideoCrossfade::video_t` blends
    // them, from the image at 0 to the video at 1. The app sets `video_t`,
    // since only it knows which way it is fading.
    match (
        world.get::<VideoPlayer>(e).is_some(),
        world.get::<BakedImage>(e),
    ) {
        (true, Some(image)) => {
            let video_t = world
                .get::<VideoCrossfade>(e)
                .map(|c| c.video_t.clamp(0.0, 1.0))
                .unwrap_or(1.0); // no VideoCrossfade: only the video
            if video_t >= 0.9999 {
                bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_VIDEO, 0);
                bg_inst.uv_offset = [0.0, 0.0];
                bg_inst.uv_scale = [1.0, 1.0];
            } else if video_t <= 0.0001 {
                bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_MAIN, image.page);
                bg_inst.uv_offset = image.uv_offset;
                bg_inst.uv_scale = image.uv_scale;
            } else {
                // End: the video. Start: the image.
                bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_VIDEO, 0);
                bg_inst.uv_offset = [0.0, 0.0];
                bg_inst.uv_scale = [1.0, 1.0];
                bg_inst.base_atlas_page = pack_atlas_page(ATLAS_SELECTOR_MAIN, image.page);
                bg_inst.base_uv_offset = image.uv_offset;
                bg_inst.base_uv_scale = image.uv_scale;
                bg_inst.crossfade_t = video_t;
            }
        }
        (true, None) => {
            bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_VIDEO, 0);
            bg_inst.uv_offset = [0.0, 0.0];
            bg_inst.uv_scale = [1.0, 1.0];
        }
        (false, Some(image)) => {
            // The image's region of the main atlas, on its page.
            bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_MAIN, image.page);
            bg_inst.uv_offset = image.uv_offset;
            bg_inst.uv_scale = image.uv_scale;
        }
        (false, None) => {}
    }
    // A baked component: its region of the main atlas, like an image. Baking
    // removed the entity's own color and effects, so nothing is drawn twice.
    if let Some(bc) = world.get::<BakedComposite>(e) {
        bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_MAIN, bc.page);
        bg_inst.uv_offset = bc.uv_offset;
        bg_inst.uv_scale = bc.uv_scale;
    }
    // A BakedTexture: fade from the start image to the end image with the
    // transition's eased progress. Never exactly 0, since the shader skips
    // fading entirely at 0 and would show the end image for a frame.
    if let Some(bt) = world.get::<BakedTexture>(e) {
        bg_inst.atlas_page = pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0); // transition_atlas — holds both baked snapshots
        bg_inst.uv_offset = bt.to_uv_offset;
        bg_inst.uv_scale = bt.to_uv_scale;
        bg_inst.base_uv_offset = bt.from_uv_offset;
        bg_inst.base_uv_scale = bt.from_uv_scale;
        bg_inst.crossfade_t = world
            .get::<ActiveTransition>(e)
            .map(|active| {
                // Clamped: an overshooting curve would otherwise fade past
                // the end image.
                active
                    .config
                    .easing
                    .apply(active.raw_t())
                    .clamp(0.0001, 1.0)
            })
            .unwrap_or(1.0); // no active transition — show the to-side fully
    }
    out.push(bg_inst);

    // The text, with no shadow or glow: the background already has them.
    if let Some(b) = world.get::<BakedText>(e) {
        let text_color = world.get::<Text>(e).map(|t| t.color).unwrap_or(Vec4::ONE);
        let mut text_qs = qs.clone();
        text_qs.color = text_color;
        // Size the text quad to the text, not to the whole entity.
        text_qs.size = b.pixel_size.into();
        // No corner radius: a round button's radius is larger than the text
        // quad, and would clip most of the text away.
        text_qs.corner_radius = 0.0;
        let mut text_inst = quad_state_to_instance(&text_qs, Some(b), None, None, None);
        text_inst.opacity = effective_opacity;

        // A virtual piece shows its source's text. Fade it out with the same
        // eased progress as the geometry, so it doesn't vanish at the end.
        if world.get::<Virtual>(e).is_some() {
            if let Some(active) = world.get::<ActiveTransition>(e) {
                let eased_t = active.config.easing.apply(active.raw_t()).clamp(0.0, 1.0);
                text_inst.opacity *= 1.0 - eased_t;
            }
        }

        out.push(text_inst);
    }
}

/// A top-level entity's data, read before drawing starts: its `QuadState`, its
/// own and cascaded visibility (`None` if absent), and its `SpawnOrder`.
type RootSnapshot = (Entity, QuadState, Option<bool>, Option<bool>, SpawnOrder);

/// Returns every visible entity as [`QuadInstance`]s, in drawing order.
///
/// Call it once per frame after the tick, and pass the result to
/// [`QuadPipeline::upload_instances`].
///
/// ## Drawing order
///
/// Each top-level entity (one without a [`ChildOf`]) is drawn, then its
/// children in [`Children`] order, depth first, so a child is always drawn over
/// its parent. Top-level entities are drawn in order of `QuadState::position.z`,
/// lowest first, and then of [`SpawnOrder`], earliest first. So among entities
/// with the same `z`, the one created last is on top.
///
/// The order is worked out explicitly rather than taken from how `bevy_ecs`
/// stores entities, which has no reliable relationship to creation order.
pub fn collect_instances(world: &mut World) -> Vec<QuadInstance> {
    // Top-level entities: a QuadState and no ChildOf. Read them all first, so
    // the query's borrow ends before the recursive walk reads the world.
    let mut roots: Vec<RootSnapshot> = {
        let mut q = world.query_filtered::<(
            Entity,
            &QuadState,
            Option<&Visibility>,
            Option<&EffectiveVisibility>,
            Option<&SpawnOrder>,
        ), Without<ChildOf>>();
        q.iter(world)
            .map(|(e, qs, vis, eff_vis, spawn_order)| {
                (
                    e,
                    qs.clone(),
                    vis.map(|v| v.visible),
                    eff_vis.map(|v| v.0),
                    // An entity without a SpawnOrder, such as one in a test
                    // world with no hooks, sorts last among equals.
                    spawn_order.copied().unwrap_or(SpawnOrder(u64::MAX)),
                )
            })
            .collect()
    };
    roots.sort_by(|a, b| {
        a.1.position
            .z
            .partial_cmp(&b.1.position.z)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.4.cmp(&b.4))
    });

    let mut out = Vec::new();
    for (e, local_qs, vis, eff_vis, _spawn_order) in roots {
        // A top-level entity's QuadState is already in world space.
        collect_subtree(world, e, &local_qs, vis, eff_vis, false, &mut out);
    }
    out
}

/// Appends `entity`'s instances, then each child's in [`Children`] order, with
/// each child placed in the world by [`compose_with_parent`].
fn collect_subtree(
    world: &World,
    entity: Entity,
    world_qs: &QuadState,
    vis: Option<bool>,
    eff_vis: Option<bool>,
    force_visible: bool,
    out: &mut Vec<QuadInstance>,
) {
    // The cascaded visibility if computed, else the entity's own, for tests
    // that skip the schedule, else visible.
    let visible = force_visible || eff_vis.unwrap_or_else(|| vis.unwrap_or(true));
    if visible {
        push_entity_instances(world, entity, world_qs, out);
    }

    let Some(children) = world.get::<Children>(entity) else {
        return;
    };
    for child in children.iter() {
        let Some(child_local) = world.get::<QuadState>(child) else {
            continue;
        };
        let child_world = compose_with_parent(world_qs, child_local);
        let child_vis = world.get::<Visibility>(child).map(|v| v.visible);
        let child_eff_vis = world.get::<EffectiveVisibility>(child).map(|v| v.0);
        collect_subtree(
            world,
            child,
            &child_world,
            child_vis,
            child_eff_vis,
            force_visible,
            out,
        );
    }
}
