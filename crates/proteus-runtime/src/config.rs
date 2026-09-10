//! [`ProteusConfig`] — runtime configuration threaded through host
//! construction.
//!
//! M13.1 placeholder: only what [`Renderer`] needs today. The full model —
//! atlas-sizing strategy for constrained targets (the `transition_atlas`
//! 2×-window default is punishing on a 4K / 512 MB device), `max_textures`,
//! safe defaults per target class — is M13.5.
//!
//! [`Renderer`]: crate::Renderer

use proteus_render::AtlasConfig;

/// Host-supplied runtime configuration.
#[derive(Debug, Clone, Copy)]
pub struct ProteusConfig {
    /// `main_atlas` sizing. `None` = [`AtlasConfig::default`].
    pub atlas: Option<AtlasConfig>,
    /// The surface clear color each frame, straight (non-premultiplied)
    /// RGBA, `0.0`–`1.0`. Shows through component transparency and during
    /// the first frames before any background asset has loaded. Defaults to
    /// opaque black; an app with a full-window background sets its own
    /// resting color here.
    pub clear_color: [f64; 4],
    /// Fallback downscale cap (longest side, pixels) for `Image` entities
    /// that don't set their own [`max_side`](proteus_ui::Image::max_side).
    /// `None` = no cap.
    pub image_max_side: Option<u32>,
}

impl Default for ProteusConfig {
    fn default() -> Self {
        Self {
            atlas: None,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            image_max_side: None,
        }
    }
}

impl ProteusConfig {
    /// The effective atlas config — the supplied one, or the default.
    pub fn atlas_config(&self) -> AtlasConfig {
        self.atlas.unwrap_or_default()
    }

    /// [`clear_color`](Self::clear_color) as a [`wgpu::Color`].
    pub fn wgpu_clear_color(&self) -> wgpu::Color {
        let [r, g, b, a] = self.clear_color;
        wgpu::Color { r, g, b, a }
    }
}
