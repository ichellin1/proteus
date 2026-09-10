//! [`Viewport`] — the drawable area, reported by the [`crate::Host`] once per
//! resize and delivered to the app through [`crate::Frame`].

use glam::Vec2;

/// Safe-area insets in logical pixels.
///
/// Zero on desktop. Populated by the web host from `env(safe-area-inset-*)`
/// in M13.2 (display cutouts, rounded corners, on-screen system bars).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// The drawable area.
///
/// `logical_size` is in logical (DPI-independent) pixels — the space Proteus
/// components are authored in. `scale_factor` maps logical → physical pixels
/// for swap-chain configuration. `safe_area` is non-zero only on platforms
/// with display cutouts (M13.2).
///
/// The [`crate::Engine`] is the single owner: the host reports a `Viewport`
/// to [`crate::Engine::resize`], which updates the renderer's projection and
/// the copy the app reads via [`crate::Frame::viewport`]. Neither the app nor
/// the renderer keeps a second copy to synchronise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub logical_size: Vec2,
    pub scale_factor: f32,
    pub safe_area: Insets,
}

impl Viewport {
    /// A viewport with no safe-area insets — the desktop case.
    pub fn new(logical_size: Vec2, scale_factor: f32) -> Self {
        Self {
            logical_size,
            scale_factor,
            safe_area: Insets::default(),
        }
    }

    /// Physical pixel dimensions, for `wgpu::SurfaceConfiguration`.
    pub fn physical_size(&self) -> (u32, u32) {
        (
            (self.logical_size.x * self.scale_factor).round().max(1.0) as u32,
            (self.logical_size.y * self.scale_factor).round().max(1.0) as u32,
        )
    }
}
