//! [`Renderer`]: draws a frame.
//!
//! Each frame it:
//!
//! 1. bakes pending [`Text`](proteus_ui::Text) into the atlas;
//! 2. decodes and bakes pending [`Image`](proteus_ui::Image)s;
//! 3. collects every visible component's data (position, size, colors,
//!    texture coordinates and so on) with [`proteus_ui::collect_instances`];
//! 4. copies that data to the GPU in one go, then draws every component with
//!    one draw call into the given target.
//!
//! The shader is the same for every component; only the data differs.
//! Components are drawn in the order collected, so later ones are on top. At
//! most `ProteusConfig.memory.max_instances` are drawn; any beyond that, the
//! topmost, are dropped with a warning.
//!
//! The host acquires and presents the surface texture; the renderer only
//! draws into it. The GPU resources live in the ECS world, where
//! [`Renderer::new`] puts them, so that systems such as component baking can
//! use them too.
//!

use proteus_render::{GpuContext, QuadPipeline};
use proteus_sdk::Proteus;
use proteus_ui::{collect_instances, TransitionAtlasSize};

use crate::bake;
use crate::config::{FontSource, ProteusConfig};
use crate::viewport::Viewport;

/// Draws a frame: bakes pending text and images, then draws every visible
/// component. See the module docs.
pub struct Renderer {
    font_atlas: proteus_render::FontAtlas,
    config: ProteusConfig,
    viewport: Viewport,
}

impl Renderer {
    /// Creates the GPU resources, adds them to `proteus`'s world, sets the
    /// projection for `viewport`, and builds the font atlas.
    ///
    /// # Panics
    ///
    /// If `config`'s memory settings don't fit `device`'s limits, with the
    /// [`ConfigError`](crate::ConfigError)'s message. Both hosts check the
    /// config before creating the device, with
    /// [`ProteusConfig::check`](crate::ProteusConfig::check), so this is a
    /// backstop for code that creates a renderer itself.
    pub fn new(
        proteus: &mut Proteus,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        viewport: Viewport,
        config: ProteusConfig,
    ) -> Self {
        let mem = &config.memory;
        if let Err(e) = config.check(&device.limits()) {
            panic!("{e}");
        }
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

        // Use the configured font, or the embedded one. Bytes that aren't a
        // font fall back to the embedded font rather than stopping the app;
        // `FontSource::from_bytes` lets an app catch them earlier.
        let font_atlas = match &config.text.default_font {
            FontSource::Embedded => proteus_render::FontAtlas::with_embedded_font(),
            FontSource::Bytes(bytes) => proteus_render::FontAtlas::new(bytes).unwrap_or_else(|e| {
                log::error!("ProteusConfig.text.default_font: {e}; using the embedded font");
                proteus_render::FontAtlas::with_embedded_font()
            }),
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

    /// Updates the projection for a new viewport. The host resizes the GPU
    /// surface itself. The atlases keep their size.
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

    /// Draws one frame into `target`, a surface texture the host has acquired.
    /// It advances nothing: [`Proteus::tick`] has already run.
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
