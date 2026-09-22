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
//! Surface *acquire / reconfigure / present* stays with the host — this
//! type never sees a `wgpu::Surface`, only an already-acquired
//! `wgpu::TextureView`. `QuadPipeline` and `proteus_render::GpuContext` live
//! as `World` resources (as in M12); `proteus_ui::bake_system` reads them
//! from there for composite bakes, and this type reaches them the same way
//! via [`Proteus::world_mut`]. [`Renderer::new`] is what inserts them
//! (the M12 shells did it themselves).
//!

use proteus_render::{GpuContext, QuadPipeline};
use proteus_sdk::Proteus;
use proteus_ui::{collect_instances, TransitionAtlasSize};

use crate::bake;
use crate::config::{FontSource, ProteusConfig};
use crate::viewport::Viewport;

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
    /// Panics if `config.memory`'s sizing does not fit the device's reported
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
        let mem = &config.memory;
        proteus_render::validate_atlas_config(device, &mem.main_atlas)
            .expect("ProteusConfig.memory.main_atlas must fit the device's reported limits");
        proteus_render::validate_render_config(
            device,
            mem.transition_atlas_size,
            mem.max_instances,
        )
        .expect("ProteusConfig.memory sizing must fit the device's reported limits");
        if config.debug.validate_config {
            log::info!(
                "ProteusConfig: ~{:.1} MiB estimated resident GPU memory (main_atlas {}×{}×{}, transition_atlas {}², {} instances)",
                config.estimated_gpu_bytes() as f64 / (1024.0 * 1024.0),
                mem.main_atlas.page_size,
                mem.main_atlas.page_size,
                mem.main_atlas.page_count,
                mem.transition_atlas_size,
                mem.max_instances,
            );
        }

        let pipeline = QuadPipeline::new(
            device,
            queue,
            surface_format,
            mem.max_instances,
            mem.main_atlas,
            mem.transition_atlas_size,
        );
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
        world.insert_resource(TransitionAtlasSize(mem.transition_atlas_size));

        // M13.4 step 2: TextConfig.default_font was declared back in M13.5
        // but always ignored in favor of the embedded font — actually read
        // it now. `FontAtlas::new(&[u8])` already accepted arbitrary TTF/OTF
        // bytes; this was pure wiring, no new capability to build.
        let font_atlas = match &config.text.default_font {
            FontSource::Embedded => proteus_render::FontAtlas::with_embedded_font(),
            FontSource::Bytes(bytes) => proteus_render::FontAtlas::new(bytes),
        };

        Self {
            font_atlas,
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

        let lazy_load = self.config.resources.lazy_load;
        bake::bake_pending_text(world, &mut self.font_atlas, &gpu.queue, lazy_load);
        bake::bake_pending_images(
            world,
            &gpu.queue,
            self.config.resources.image_max_side,
            lazy_load,
        );

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
                        load: wgpu::LoadOp::Clear(self.config.render.wgpu_clear_color()),
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
