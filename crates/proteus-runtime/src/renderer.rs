//! [`Renderer`] — the render primitive.
//!
//! Owns the [`FontAtlas`] and the [`ProteusConfig`]. The per-frame work that
//! was hand-duplicated in `proteus-shell-native` and `proteus-shell-web`
//! moves here:
//!
//! 1. bake pending [`Text`] — rasterize via [`FontAtlas`] into `main_atlas`
//! 2. bake pending [`Image`] — `decode_image` → `resize_to_fit` → `main_atlas`
//! 3. [`proteus_ui::collect_instances`]
//! 4. `QuadPipeline::upload_instances`
//! 5. encode one render pass into the handed-in target
//!
//! Surface *acquire / reconfigure / present* stays with the [`Host`] — this
//! type never sees a `wgpu::Surface`, only an already-acquired
//! `wgpu::TextureView`. `QuadPipeline` and `proteus_render::GpuContext` stay
//! `World` resources (as in M12); `bake_system` already reads them from the
//! world for composite bakes, and [`Renderer::render`] reaches them the same
//! way via [`Proteus::world_mut`].
//!
//! [`Host`]: crate::Host
//! [`Text`]: proteus_ui::Text
//! [`Image`]: proteus_ui::Image

use proteus_render::FontAtlas;
use proteus_sdk::Proteus;

use crate::config::ProteusConfig;
use crate::viewport::Viewport;

/// See the module docs.
//
// M13.1 step 1: fields/signatures only. `new` and `render` bodies land in
// step 2 when the loop is lifted out of `proteus-shell-native`.
#[allow(dead_code)]
pub struct Renderer {
    font_atlas: FontAtlas,
    config: ProteusConfig,
    viewport: Viewport,
}

impl Renderer {
    /// Build the renderer for a freshly created device/queue and surface
    /// format. The caller has already validated `config`'s atlas sizing
    /// against the device limits (`proteus_render::validate_atlas_config`).
    pub fn new(
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _surface_format: wgpu::TextureFormat,
        _viewport: Viewport,
        _config: &ProteusConfig,
    ) -> Self {
        todo!("M13.1 step 2: lift QuadPipeline/FontAtlas setup out of proteus-shell-native")
    }

    /// Rebuild the orthographic projection for a new viewport. Atlas
    /// resizing on viewport change is out of scope for M13.1 (M13.5).
    pub fn resize(&mut self, _viewport: Viewport) {
        todo!("M13.1 step 2")
    }

    /// Render one frame into `target` — a surface texture view the host has
    /// already acquired. Advances no simulation; [`Proteus::tick`] has
    /// already run by the time the engine calls this.
    pub fn render(&mut self, _proteus: &mut Proteus, _target: &wgpu::TextureView) {
        todo!("M13.1 step 2: bake pending Text/Image, collect_instances, one draw pass")
    }
}
