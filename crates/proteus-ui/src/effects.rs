//! Visual effects: drop shadows, glows and borders.
//!
//! Each is an optional component. All are drawn by the fragment shader with no
//! extra render passes or atlas space, and an effect that is absent, or has a
//! zero alpha or width, costs nothing.
//!
//! ## Drop shadow
//!
//! The shadow has the component's shape, including its rounded corners, moved
//! by `offset`, grown by `spread` and softened by `softness`. The quad is
//! enlarged so that the parts of the shadow outside the component are drawn.
//!
//! ## Glow
//!
//! A soft halo around the component, drawn as a shadow with no offset. Its
//! color is set separately from the component's, so it can be any color and
//! works on images too.
//!
//! A component shows a drop shadow or a glow, not both. If both are set, the
//! drop shadow is drawn.
//!
//! ## Border
//!
//! A line along the component's edge, following its rounded corners, `width`
//! pixels thick. It is drawn over the component's fill or image and under its
//! text. Only a border inside the edge (`offset = -1.0`, which
//! [`Border::new`] uses) draws correctly; see [`Border`]. A border is
//! independent of shadows and glows, so a component can have all three.
//!
//  B-14, to be fixed in step 5; remove this paragraph then.
//! **Known issue:** a component's opacity doesn't apply to its shadow, glow
//! or border yet. They stay at full strength as the component fades, so to
//! fade one out completely, fade their colors' alpha too.

use bevy_ecs::prelude::Component;
use glam::{Vec2, Vec4};

// ---------------------------------------------------------------------------
// DropShadow
// ---------------------------------------------------------------------------

/// A soft drop shadow behind the entity.
///
/// `offset` is in the entity's own pixels, x right and y up, so the shadow
/// rotates with the entity. A shadow below and to the right has a positive x
/// and a negative y:
/// ```
/// use glam::Vec2;
/// use proteus_ui::DropShadow;
///
/// let shadow = DropShadow {
///     offset: Vec2::new(4.0, -4.0),  // 4 px right, 4 px down
///     ..DropShadow::default()
/// };
/// ```

#[derive(Component, Clone, Debug)]
pub struct DropShadow {
    /// How far the shadow is moved from the entity, in the entity's own pixels
    /// (x right, y up).
    pub offset: Vec2,
    /// The shadow's color. An alpha of `0.0` turns it off.
    pub color: Vec4,
    /// How soft the edge is, in pixels. The minimum is `0.5`.
    pub softness: f32,
    /// How much larger than the entity the shadow is, in pixels, before
    /// softening.
    pub spread: f32,
}

impl Default for DropShadow {
    /// A subtle, translucent black shadow, 4 pixels right and 4 down, with an
    /// 8-pixel soft edge.
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
    /// A translucent black shadow with the given offset and softness, and no
    /// spread.
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

/// A soft glow around the entity, in a color independent of the entity's own.
///
/// An entity shows a glow or a [`DropShadow`], not both, since the glow is
/// drawn as a shadow with no offset. If both are set, the drop shadow is
/// drawn.
#[derive(Component, Clone, Debug)]
pub struct Glow {
    /// How far the glow spreads, in pixels. Around `4.0` is the smallest that
    /// shows well.
    pub radius: f32,
    /// The glow's color. An alpha of `0.0` turns it off.
    pub color: Vec4,
    /// Multiplies `color`'s alpha; the result is clamped to `0`–`1`.
    pub intensity: f32,
}

impl Default for Glow {
    /// A soft white glow, 12 pixels wide, at 70% intensity.
    fn default() -> Self {
        Self {
            radius: 12.0,
            color: Vec4::new(1.0, 1.0, 1.0, 0.8),
            intensity: 0.7,
        }
    }
}

impl Glow {
    /// A glow with the given radius and color, at 70% intensity.
    pub fn new(radius: f32, color: Vec4) -> Self {
        Self {
            radius,
            color,
            ..Self::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Border
// ---------------------------------------------------------------------------

/// A border around the entity, following its rounded corners.
///
/// Only `offset = -1.0`, a border inside the edge, draws correctly. At `0.0`
/// only the inner half of the border shows, and at `1.0` nothing does, because
/// the quad isn't enlarged to make room outside the edge. [`Border::new`] uses
/// `-1.0`.
#[derive(Component, Clone, Debug)]
pub struct Border {
    /// Thickness in pixels. `0.0` turns the border off.
    pub width: f32,
    /// The border's color. An alpha of `0.0` turns it off.
    pub color: Vec4,
    /// Where the border sits: `-1.0` inside the edge, `0.0` centered on it,
    /// `1.0` outside. Only `-1.0` draws correctly; see [`Border`].
    pub offset: f32,
}

impl Default for Border {
    /// A 2-pixel opaque white border inside the edge.
    fn default() -> Self {
        Self {
            width: 2.0,
            color: Vec4::ONE,
            offset: -1.0,
        }
    }
}

impl Border {
    /// A border of the given width and color, inside the edge.
    pub fn new(width: f32, color: Vec4) -> Self {
        Self {
            width,
            color,
            ..Self::default()
        }
    }
}
