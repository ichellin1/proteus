//! [`Renderer`]: draws a frame.
//!
//! Each frame, [`Renderer::prepare`]:
//!
//! 1. bakes pending [`Text`](proteus_ui::Text) into the atlas;
//! 2. decodes and bakes pending [`Image`](proteus_ui::Image)s;
//! 3. brings every visible component's data (position, size, colors, texture
//!    coordinates and so on) up to date with a [`proteus_ui::InstanceCache`],
//!    which redoes only the components that changed;
//! 4. copies what changed to the GPU.
//!
//! Then [`Renderer::draw`] draws every component with one draw call into the
//! given target. A frame that would look the same as the last one, because
//! neither the components' data nor any texture changed, needn't be drawn:
//! `prepare` says whether it changed, so a host can skip acquiring and
//! presenting a surface texture as well.
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
use proteus_ui::{InstanceCache, InstanceChanges, TransitionAtlasSize};

use crate::bake;
use crate::config::{FontSource, ProteusConfig};
use crate::viewport::Viewport;

/// Draws a frame: bakes pending text and images, then draws every visible
/// component. See the module docs.
pub struct Renderer {
    font_atlas: proteus_render::FontAtlas,
    config: ProteusConfig,
    viewport: Viewport,
    /// Every component's instances, kept from frame to frame.
    instances: InstanceCache,
    /// [`QuadPipeline::content_version`] when the last frame was prepared, or
    /// `None` before the first.
    content_version: Option<u64>,
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
            instances: InstanceCache::default(),
            content_version: None,
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

    /// Draws one frame into `target`, a surface texture the host has acquired:
    /// [`Renderer::prepare`], then [`Renderer::draw`], whether or not the
    /// frame changed. It advances nothing: [`Proteus::tick`] has already run.
    pub fn render(&mut self, proteus: &mut Proteus, target: &wgpu::TextureView) {
        self.prepare(proteus);
        self.draw(proteus, target);
    }

    /// Bakes pending text and images, brings every visible component's
    /// instances up to date, and uploads the ones that changed. Returns whether this frame would look
    /// different from the last one prepared: the components' data changed, or
    /// a texture or the projection did. Always `true` with
    /// [`FrameConfig::skip_idle_frames`](crate::config::FrameConfig::skip_idle_frames)
    /// off.
    ///
    /// When it returns `false`, the last frame drawn is still correct, and the
    /// host can skip [`Renderer::draw`]. A host that skips a changed frame,
    /// because it couldn't acquire a surface texture, must draw the next one
    /// whatever this returns.
    pub fn prepare(&mut self, proteus: &mut Proteus) -> bool {
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

        let changes = self.instances.update(world);
        let mut pipeline = world.resource_mut::<QuadPipeline>();
        let instances_changed = changes != InstanceChanges::None;
        match changes {
            InstanceChanges::None => {}
            InstanceChanges::Ranges(ranges) => {
                pipeline.upload_instance_ranges(&gpu.queue, self.instances.instances(), &ranges);
            }
            InstanceChanges::All => {
                pipeline.upload_instances(&gpu.queue, self.instances.instances());
            }
        }
        // Read after the bakes and the upload, which may change it.
        let version = pipeline.content_version();
        let content_changed = self.content_version != Some(version);
        self.content_version = Some(version);

        instances_changed || content_changed || !self.config.frame.skip_idle_frames
    }

    /// Draws the instances [`Renderer::prepare`] uploaded into `target`, a
    /// surface texture the host has acquired, clearing it first.
    pub fn draw(&mut self, proteus: &mut Proteus, target: &wgpu::TextureView) {
        let gpu = proteus.world().resource::<GpuContext>().clone();
        let world = proteus.world();

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
            // Draws nothing when there are no instances.
            world.resource::<QuadPipeline>().draw(&mut pass);
        }
        gpu.queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3, Vec4};
    use proteus_sdk::{ComponentSpec, Easing, Handle, QuadState, TransitionConfig};
    use proteus_ui::{StyleOverride, Text};

    use super::*;
    use crate::app::gpu_tests::headless_device;

    const DT: f32 = 1.0 / 60.0;

    /// A renderer drawing into a texture, run a frame at a time as a host
    /// runs it.
    struct Scene {
        proteus: Proteus,
        renderer: Renderer,
        target: wgpu::TextureView,
    }

    impl Scene {
        fn new(config: ProteusConfig) -> Option<Self> {
            let Some((device, queue)) = pollster::block_on(headless_device()) else {
                eprintln!("proteus-runtime: no GPU adapter available — skipping");
                return None;
            };
            let mut proteus = Proteus::new();
            let renderer = Renderer::new(
                &mut proteus,
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                Viewport::new(Vec2::new(64.0, 64.0), 1.0),
                config,
            );
            let target = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: 64,
                        height: 64,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            Some(Self {
                proteus,
                renderer,
                target,
            })
        }

        /// A 20 × 20 component at the center, which turns red under the
        /// pointer.
        fn square(&mut self) -> Handle {
            self.proteus.component(
                ComponentSpec::new(QuadState {
                    size: Vec2::new(20.0, 20.0),
                    color: Vec4::new(0.0, 0.0, 1.0, 1.0),
                    ..Default::default()
                })
                .hover(StyleOverride {
                    color: Some(Vec4::new(1.0, 0.0, 0.0, 1.0)),
                    ..Default::default()
                }),
            )
        }

        /// Runs a frame as a host does, and returns whether it was drawn.
        fn frame(&mut self) -> bool {
            self.proteus.tick(DT);
            self.proteus.refresh_cascades();
            let changed = self.renderer.prepare(&mut self.proteus);
            if changed {
                self.renderer.draw(&mut self.proteus, &self.target);
            }
            changed
        }

        /// Runs frames until one isn't drawn, as on a still screen.
        fn settle(&mut self) {
            for _ in 0..10 {
                if !self.frame() {
                    return;
                }
            }
            panic!("the screen never went still");
        }
    }

    #[test]
    fn a_still_frame_isnt_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        scene.square();
        assert!(scene.frame(), "the first frame is drawn");
        scene.settle();
        assert!(!scene.frame());
        assert!(!scene.frame());
    }

    #[test]
    fn every_frame_of_a_running_transition_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        let square = scene.square();
        scene.settle();
        square
            .animate_to(
                &mut scene.proteus,
                QuadState {
                    position: Vec3::new(10.0, 0.0, 0.0),
                    size: Vec2::new(20.0, 20.0),
                    color: Vec4::new(0.0, 0.0, 1.0, 1.0),
                    ..Default::default()
                },
                TransitionConfig {
                    duration: 0.2,
                    delay: 0.0,
                    easing: Easing::Linear,
                },
            )
            .unwrap();
        // 0.2 seconds is 12 frames; the transition starts on the next.
        for i in 0..12 {
            assert!(scene.frame(), "frame {i} of the transition is drawn");
        }
        scene.settle();
    }

    #[test]
    fn input_that_changes_a_style_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        scene.square();
        scene.settle();
        scene.proteus.pointer_moved(Some(Vec2::ZERO));
        assert!(scene.frame(), "the hover style is drawn");
    }

    #[test]
    fn input_that_changes_nothing_isnt_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        scene.square();
        scene.settle();
        // Over the empty corner, away from the square.
        scene.proteus.pointer_moved(Some(Vec2::new(25.0, 25.0)));
        scene.proteus.pointer_pressed();
        assert!(!scene.frame());
        scene.proteus.pointer_released();
        assert!(!scene.frame());
    }

    #[test]
    fn a_changed_component_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        let square = scene.square();
        scene.settle();
        square.set_opacity(&mut scene.proteus, 0.5).unwrap();
        assert!(scene.frame());
        scene.settle();
        square.destroy(&mut scene.proteus).unwrap();
        assert!(scene.frame(), "a component removed is drawn too");
    }

    #[test]
    fn text_that_bakes_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        let square = scene.square();
        scene.settle();
        square
            .set_text(&mut scene.proteus, Text::new("Hi", 12.0))
            .unwrap();
        assert!(scene.frame());
        assert!(square.baked_text_size(&scene.proteus).is_some());
    }

    #[test]
    fn a_new_video_frame_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        scene.square();
        let video = scene.proteus.create_video();
        let pixels = [255u8; 4 * 4 * 4];
        assert!(video.upload_frame(&mut scene.proteus, 4, 4, &pixels));
        scene.settle();
        // The same size reuses the texture, so only its pixels change.
        assert!(video.upload_frame(&mut scene.proteus, 4, 4, &pixels));
        assert!(scene.frame());
    }

    #[test]
    fn a_resize_is_drawn() {
        let Some(mut scene) = Scene::new(ProteusConfig::default()) else {
            return;
        };
        scene.square();
        scene.settle();
        scene.renderer.resize(
            &mut scene.proteus,
            Viewport::new(Vec2::new(32.0, 32.0), 1.0),
        );
        assert!(scene.frame());
    }

    #[test]
    fn with_skip_idle_frames_off_every_frame_is_drawn() {
        let mut config = ProteusConfig::default();
        config.frame.skip_idle_frames = false;
        let Some(mut scene) = Scene::new(config) else {
            return;
        };
        scene.square();
        for _ in 0..5 {
            assert!(scene.frame());
        }
    }
}
