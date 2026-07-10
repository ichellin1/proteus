//! ECS components for the metamorphic component model.
//!
//! Every visible element in Proteus is an ECS entity carrying these components.
//! The renderer reads them each frame to build the instance buffer.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

// ---------------------------------------------------------------------------
// QuadState — the visual geometry of one component
// ---------------------------------------------------------------------------

/// The complete geometric and visual state of one component quad.
///
/// This is what gets lerped during a transition. The renderer reads it
/// each frame to build a `QuadInstance` for the GPU instance buffer.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct QuadState {
    /// World-space position (x, y, z). Z is reserved for future depth sorting;
    /// draw order currently determines stacking (last uploaded = on top).
    pub position: Vec3,
    /// Size in pixels (width, height).
    pub size: Vec2,
    /// Rotation in radians.
    pub rotation: f32,
    /// Uniform scale multiplier.
    pub scale: f32,
    /// Anchor point, normalized 0–1, Y-down screen convention.
    /// [0, 0] = top-left, [0.5, 0.5] = center (default), [1, 1] = bottom-right.
    pub anchor: Vec2,
    /// RGBA color tint. Alpha is the tint alpha, independent of `opacity`.
    pub color: Vec4,
    /// Corner radius in pixels (SDF). 0.0 = sharp corners.
    pub corner_radius: f32,
}

impl QuadState {
    /// Linearly interpolate between `self` (from-state) and `other` (to-state).
    ///
    /// `t = 0.0` returns `self`, `t = 1.0` returns `other`.
    /// Rotation uses a direct lerp — it takes the long way around for differences
    /// > 180°. Sufficient for typical UI rotation ranges; use a separate slerp
    /// wrapper for full-circle animations.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            size: self.size.lerp(other.size, t),
            rotation: self.rotation + (other.rotation - self.rotation) * t,
            scale: self.scale + (other.scale - self.scale) * t,
            anchor: self.anchor.lerp(other.anchor, t),
            color: self.color.lerp(other.color, t),
            corner_radius: self.corner_radius + (other.corner_radius - self.corner_radius) * t,
        }
    }
}

impl Default for QuadState {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            size: Vec2::new(100.0, 100.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE, // opaque white
            corner_radius: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle — the component state machine
// ---------------------------------------------------------------------------

/// The transition lifecycle of a component entity.
///
/// ```text
///   Idle ──── TransitionRequest ───► Transitioning ──── t=1.0 ───► Idle
/// ```
#[derive(Component, Debug, Clone, PartialEq, Default)]
pub enum Lifecycle {
    /// No active transition. The entity is fully settled at its current state.
    #[default]
    Idle,
    /// A transition is running. The entity carries an `ActiveTransition` component.
    Transitioning,
}

// ---------------------------------------------------------------------------
// TransitionRequest — signals intent to start a transition
// ---------------------------------------------------------------------------

/// Added to an entity to request a transition to a new `QuadState`.
///
/// `transition_setup_system` reads this, creates an `ActiveTransition`,
/// sets `Lifecycle::Transitioning`, and removes the request.
#[derive(Component, Debug, Clone)]
pub struct TransitionRequest {
    pub to: QuadState,
    pub config: crate::transition::TransitionConfig,
}
