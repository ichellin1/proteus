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
use proteus_ui::{
    BakeScale, BakedText, InstanceCache, InstanceChanges, TextTextureRef, TransitionAtlasSize,
};

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

        let mut pipeline = QuadPipeline::new(
            device,
            queue,
            surface_format,
            mem.max_instances,
            mem.main_atlas,
            mem.transition_atlas_size,
        );
        pipeline.set_plain_pipeline(device, config.render.plain_pipeline);
        pipeline.set_skip_covered_pixels(device, config.render.skip_covered_pixels);
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
        world.insert_resource(BakeScale(
            config.resources.bake_scale(viewport.scale_factor),
        ));
        // Every host makes its renderer from the config, so the key settings
        // reach the app here too.
        proteus.set_key_repeat(config.input.key_repeat);
        proteus.set_long_press_ms(config.input.long_press_ms);

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
    ///
    /// If the display's scale factor changed the bake scale, as when a window
    /// moves to a display of another density, text is baked again at the new
    /// one before the next frame is drawn. Baked components keep the scale
    /// they were baked at: baking destroys what they were drawn from.
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

        let scale = self.config.resources.bake_scale(viewport.scale_factor);
        let world = proteus.world_mut();
        if world.resource::<BakeScale>().0 != scale {
            world.insert_resource(BakeScale(scale));
            let baked: Vec<_> = world
                .query_filtered::<bevy_ecs::entity::Entity, bevy_ecs::query::With<BakedText>>()
                .iter(world)
                .collect();
            for entity in baked {
                // Releases the old texture; see `proteus_ui::texture_ref`.
                world
                    .entity_mut(entity)
                    .remove::<(BakedText, TextTextureRef)>();
            }
        }
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
        let size = target.texture().size();
        proteus
            .world_mut()
            .resource_mut::<QuadPipeline>()
            .prepare_depth(&gpu.device, size.width, size.height);
        let world = proteus.world();
        let pipeline = world.resource::<QuadPipeline>();

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
                depth_stencil_attachment: pipeline.depth_attachment(),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Draws nothing when there are no instances.
            pipeline.draw(&mut pass);
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
            Self::with_scale(config, 1.0)
        }

        /// A scene on a display with `scale_factor`.
        fn with_scale(config: ProteusConfig, scale_factor: f32) -> Option<Self> {
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
                Viewport::new(Vec2::new(64.0, 64.0), scale_factor),
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

    /// `handle`'s baked text: its size in logical pixels, and its texture's
    /// width in the atlas, in texels.
    fn baked_text(scene: &Scene, handle: &Handle) -> ([f32; 2], u32) {
        let world = scene.proteus.world();
        let baked = world.get::<BakedText>(handle.id()).expect("baked");
        let texture = world.get::<TextTextureRef>(handle.id()).expect("baked").0;
        let width = world
            .resource::<QuadPipeline>()
            .texture_registry
            .main_atlas_region(texture)
            .expect("in the atlas")
            .width;
        (baked.pixel_size, width)
    }

    /// A component showing "Proteus" at 16 pixels, baked.
    fn text_scene(scale_factor: f32, config: ProteusConfig) -> Option<(Scene, Handle)> {
        let mut scene = Scene::with_scale(config, scale_factor)?;
        let label = scene
            .proteus
            .component(ComponentSpec::new(QuadState::default()).text(Text::new("Proteus", 16.0)));
        scene.frame();
        Some((scene, label))
    }

    #[test]
    fn text_is_baked_at_the_display_scale_and_drawn_at_its_logical_size() {
        let Some((at_1, label_1)) = text_scene(1.0, ProteusConfig::default()) else {
            return;
        };
        let Some((at_2, label_2)) = text_scene(2.0, ProteusConfig::default()) else {
            return;
        };
        let (size_1, texels_1) = baked_text(&at_1, &label_1);
        let (size_2, texels_2) = baked_text(&at_2, &label_2);
        // Glyphs are rounded to whole texels at each size, so allow a texel.
        assert!(
            (texels_2 as f32 - 2.0 * texels_1 as f32).abs() <= 2.0,
            "twice the texels: {texels_1} at 1, {texels_2} at 2"
        );
        assert!(
            (size_2[0] - size_1[0]).abs() <= 1.0 && (size_2[1] - size_1[1]).abs() <= 1.0,
            "the same logical size: {size_1:?} at 1, {size_2:?} at 2"
        );
    }

    #[test]
    fn a_new_display_scale_bakes_text_again() {
        let Some((mut scene, label)) = text_scene(1.0, ProteusConfig::default()) else {
            return;
        };
        let (size_1, texels_1) = baked_text(&scene, &label);
        scene.renderer.resize(
            &mut scene.proteus,
            Viewport::new(Vec2::new(64.0, 64.0), 2.0),
        );
        assert!(scene.frame(), "the new bake is drawn");
        let (size_2, texels_2) = baked_text(&scene, &label);
        assert!(
            texels_2 as f32 >= 2.0 * texels_1 as f32 - 2.0,
            "{texels_1} then {texels_2}"
        );
        assert!(
            (size_2[0] - size_1[0]).abs() <= 1.0,
            "{size_1:?} then {size_2:?}"
        );
    }

    #[test]
    fn max_bake_scale_caps_the_bake_scale() {
        let Some((at_2, label_2)) = text_scene(2.0, ProteusConfig::default()) else {
            return;
        };
        let Some((at_3, label_3)) = text_scene(3.0, ProteusConfig::default()) else {
            return;
        };
        assert_eq!(
            baked_text(&at_3, &label_3).1,
            baked_text(&at_2, &label_2).1,
            "a 3× display bakes at the default cap, 2"
        );
        let mut config = ProteusConfig::default();
        config.resources.max_bake_scale = 1.0;
        let Some((capped, label)) = text_scene(2.0, config) else {
            return;
        };
        let Some((at_1, label_1)) = text_scene(1.0, ProteusConfig::default()) else {
            return;
        };
        assert_eq!(baked_text(&capped, &label).1, baked_text(&at_1, &label_1).1);
    }

    #[test]
    fn a_baked_component_is_baked_at_the_display_scale() {
        let Some(mut scene) = Scene::with_scale(ProteusConfig::default(), 2.0) else {
            return;
        };
        let card = scene.proteus.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(20.0, 10.0),
                color: Vec4::new(0.2, 0.4, 0.8, 1.0),
                ..Default::default()
            })
            .bake(),
        );
        scene.frame();
        scene.frame();
        let baked = scene
            .proteus
            .world()
            .get::<proteus_ui::BakedComposite>(card.id())
            .expect("baked");
        assert_eq!(baked.pixel_size, [40.0, 20.0]);
    }

    /// A split whose source, at the bake scale, would be wider than the
    /// transition atlas still bakes its pieces, rather than drawing them as
    /// plain shapes: like the demo's video screen, 1,152 wide in a 1,280-wide
    /// window, going back to its tiles at a scale of 2.
    #[test]
    fn a_split_too_wide_for_the_transition_atlas_at_its_scale_still_bakes() {
        let Some(mut scene) = Scene::with_scale(ProteusConfig::default(), 2.0) else {
            return;
        };
        let screen = scene.proteus.component(ComponentSpec::new(QuadState {
            size: Vec2::new(1152.0, 648.0),
            color: Vec4::new(0.3, 0.3, 0.8, 1.0),
            ..Default::default()
        }));
        let tiles: Vec<Handle> = (0..3)
            .map(|i| {
                scene.proteus.component(
                    ComponentSpec::new(QuadState {
                        position: Vec3::new(-300.0 + 300.0 * i as f32, 0.0, 0.0),
                        size: Vec2::new(200.0, 300.0),
                        color: Vec4::new(0.8, 0.3, 0.3, 1.0),
                        ..Default::default()
                    })
                    .visible(false),
                )
            })
            .collect();
        scene.frame();
        screen
            .split_to(
                &mut scene.proteus,
                &tiles,
                TransitionConfig {
                    duration: 0.5,
                    delay: 0.0,
                    easing: Easing::Linear,
                },
                proteus_sdk::SplitStrategy::Row,
            )
            .unwrap();
        scene.frame();
        scene.frame();
        let world = scene.proteus.world_mut();
        let baked = world
            .query_filtered::<(), (
                bevy_ecs::query::With<proteus_ui::Virtual>,
                bevy_ecs::query::With<proteus_ui::BakedTexture>,
            )>()
            .iter(world)
            .count();
        assert_eq!(baked, 3, "every piece is baked, not a plain shape");
    }

    /// A split's bakes fit the transition atlas together, not just one by
    /// one: like the demo's gallery going from an enlarged photo back to its
    /// 4 × 3 grid at a scale of 2, where the photo and the twelve tiles, each
    /// at 2, would overflow it, and the last tiles would be drawn as plain
    /// shapes.
    #[test]
    fn a_splits_bakes_all_fit_the_transition_atlas_together() {
        let Some(mut scene) = Scene::with_scale(ProteusConfig::default(), 2.0) else {
            return;
        };
        let photo = scene.proteus.component(ComponentSpec::new(QuadState {
            size: Vec2::new(1100.0, 750.0),
            color: Vec4::new(0.3, 0.3, 0.8, 1.0),
            ..Default::default()
        }));
        let tiles: Vec<Handle> = (0..12)
            .map(|i| {
                let (col, row) = ((i % 4) as f32, (i / 4) as f32);
                scene.proteus.component(
                    ComponentSpec::new(QuadState {
                        position: Vec3::new(-360.0 + 240.0 * col, 240.0 - 240.0 * row, 0.0),
                        size: Vec2::new(220.0, 220.0),
                        color: Vec4::new(0.8, 0.3, 0.3, 1.0),
                        ..Default::default()
                    })
                    .visible(false),
                )
            })
            .collect();
        scene.frame();
        photo
            .split_to(
                &mut scene.proteus,
                &tiles,
                TransitionConfig {
                    duration: 0.5,
                    delay: 0.0,
                    easing: Easing::Linear,
                },
                proteus_sdk::SplitStrategy::Grid { cols: 4, rows: 3 },
            )
            .unwrap();
        scene.frame();
        scene.frame();
        let world = scene.proteus.world_mut();
        let baked = world
            .query_filtered::<(), (
                bevy_ecs::query::With<proteus_ui::Virtual>,
                bevy_ecs::query::With<proteus_ui::BakedTexture>,
            )>()
            .iter(world)
            .count();
        assert_eq!(baked, 12, "every piece is baked, not a plain shape");
    }
}
