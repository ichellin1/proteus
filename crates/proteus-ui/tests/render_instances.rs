//! M6 visual regression tests — instance buffer approach.
//!
//! Instead of diffing pixel snapshots (which require a GPU and produce
//! non-deterministic output across drivers), these tests verify the
//! `Vec<QuadInstance>` that `collect_instances` produces from the ECS world.
//!
//! That buffer IS the ground truth for what appears on screen — the shader
//! only multiplies colors and samples a UV region, so if the instance data
//! is correct, the visual output is correct.
//!
//! ## Test matrix
//!
//! | Test | What it guards |
//! |---|---|
//! | `static_quad_produces_one_instance` | Basic position/size/color pass-through |
//! | `hidden_entity_produces_no_instance` | `Visibility::HIDDEN` filter |
//! | `no_visibility_defaults_to_visible` | Missing `Visibility` = visible |
//! | `text_entity_produces_two_instances` | Two-layer rendering model |
//! | `text_color_applied_to_overlay` | `Text::color` routes to overlay layer |
//! | `transition_lerps_at_t_half` | 1→1 lerp math at t = 0.5 (linear easing) |
//! | `shadow_params_populate_instance` | M8: DropShadow fields in background instance |
//! | `no_shadow_by_default` | M8: absence of DropShadow → all-zero shadow fields |
//! | `shadow_not_on_text_overlay` | M8: overlay layer never carries shadow data |
//! | `glow_params_populate_instance` | M8.6: Glow encodes zero-offset halo into shadow slots |
//! | `no_glow_by_default` | M8.6: absence of Glow → all-zero shadow fields |
//! | `shadow_wins_over_glow` | M8.6: DropShadow takes precedence over Glow |
//! | `border_params_populate_instance` | Border fields copy into instance border slots |
//! | `no_border_by_default` | absence of Border → all-zero border fields |
//! | `border_not_on_text_overlay` | overlay layer never carries border data |

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use proteus_render::{QuadPipeline, TransitionAtlasAllocator};
use proteus_ui::{
    collect_instances, linear, transition::TransitionConfig, ActiveTransition, BakedText,
    BakedTexture, Border, DropShadow, Glow, ProteusWorld, QuadState, Text, TransitionRequest,
    VideoPlayer, Visibility,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A sky-blue button quad — used as a stable fixture across tests.
fn sky_blue_button() -> QuadState {
    QuadState {
        position: Vec3::new(100.0, 200.0, 0.0),
        size: Vec2::new(120.0, 40.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.37, 0.65, 1.0, 1.0),
        corner_radius: 8.0,
    }
}

/// A gold detail quad — used as the transition target in lerp tests.
fn gold_detail() -> QuadState {
    QuadState {
        position: Vec3::new(300.0, 200.0, 0.0),
        size: Vec2::new(320.0, 200.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.85, 0.65, 0.13, 1.0),
        corner_radius: 12.0,
    }
}

/// A standard 1-second linear transition config.
fn linear_1s() -> TransitionConfig {
    TransitionConfig {
        duration: 1.0,
        delay: 0.0,
        easing: linear,
    }
}

/// Assert two f32 slices are within epsilon of each other.
fn assert_f32_slice_approx(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label}: slice length mismatch"
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert!((a - e).abs() < 1e-4, "{label}[{i}]: expected {e}, got {a}");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A single visible quad produces exactly one `QuadInstance` with the
/// correct position, size, and color.
#[test]
fn static_quad_produces_one_instance() {
    let mut world = World::new();
    world.spawn(sky_blue_button());

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1, "expected exactly one instance");
    assert_f32_slice_approx(&instances[0].position, &[100.0, 200.0, 0.0], "position");
    assert_f32_slice_approx(&instances[0].size, &[120.0, 40.0], "size");
    assert_f32_slice_approx(&instances[0].color, &[0.37, 0.65, 1.0, 1.0], "color");
    // Solid quad uses the white-pixel sentinel UV.
    assert_eq!(instances[0].uv_offset, QuadPipeline::WHITE_PIXEL_UV_OFFSET);
    assert_eq!(instances[0].uv_scale, QuadPipeline::WHITE_PIXEL_UV_SCALE);
}

/// Entities with `Visibility::HIDDEN` must not appear in the instance buffer.
#[test]
fn hidden_entity_produces_no_instance() {
    let mut world = World::new();
    world.spawn((sky_blue_button(), Visibility::HIDDEN));

    let instances = collect_instances(&mut world);

    assert_eq!(
        instances.len(),
        0,
        "hidden entity should produce no instances"
    );
}

/// Entities with no `Visibility` component at all are treated as visible.
/// This is the default — Virtual entities have no Visibility and must render.
#[test]
fn no_visibility_defaults_to_visible() {
    let mut world = World::new();
    world.spawn(sky_blue_button()); // no Visibility component

    let instances = collect_instances(&mut world);

    assert_eq!(
        instances.len(),
        1,
        "entity without Visibility should default to visible"
    );
}

/// A text entity produces two instances: a solid-color background layer
/// (WHITE_PIXEL_UV) and a text overlay layer (BakedText UV).
#[test]
fn text_entity_produces_two_instances() {
    let baked = BakedText {
        uv_offset: [0.10, 0.20],
        uv_scale: [0.05, 0.01],
        pixel_size: [60.0, 24.0],
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), baked, Text::new("Elephant", 22.0)));

    let instances = collect_instances(&mut world);

    assert_eq!(
        instances.len(),
        2,
        "text entity should produce two instances"
    );

    // Layer 0: solid-color background.
    assert_eq!(
        instances[0].uv_offset,
        QuadPipeline::WHITE_PIXEL_UV_OFFSET,
        "background layer must use white-pixel UV"
    );
    assert_eq!(
        instances[0].uv_scale,
        QuadPipeline::WHITE_PIXEL_UV_SCALE,
        "background layer must use white-pixel UV scale"
    );
    assert_f32_slice_approx(
        &instances[0].color,
        &[0.37, 0.65, 1.0, 1.0],
        "background color",
    );

    // Layer 1: text overlay.
    assert_eq!(
        instances[1].uv_offset,
        [0.10, 0.20],
        "text overlay must use BakedText UV offset"
    );
    assert_eq!(
        instances[1].uv_scale,
        [0.05, 0.01],
        "text overlay must use BakedText UV scale"
    );
}

/// `Text::color` is applied to the overlay instance, not the background.
/// Default text color is opaque white (Vec4::ONE).
#[test]
fn text_color_applied_to_overlay() {
    let dark = Vec4::new(0.1, 0.1, 0.1, 1.0);
    let baked = BakedText {
        uv_offset: [0.0, 0.0],
        uv_scale: [0.1, 0.02],
        pixel_size: [60.0, 24.0],
    };

    let mut world = World::new();
    world.spawn((
        sky_blue_button(),
        baked,
        Text::new("Elephant", 22.0).with_color(dark),
    ));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 2);
    // Background keeps the quad's color.
    assert_f32_slice_approx(
        &instances[0].color,
        &[0.37, 0.65, 1.0, 1.0],
        "background color",
    );
    // Overlay uses Text::color.
    assert_f32_slice_approx(&instances[1].color, &[0.1, 0.1, 0.1, 1.0], "overlay color");
}

// ---------------------------------------------------------------------------
// M8 drop shadow tests
// ---------------------------------------------------------------------------

/// An entity with a `DropShadow` component has the shadow fields populated in
/// its (background) instance.
#[test]
fn shadow_params_populate_instance() {
    use glam::{Vec2, Vec4};

    let shadow = DropShadow {
        offset: Vec2::new(6.0, -6.0),
        color: Vec4::new(0.0, 0.0, 0.0, 0.5),
        softness: 10.0,
        spread: 2.0,
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), shadow));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[6.0, -6.0, 10.0, 2.0],
        "shadow_params",
    );
    assert_f32_slice_approx(
        &instances[0].shadow_color,
        &[0.0, 0.0, 0.0, 0.5],
        "shadow_color",
    );
}

/// An entity without a `DropShadow` component has all-zero shadow fields,
/// meaning the shader skips the shadow branch entirely.
#[test]
fn no_shadow_by_default() {
    let mut world = World::new();
    world.spawn(sky_blue_button()); // no DropShadow

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    // shadow_color.a == 0 → shader skips shadow
    assert_f32_slice_approx(
        &instances[0].shadow_color,
        &[0.0, 0.0, 0.0, 0.0],
        "shadow_color should be all-zero without DropShadow",
    );
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[0.0, 0.0, 0.0, 0.0],
        "shadow_params should be all-zero without DropShadow",
    );
}

/// The text overlay instance (layer 1) must never carry shadow data, even when
/// the entity has a `DropShadow` component.  The background layer (layer 0)
/// already casts the shadow; duplicating it on the overlay would render it twice.
#[test]
fn shadow_not_on_text_overlay() {
    use glam::Vec2;

    let shadow = DropShadow::new(Vec2::new(4.0, -4.0), 8.0);
    let baked = BakedText {
        uv_offset: [0.1, 0.2],
        uv_scale: [0.05, 0.01],
        pixel_size: [60.0, 24.0],
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), baked, Text::new("Hi", 22.0), shadow));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 2, "text entity must emit two instances");

    // Background (layer 0): shadow present.
    assert!(
        instances[0].shadow_color[3] > 0.0,
        "background layer should have shadow alpha > 0"
    );

    // Overlay (layer 1): shadow must be zeroed.
    assert_f32_slice_approx(
        &instances[1].shadow_color,
        &[0.0, 0.0, 0.0, 0.0],
        "text overlay shadow_color must be zero",
    );
    assert_f32_slice_approx(
        &instances[1].shadow_params,
        &[0.0, 0.0, 0.0, 0.0],
        "text overlay shadow_params must be zero",
    );
}

/// A 1→1 transition with linear easing at t = 0.5 produces an instance
/// whose position and size are the midpoints of the from and to states.
///
/// Frame 0 (dt=0.0): `TransitionSetup` queues `ActiveTransition` via commands.
/// Frame 1 (dt=0.5): `FlushCommands` applies it; `TransitionTick` advances t to 0.5.
#[test]
fn transition_lerps_at_t_half() {
    let from = sky_blue_button(); // position.x = 100, size.x = 120
    let to = gold_detail(); // position.x = 300, size.x = 320

    let mut ui_world = ProteusWorld::new();
    let e = ui_world.world.spawn(from.clone()).id();
    ui_world.world.entity_mut(e).insert(TransitionRequest {
        to: to.clone(),
        from_state: None,
        config: linear_1s(),
    });

    // Frame 0: TransitionSetup detects the request and queues ActiveTransition.
    // dt=0.0 so TransitionTick doesn't advance even if it ran.
    ui_world.update(0.0);

    // Frame 1: FlushCommands applies ActiveTransition; TransitionTick runs with dt=0.5.
    ui_world.update(0.5);

    let instances = collect_instances(&mut ui_world.world);

    assert_eq!(instances.len(), 1);

    // position.x: lerp(100, 300, 0.5) = 200
    assert_f32_slice_approx(
        &instances[0].position,
        &[200.0, 200.0, 0.0],
        "mid-transition position",
    );
    // size.x: lerp(120, 320, 0.5) = 220
    assert_f32_slice_approx(&instances[0].size, &[220.0, 120.0], "mid-transition size");
}

// ---------------------------------------------------------------------------
// M8.6 glow tests
// ---------------------------------------------------------------------------

/// An entity with a [`Glow`] component has the glow encoded into the shadow
/// slots of the background instance.  The offset fields are zero (producing a
/// symmetric halo) and the effective alpha is `color.a * intensity`.
#[test]
fn glow_params_populate_instance() {
    let glow = Glow {
        radius: 12.0,
        color: Vec4::new(0.37, 0.65, 1.0, 1.0),
        intensity: 0.7,
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), glow));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    // Offset must be (0, 0) — symmetric halo, not a directional shadow.
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[0.0, 0.0, 12.0, 0.0],
        "shadow_params for glow (zero offset, radius, zero spread)",
    );
    // Effective alpha = color.a * intensity = 1.0 * 0.7 = 0.7.
    assert_f32_slice_approx(
        &instances[0].shadow_color,
        &[0.37, 0.65, 1.0, 0.7],
        "shadow_color for glow (RGB from Glow::color, A = color.a * intensity)",
    );
}

/// An entity without a [`Glow`] component (and without a [`DropShadow`]) has
/// all-zero shadow fields.  The shader's `shadow_color.a == 0` branch is
/// skipped, so there is no glow or shadow at zero runtime cost.
#[test]
fn no_glow_by_default() {
    let mut world = World::new();
    world.spawn(sky_blue_button()); // no Glow, no DropShadow

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert_f32_slice_approx(
        &instances[0].shadow_color,
        &[0.0, 0.0, 0.0, 0.0],
        "shadow_color must be all-zero when neither Glow nor DropShadow is present",
    );
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[0.0, 0.0, 0.0, 0.0],
        "shadow_params must be all-zero when neither Glow nor DropShadow is present",
    );
}

// ---------------------------------------------------------------------------
// M9 VideoPlayer tests
// ---------------------------------------------------------------------------

/// A `VideoPlayer` entity must have its background instance routed to
/// `atlas_page = 2` (the `video_atlas` binding) with full-coverage UV mapping.
/// This is the branch in `collect_instances` that had zero prior test coverage.
#[test]
fn video_player_sets_atlas_page_2() {
    let mut world = World::new();
    world.spawn((sky_blue_button(), VideoPlayer));

    let instances = collect_instances(&mut world);

    assert_eq!(
        instances.len(),
        1,
        "VideoPlayer emits one background instance"
    );
    assert_eq!(
        instances[0].atlas_page, 2,
        "VideoPlayer must route to atlas_page 2 (video_atlas)"
    );
    assert_eq!(
        instances[0].uv_offset,
        [0.0, 0.0],
        "VideoPlayer uv_offset must be [0, 0] — full texture, no atlas sub-region"
    );
    assert_eq!(
        instances[0].uv_scale,
        [1.0, 1.0],
        "VideoPlayer uv_scale must be [1, 1] — full texture coverage"
    );
}

/// A `VideoPlayer` entity with `Glow` must emit both the atlas_page=2 routing
/// *and* a correctly encoded glow in the same instance.  This guards the path
/// where both the video branch and the shadow/glow branch are active at once —
/// the combination that would have exposed the UV inflation distortion.
#[test]
fn video_player_with_glow_has_atlas_page_and_glow_params() {
    let glow = Glow {
        radius: 15.0,
        color: Vec4::new(0.60, 0.65, 1.00, 1.0),
        intensity: 0.7,
    };

    let mut world = World::new();
    world.spawn((
        QuadState {
            color: Vec4::ONE, // white = unfiltered video
            ..sky_blue_button()
        },
        VideoPlayer,
        glow,
    ));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    // Video routing.
    assert_eq!(instances[0].atlas_page, 2, "must route to video_atlas");
    assert_eq!(instances[0].uv_offset, [0.0, 0.0]);
    assert_eq!(instances[0].uv_scale, [1.0, 1.0]);
    // Glow encoded into shadow slots.
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[0.0, 0.0, 15.0, 0.0],
        "shadow_params: zero offset, radius in softness slot, zero spread",
    );
    assert!(
        instances[0].shadow_color[3] > 0.0,
        "shadow_color.a must be > 0 to activate the glow branch in the shader"
    );
}

/// `Glow::intensity > 1.0` must be clamped to 1.0 before the instance is
/// emitted.  An effective alpha above 1.0 inverts the alpha-blending equation
/// in the shader, producing visible negative-transparency artefacts.
#[test]
fn glow_intensity_above_one_is_clamped() {
    let glow = Glow {
        radius: 12.0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        intensity: 3.0, // 1.0 * 3.0 = 3.0 without the clamp
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), glow));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert_eq!(
        instances[0].shadow_color[3], 1.0,
        "effective alpha must be clamped to 1.0 (was 3.0 before the fix)"
    );
}

// ---------------------------------------------------------------------------

/// When both [`DropShadow`] and [`Glow`] are present on the same entity,
/// `DropShadow` takes precedence and `Glow` is ignored.
#[test]
fn shadow_wins_over_glow() {
    use glam::Vec2;

    let shadow = DropShadow {
        offset: Vec2::new(4.0, -4.0),
        color: Vec4::new(0.0, 0.0, 0.0, 0.45),
        softness: 8.0,
        spread: 0.0,
    };
    // Glow with a large radius and red color — if it were used, shadow_params[2]
    // would be 20.0 (not 8.0) and shadow_color would be red (not black).
    let glow = Glow {
        radius: 20.0,
        color: Vec4::new(1.0, 0.0, 0.0, 1.0),
        intensity: 1.0,
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), shadow, glow));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    // Shadow offset must be (4.0, -4.0) — not glow's (0, 0, 20, 0).
    assert_f32_slice_approx(
        &instances[0].shadow_params,
        &[4.0, -4.0, 8.0, 0.0],
        "shadow_params must come from DropShadow, not Glow",
    );
    // Shadow color must be the drop shadow color (opaque black), not
    // the entity color that Glow would have used.
    assert_f32_slice_approx(
        &instances[0].shadow_color,
        &[0.0, 0.0, 0.0, 0.45],
        "shadow_color must come from DropShadow, not Glow",
    );
}

// ---------------------------------------------------------------------------
// Border
// ---------------------------------------------------------------------------

#[test]
fn border_params_populate_instance() {
    let border = Border {
        width: 5.0,
        color: Vec4::new(0.68, 0.85, 0.90, 1.0),
        offset: -1.0,
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), border));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert!(
        (instances[0].border_width - 5.0).abs() < 1e-5,
        "border_width"
    );
    assert_f32_slice_approx(
        &instances[0].border_color,
        &[0.68, 0.85, 0.90, 1.0],
        "border_color",
    );
    assert!(
        (instances[0].border_offset - (-1.0)).abs() < 1e-5,
        "border_offset"
    );
}

#[test]
fn no_border_by_default() {
    let mut world = World::new();
    world.spawn(sky_blue_button()); // no Border

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].border_width, 0.0, "border_width should be 0");
    assert_f32_slice_approx(
        &instances[0].border_color,
        &[0.0, 0.0, 0.0, 0.0],
        "border_color should be all-zero without Border",
    );
    assert_eq!(instances[0].border_offset, 0.0, "border_offset should be 0");
}

/// The text overlay instance (layer 1) must never carry border data, even when
/// the entity has a `Border` component — the background layer already draws
/// the border; duplicating it on the overlay would render it twice.
#[test]
fn border_not_on_text_overlay() {
    let border = Border {
        width: 5.0,
        color: Vec4::new(0.68, 0.85, 0.90, 1.0),
        offset: -1.0,
    };
    let baked = BakedText {
        uv_offset: [0.1, 0.2],
        uv_scale: [0.05, 0.01],
        pixel_size: [60.0, 24.0],
    };

    let mut world = World::new();
    world.spawn((sky_blue_button(), baked, Text::new("START", 18.0), border));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 2, "background + text overlay");
    assert_eq!(
        instances[1].border_width, 0.0,
        "text overlay must not carry border data"
    );
}

// ---------------------------------------------------------------------------
// BakedTexture (two-sided crossfade)
// ---------------------------------------------------------------------------

/// A valid `TransitionAllocId` for test fixtures — `BakedTexture::own_alloc`
/// has no public constructor other than going through a real allocator (by
/// design: it's meant to always correspond to a live allocation). The
/// allocator itself is pure CPU bookkeeping, no GPU device needed.
fn fixture_alloc_id() -> proteus_render::TransitionAllocId {
    let mut allocator = TransitionAtlasAllocator::new(1024);
    allocator.allocate(64, 64).unwrap().0
}

fn baked_texture() -> BakedTexture {
    BakedTexture {
        from_uv_offset: [0.1, 0.2],
        from_uv_scale: [0.05, 0.05],
        to_uv_offset: [0.6, 0.7],
        to_uv_scale: [0.03, 0.03],
        own_alloc: fixture_alloc_id(),
    }
}

/// `BakedTexture` routes to the two crossfade UV pairs and forces
/// `atlas_page = 1` (`transition_atlas` — the only atlas the shader's
/// crossfade path reads `base_uv` from).
#[test]
fn baked_texture_populates_both_uv_sides() {
    let mut world = World::new();
    world.spawn((sky_blue_button(), baked_texture()));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].atlas_page, 1, "must sample transition_atlas");
    assert_f32_slice_approx(&instances[0].base_uv_offset, &[0.1, 0.2], "base_uv_offset");
    assert_f32_slice_approx(&instances[0].base_uv_scale, &[0.05, 0.05], "base_uv_scale");
    assert_f32_slice_approx(&instances[0].uv_offset, &[0.6, 0.7], "uv_offset (to-side)");
    assert_f32_slice_approx(&instances[0].uv_scale, &[0.03, 0.03], "uv_scale (to-side)");
}

/// With no `ActiveTransition` on the entity, `crossfade_t` defaults to 1.0 —
/// show the to-side fully (matches a virtual that already finished, or one
/// that's about to start with delay still pending).
#[test]
fn baked_texture_without_active_transition_shows_to_side_fully() {
    let mut world = World::new();
    world.spawn((sky_blue_button(), baked_texture()));

    let instances = collect_instances(&mut world);

    assert_eq!(instances[0].crossfade_t, 1.0);
}

/// `crossfade_t` tracks the entity's own `ActiveTransition` progress (eased),
/// matching the same computation `transition_tick_system` uses.
#[test]
fn baked_texture_crossfade_t_tracks_active_transition_progress() {
    let active = ActiveTransition::new(
        sky_blue_button(),
        gold_detail(),
        TransitionConfig {
            duration: 1.0,
            delay: 0.0,
            easing: linear,
        },
    );

    let mut world = World::new();
    world.spawn((sky_blue_button(), baked_texture(), active));

    let instances = collect_instances(&mut world);

    // Fresh ActiveTransition: elapsed = 0.0, so raw_t = 0.0 — but the shader
    // skips its crossfade branch entirely at exactly 0.0 (a zero-cost no-bake
    // fast path), so this is clamped to a tiny epsilon above zero rather than
    // landing on 0.0 exactly.
    assert!(
        instances[0].crossfade_t > 0.0 && instances[0].crossfade_t < 0.01,
        "expected a near-zero (but not exactly zero) crossfade_t, got {}",
        instances[0].crossfade_t,
    );
}

/// `crossfade_t` still respects the delay phase — burns delay first, exactly
/// like `transition_tick_system`'s own elapsed-time accounting.
#[test]
fn baked_texture_crossfade_t_is_near_zero_during_delay() {
    let mut active = ActiveTransition::new(
        sky_blue_button(),
        gold_detail(),
        TransitionConfig {
            duration: 1.0,
            delay: 5.0,
            easing: linear,
        },
    );
    active.delay_remaining = 5.0; // still fully within the delay window

    let mut world = World::new();
    world.spawn((sky_blue_button(), baked_texture(), active));

    let instances = collect_instances(&mut world);

    assert!(
        instances[0].crossfade_t < 0.01,
        "expected near-zero crossfade_t during delay, got {}",
        instances[0].crossfade_t,
    );
}

/// The text overlay instance (layer 1) must never carry `BakedTexture`
/// crossfade data — it isn't part of the baked snapshot's own crossfade;
/// only the background layer is.
#[test]
fn baked_texture_not_on_text_overlay() {
    let baked_text = BakedText {
        uv_offset: [0.1, 0.2],
        uv_scale: [0.05, 0.01],
        pixel_size: [60.0, 24.0],
    };

    let mut world = World::new();
    world.spawn((
        sky_blue_button(),
        baked_text,
        Text::new("START", 18.0),
        baked_texture(),
    ));

    let instances = collect_instances(&mut world);

    assert_eq!(instances.len(), 2, "background + text overlay");
    assert_eq!(
        instances[1].atlas_page, 0,
        "text overlay must keep sampling main_atlas, not transition_atlas"
    );
}
