//! [`Renderer`] — the render primitive.
//!
//! Owns the [`FontAtlas`] and the [`ProteusConfig`]. The per-frame work that
//! was hand-duplicated in `proteus-shell-native` and `proteus-shell-web`
//! moves here:
//!
//! 1. bake pending [`Text`](proteus_ui::Text) — rasterize into `main_atlas`
//! 2. bake pending [`Image`](proteus_ui::Image) — decode + downscale into `main_atlas`
//! 3. [`proteus_ui::collect_instances`]
//! 4. `QuadPipeline::upload_instances`
//! 5. encode one render pass into the handed-in target
//!
//! Surface *acquire / reconfigure / present* stays with the [`Host`] — this
//! type never sees a `wgpu::Surface`, only an already-acquired
//! `wgpu::TextureView`. `QuadPipeline` and `proteus_render::GpuContext` live
//! as `World` resources (as in M12); `proteus_ui::bake_system` reads them
//! from there for composite bakes, and this type reaches them the same way
//! via [`Proteus::world_mut`]. [`Renderer::new`] is what inserts them
//! (the M12 shells did it themselves).
//!
//! [`Host`]: crate::Host

use proteus_render::{GpuContext, QuadPipeline};
use proteus_sdk::Proteus;
use proteus_ui::collect_instances;

use crate::bake;
use crate::config::ProteusConfig;
use crate::viewport::Viewport;

/// Instance-buffer capacity — matches the value both M12 shells used.
const MAX_INSTANCES: u32 = 4096;

/// See the module docs.
pub struct Renderer {
    font_atlas: proteus_render::FontAtlas,
    config: ProteusConfig,
    viewport: Viewport,
}

impl Renderer {
    /// Create the `QuadPipeline` + `GpuContext`, insert them into the world,
    /// set the initial projection, and build the font atlas.
    ///
    /// Panics if `config`'s atlas sizing does not fit the device's reported
    /// limits (a `Result` form can come later; the M12 shells also treated
    /// this as fatal).
    pub fn new(
        proteus: &mut Proteus,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        viewport: Viewport,
        config: ProteusConfig,
    ) -> Self {
        let atlas = config.atlas_config();
        proteus_render::validate_atlas_config(device, &atlas)
            .expect("ProteusConfig atlas sizing must fit the device's reported limits");

        let pipeline = QuadPipeline::new(device, queue, surface_format, MAX_INSTANCES, atlas);
        pipeline.set_view_projection(
            queue,
            QuadPipeline::ortho(viewport.logical_size.x, viewport.logical_size.y),
        );

        let world = proteus.world_mut();
        world.insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        world.insert_resource(pipeline);

        Self {
            font_atlas: proteus_render::FontAtlas::with_embedded_font(),
            config,
            viewport,
        }
    }

    /// The viewport this renderer is currently projecting for.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Rebuild the orthographic projection for a new viewport. The host is
    /// responsible for reconfiguring the `wgpu::Surface` itself; atlas
    /// resizing on viewport change is out of scope for M13.1 (M13.5).
    pub fn resize(&mut self, proteus: &mut Proteus, viewport: Viewport) {
        self.viewport = viewport;
        let queue = proteus.world().resource::<GpuContext>().queue.clone();
        proteus
            .world()
            .resource::<QuadPipeline>()
            .set_view_projection(
                &queue,
                QuadPipeline::ortho(viewport.logical_size.x, viewport.logical_size.y),
            );
    }

    /// Render one frame into `target` — a surface texture view the host has
    /// already acquired. [`Proteus::tick`] has already run by the time the
    /// engine calls this; the renderer advances no simulation.
    pub fn render(&mut self, proteus: &mut Proteus, target: &wgpu::TextureView) {
        let gpu = proteus.world().resource::<GpuContext>().clone();
        let world = proteus.world_mut();

        bake::bake_pending_text(world, &mut self.font_atlas, &gpu.queue);
        bake::bake_pending_images(world, &gpu.queue, self.config.image_max_side);

        let instances = collect_instances(world);
        if !instances.is_empty() {
            world
                .resource_mut::<QuadPipeline>()
                .upload_instances(&gpu.queue, &instances);
        }

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("proteus_frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("proteus_main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.config.wgpu_clear_color()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !instances.is_empty() {
                world.resource::<QuadPipeline>().draw(&mut pass);
            }
        }
        gpu.queue.submit([encoder.finish()]);
    }
}
