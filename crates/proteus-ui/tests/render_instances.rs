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

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use proteus_render::QuadPipeline;
use proteus_ui::{
    collect_instances, linear, transition::TransitionConfig, BakedText, ProteusWorld, QuadState,
    Text, TransitionRequest, Visibility,
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
