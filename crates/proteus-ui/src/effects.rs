//! Visual effect components for M8 and beyond.
//!
//! Each effect is an optional ECS component that the render system reads when
//! collecting instance data.  Effects whose controlling alpha / weight is 0 are
//! free in the fragment shader — the branch is skipped without sampling or
//! computing anything.
//!
//! ## M8 — Drop Shadow
//!
//! A soft drop shadow rendered entirely in the fragment shader via the existing
//! `sdf_rounded_rect` function.  The shadow shape mirrors the component's
//! rounded rectangle (including its corner radius), offset by `offset`, expanded
//! by `spread`, and blurred by `softness`.
//!
//! The vertex shader inflates the quad geometry so that shadow pixels that fall
//! *outside* the component's normal bounds are still rasterized.  No extra render
//! passes or atlas allocations are required.
//!
//! ## Future effects (M8.5 / M8.6)
//!
//! Blur (M8.5) and Glow (M8.6) require an offscreen bake pass and will be
//! defined in separate types here.

use bevy_ecs::prelude::Component;
use glam::{Vec2, Vec4};

// ---------------------------------------------------------------------------
// DropShadow
// ---------------------------------------------------------------------------

/// Soft drop shadow rendered via SDF in the fragment shader.
///
/// Attach this component to any entity that has a [`crate::QuadState`] to give
/// it a drop shadow.  Remove the component to disable the shadow (zero runtime
/// cost — the shader branch is not entered when `shadow_color.a == 0`).
///
/// ## Coordinate system
///
/// `offset` is in **world-space** pixels (X right, Y up — same as
/// `QuadState::position`).  A shadow that appears below-right of the component
/// on screen has a positive X and a **negative** Y:
/// ```
/// use glam::Vec2;
/// use proteus_ui::DropShadow;
///
/// let shadow = DropShadow {
///     offset: Vec2::new(4.0, -4.0),  // 4 px right, 4 px down
///     ..DropShadow::default()
/// };
/// ```
///
/// ## Fields
///
/// - `offset`   — pixel displacement of the shadow center from the component center.
/// - `color`    — RGBA shadow color.  `alpha == 0.0` is a no-op (no shadow rendered).
/// - `softness` — penumbra radius in pixels.  Larger = softer edge.  Minimum 0.5.
/// - `spread`   — uniform expansion of the shadow shape before blurring.
///   Positive values make the shadow larger than the component.
#[derive(Component, Clone, Debug)]
pub struct DropShadow {
    /// Shadow displacement in world-space pixels (X right, Y up).
    pub offset: Vec2,
    /// Shadow color and opacity.  `alpha == 0.0` completely disables the shadow.
    pub color: Vec4,
    /// Penumbra softness in pixels.  Values below 0.5 are clamped in the shader.
    pub softness: f32,
    /// Uniform shape expansion in pixels applied before softening.
    pub spread: f32,
}

impl Default for DropShadow {
    /// A subtle semi-transparent black shadow offset 4 px right and 4 px down
    /// (Y-up: `offset.y = -4.0`), with an 8 px soft edge.
    fn default() -> Self {
        Self {
            offset: Vec2::new(4.0, -4.0),
            color: Vec4::new(0.0, 0.0, 0.0, 0.45),
            softness: 8.0,
            spread: 0.0,
        }
    }
}

impl DropShadow {
    /// Compact factory: specify just the offset and softness; color defaults to
    /// semi-transparent black and spread to 0.
    pub fn new(offset: Vec2, softness: f32) -> Self {
        Self {
            offset,
            softness,
            ..Self::default()
        }
    }
}
