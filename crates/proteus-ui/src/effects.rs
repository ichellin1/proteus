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
//! ## M8.6 — Glow
//!
//! A soft radial halo/glow that emanates outward from the component boundary.
//! Glow reuses the same `shadow_params`/`shadow_color` instance slots as Drop
//! Shadow by encoding a zero offset — the shader's existing SDF shadow branch
//! produces a symmetric halo when both offset components are zero.  No new
//! vertex attributes, pipeline changes, or shader changes are required.
//!
//! The halo `color` is set explicitly by the caller and is independent of the
//! entity's `QuadState::color` — this allows the glow to be a different hue or
//! intensity than the component's fill, and works correctly for textured or
//! non-solid components.
//!
//! If both [`DropShadow`] and [`Glow`] are attached to the same entity,
//! `DropShadow` takes precedence and `Glow` is silently ignored.

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

// ---------------------------------------------------------------------------
// Glow
// ---------------------------------------------------------------------------

/// Soft radial glow/halo emanating outward from the component boundary.
///
/// Attach this component to any entity that has a [`crate::QuadState`] to give
/// it a glow effect.  Remove the component to disable the glow (zero runtime
/// cost when absent).
///
/// The halo `color` is set explicitly and is independent of the entity's fill
/// color — this allows a different hue or opacity and works correctly for
/// textured or non-solid components.
///
/// Glow reuses the `shadow_params`/`shadow_color` instance slots that
/// [`DropShadow`] uses, encoding a zero offset so the SDF shadow branch in the
/// fragment shader produces a symmetric halo instead of a directional shadow.
/// No new vertex attributes, pipeline changes, or shader changes are required.
///
/// **Mutual exclusivity:** if both [`DropShadow`] and [`Glow`] are present on
/// the same entity, `DropShadow` takes precedence and `Glow` is ignored.
///
/// ## Fields
///
/// - `radius`    — halo spread in pixels.  Controls the Gaussian sigma
///   (sigma ≈ radius / 3).  Minimum useful value is ~4.0.
/// - `color`     — RGBA halo color.  Set this independently of the entity's
///   fill color.  `alpha == 0.0` disables the glow.
/// - `intensity` — multiplier applied to `color.a` before upload.  Effective
///   alpha = `color.a * intensity`.  Values > 1.0 are clamped in the shader.
#[derive(Component, Clone, Debug)]
pub struct Glow {
    /// Halo spread in pixels.
    pub radius: f32,
    /// Halo color and base opacity, independent of the entity's fill color.
    pub color: Vec4,
    /// Opacity multiplier (effective alpha = `color.a * intensity`).
    pub intensity: f32,
}

impl Default for Glow {
    /// A soft white halo with 12 px radius and 70 % intensity.
    fn default() -> Self {
        Self {
            radius: 12.0,
            color: Vec4::new(1.0, 1.0, 1.0, 0.8),
            intensity: 0.7,
        }
    }
}

impl Glow {
    /// Compact factory: specify the radius and color; intensity defaults to 0.7.
    pub fn new(radius: f32, color: Vec4) -> Self {
        Self {
            radius,
            color,
            ..Self::default()
        }
    }
}
