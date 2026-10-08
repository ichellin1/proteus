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
    /// Position in world units, or relative to the parent for a child. Among
    /// overlapping top-level entities, higher `z` is drawn on top; with equal
    /// `z`, the one created last is.
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
    /// Interpolates linearly from `self` to `other`: `t = 0.0` returns `self`,
    /// and `t = 1.0` returns `other`.
    ///
    /// Rotation is interpolated directly, not by the shortest way round, so a
    /// change of more than half a turn goes the long way.
    ///
    /// A `t` outside `0`–`1`, from an easing curve that overshoots,
    /// extrapolates, but sizes, corner radii and scale don't go below zero and
    /// colors stay within `0`–`1`.
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        // `t` can go past 0 or 1 with an easing curve that overshoots. Keep
        // the result drawable: no negative sizes, corner radii or scale, and
        // colors within 0..=1.
        Self {
            position: self.position.lerp(other.position, t),
            size: self.size.lerp(other.size, t).max(Vec2::ZERO),
            rotation: self.rotation + (other.rotation - self.rotation) * t,
            scale: (self.scale + (other.scale - self.scale) * t).max(0.0),
            anchor: self.anchor.lerp(other.anchor, t),
            color: self.color.lerp(other.color, t).clamp(Vec4::ZERO, Vec4::ONE),
            corner_radius: (self.corner_radius + (other.corner_radius - self.corner_radius) * t)
                .max(0.0),
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
// TransitionRequest — a request to start a transition
// ---------------------------------------------------------------------------

/// Added to an entity to request a transition to a new `QuadState`.
///
/// `transition_setup_system` reads this, creates an `ActiveTransition`,
/// sets `Lifecycle::Transitioning`, and removes the request.
#[derive(Component, Debug, Clone, Default)]
pub struct TransitionRequest {
    /// The geometry to end at.
    pub to: QuadState,
    /// How the transition is timed.
    pub config: crate::transition::TransitionConfig,
    /// Where to start, if not the entity's current geometry. A channel uses it
    /// so that the `to` entity starts from the `from` entity's geometry.
    pub from_state: Option<QuadState>,
}

// ---------------------------------------------------------------------------
// Visibility — ECS activation flag
// ---------------------------------------------------------------------------

/// Whether an entity is visible.
///
/// A hidden entity still exists, but isn't drawn or hit-tested, and neither
/// are its descendants. Splits and merges hide their sources and targets this
/// way. The default is visible.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Visibility {
    /// `true` if the entity is visible.
    pub visible: bool,
}

impl Visibility {
    /// Convenience constant — fully visible.
    pub const VISIBLE: Self = Self { visible: true };
    /// Convenience constant — hidden from all behavioral systems.
    pub const HIDDEN: Self = Self { visible: false };
}

impl Default for Visibility {
    fn default() -> Self {
        Self::VISIBLE
    }
}

// ---------------------------------------------------------------------------
// Disabled / TransitionInteractionConfig: when an entity accepts input
// ---------------------------------------------------------------------------

/// Marks an entity as disabled: drawn, but ignoring input.
///
/// A disabled entity gets no hover, press, click or focus events, but still
/// blocks the pointer from reaching the entities behind it (see
/// [`crate::input`]). `interaction_style_system` applies its `disabled`
/// style, so it can look unavailable.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Disabled;

/// Whether an entity accepts input while it is transitioning.
///
/// Without this, or with `allow_pointer: false`, a transitioning entity is left
/// out of hit-testing.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TransitionInteractionConfig {
    /// Accept pointer input during a transition. Defaults to `false`.
    pub allow_pointer: bool,
    /// Not read yet; reserved for keyboard navigation. Defaults to `false`.
    pub allow_navigation: bool,
}

// ---------------------------------------------------------------------------
// Virtual — render-only marker
// ---------------------------------------------------------------------------

/// Marker for virtual entities created by group transitions (1→N, N→1).
///
/// Virtual entities are ephemeral stand-ins that animate geometry during a
/// group transition and are despawned on completion. They are purely visual.
/// Every behavioral system — input, navigation, regular transition completion —
/// must query `Without<Virtual>` to exclude them.
#[derive(Component, Debug, Clone)]
pub struct Virtual;
