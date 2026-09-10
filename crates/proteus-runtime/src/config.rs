//! [`ProteusConfig`] — runtime configuration threaded through host
//! construction.
//!
//! M13.1 placeholder: only the `main_atlas` sizing knob [`Renderer`] needs
//! today. The full model — atlas-sizing strategy for constrained targets
//! (the `transition_atlas` 2×-window default is punishing on a 4K / 512 MB
//! device), `max_textures`, safe defaults per target class — is M13.5.
//!
//! [`Renderer`]: crate::Renderer

use proteus_render::AtlasConfig;

/// Host-supplied runtime configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProteusConfig {
    /// `main_atlas` sizing. `None` = [`AtlasConfig::default`].
    pub atlas: Option<AtlasConfig>,
}

impl ProteusConfig {
    /// The effective atlas config — the supplied one, or the default.
    pub fn atlas_config(&self) -> AtlasConfig {
        self.atlas.unwrap_or_default()
    }
}
