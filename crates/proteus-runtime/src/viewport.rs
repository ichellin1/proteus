//! [`Viewport`]: the drawable area.

use glam::Vec2;

/// How far in from each edge the area that nothing covers begins, in logical
/// pixels: the space taken by display cutouts, rounded corners and on-screen
/// system bars.
///
/// Always zero today: no host reports insets yet. Reserved for devices that
/// have them, such as phones.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Insets {
    /// Inset from the top edge.
    pub top: f32,
    /// Inset from the right edge.
    pub right: f32,
    /// Inset from the bottom edge.
    pub bottom: f32,
    /// Inset from the left edge.
    pub left: f32,
}

/// The drawable area.
///
/// The host reports it to [`Engine::resize`](crate::Engine::resize) when it
/// changes, and the app reads it from [`Frame::viewport`](crate::Frame::viewport).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Size in logical pixels, which don't depend on the display's density.
    /// Components are laid out in these units.
    pub logical_size: Vec2,
    /// Physical pixels per logical pixel: `2.0` on a typical high-density
    /// display.
    pub scale_factor: f32,
    /// Areas at the edges that may be covered. Always zero today; see
    /// [`Insets`].
    pub safe_area: Insets,
}

impl Viewport {
    /// A viewport with no safe-area insets, as on desktop.
    pub fn new(logical_size: Vec2, scale_factor: f32) -> Self {
        Self {
            logical_size,
            scale_factor,
            safe_area: Insets::default(),
        }
    }

    /// The size in physical pixels, at least 1 in each direction.
    pub fn physical_size(&self) -> (u32, u32) {
        (
            (self.logical_size.x * self.scale_factor).round().max(1.0) as u32,
            (self.logical_size.y * self.scale_factor).round().max(1.0) as u32,
        )
    }
}
