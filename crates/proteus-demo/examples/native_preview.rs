//! Minimal native preview harness for `proteus-demo` — **not** part of this
//! crate's public API. `cargo run --example native_preview -p proteus-demo`
//! opens a window and runs whatever content `Demo` currently has, so each
//! M12.5 migration step can be visually confirmed as it lands. With zero
//! content (M12.5 Step 1), it should open a window and render a plain
//! background with no errors — proving the window → wgpu → `Demo` → render
//! pipeline works end to end before any screen content exists.
//!
//! Deliberately minimal: no theme toggle, no HiDPI-perfect asset loading,
//! none of `proteus-shell-native`'s real production concerns — just enough
//! wgpu setup to prove `Demo` renders, mirroring
//! `proteus-shell-native/src/main.rs`'s own device/surface setup pattern
//! (same non-sRGB surface-format selection, same `AtlasConfig`
//! validation) so this harness stays a faithful preview of the real thing.

use std::sync::Arc;

use bevy_ecs::prelude::{Entity, Without};
use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use proteus_demo::Demo;
use proteus_render::{validate_atlas_config, AtlasConfig, FontAtlas, GpuContext, QuadPipeline};
use proteus_ui::{BakedText, Text, TextureRef};

fn main() {
    env_logger::init();
    log::info!("proteus-demo native preview");

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = PreviewApp::default();
    event_loop.run_app(&mut app).expect("event loop error");
}

#[derive(Default)]
struct PreviewApp {
    state: Option<State>,
}

impl ApplicationHandler for PreviewApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            state.window.request_redraw();
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title("proteus-demo — native preview")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32)),
                )
                .expect("failed to create window"),
        );
        let state = pollster::block_on(State::new(window));
        self.state = Some(state);
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                log::info!("window closed — exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::CursorMoved { position, .. } => {
                let scale_factor = state.window.scale_factor() as f32;
                let w = state.surface_config.width as f32 / scale_factor;
                let h = state.surface_config.height as f32 / scale_factor;
                let wx = (position.x as f32 / scale_factor) - w / 2.0;
                let wy = h / 2.0 - (position.y as f32 / scale_factor);
                state.demo.pointer_moved(Some(Vec2::new(wx, wy)));
            }
            WindowEvent::CursorLeft { .. } => state.demo.pointer_moved(None),
            WindowEvent::MouseInput {
                state: btn_state,
                button: MouseButton::Left,
                ..
            } => match btn_state {
                ElementState::Pressed => state.demo.pointer_pressed(),
                ElementState::Released => state.demo.pointer_released(),
            },
            WindowEvent::RedrawRequested => state.render(),
            _ => {}
        }
    }
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    demo: Demo,
    font_atlas: FontAtlas,
    last_frame: std::time::Instant,
}

/// Rasterizes and uploads any `Text` component that doesn't have a
/// `BakedText` yet — mirrors what `proteus-shell-native`/`-web` each
/// hand-roll today (`bake_pending_text`). `proteus-sdk`/`Demo` don't do this
/// themselves (see `proteus-demo`'s crate-root doc: baking stays a shell
/// concern), so a harness that wants to actually *see* text has to do it,
/// same as any other host application would.
fn bake_pending_text(
    world: &mut bevy_ecs::world::World,
    font_atlas: &mut FontAtlas,
    queue: &wgpu::Queue,
) {
    let pending: Vec<(Entity, Text)> = {
        let mut query = world.query_filtered::<(Entity, &Text), Without<BakedText>>();
        query.iter(world).map(|(e, t)| (e, t.clone())).collect()
    };

    for (entity, text) in pending {
        let Some(glyphs) =
            font_atlas.rasterize_text_tracked(&text.content, text.size_px, text.letter_spacing_px)
        else {
            continue;
        };

        let (uv, texture_id) = {
            let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
                return;
            };
            let Some(texture_id) =
                pipeline
                    .texture_registry
                    .register_static(glyphs.width, glyphs.height, false)
            else {
                continue;
            };
            let placement = pipeline
                .texture_registry
                .main_atlas_region(texture_id)
                .expect("just registered");
            pipeline.write_to_main_atlas(queue, placement, &glyphs.rgba_pixels);
            let uv = pipeline
                .texture_registry
                .main_atlas_uv(texture_id)
                .expect("just registered");
            (uv, texture_id)
        };

        world.entity_mut(entity).insert((
            BakedText {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [glyphs.width as f32, glyphs.height as f32],
            },
            TextureRef(texture_id),
        ));
    }
}

impl State {
    async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("no suitable GPU adapter found");

        log::info!("GPU adapter: {}", adapter.get_info().name);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("proteus-demo-preview"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await
            .expect("failed to create GPU device");

        let surface_caps = surface.get_capabilities(&adapter);
        // Non-sRGB surface format — matches proteus-shell-native's own
        // choice exactly (see its RenderState::new doc): every color in
        // this codebase is authored already gamma-encoded, so an sRGB
        // swapchain would apply an unwanted second gamma pass.
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let atlas_config = AtlasConfig::default();
        validate_atlas_config(&device, &atlas_config)
            .expect("AtlasConfig must fit this device's real reported limits");
        let pipeline = QuadPipeline::new(&device, &queue, surface_format, 4096, atlas_config);

        // Window is created at a *logical* size but inner_size() returns
        // *physical* pixels on HiDPI displays — divide by scale_factor so
        // world units stay 1:1 with logical pixels (matches
        // proteus-shell-native's own projection setup).
        let scale_factor = window.scale_factor() as f32;
        pipeline.set_view_projection(
            &queue,
            QuadPipeline::ortho(
                size.width as f32 / scale_factor,
                size.height as f32 / scale_factor,
            ),
        );

        let mut demo = Demo::new();
        demo.app_mut().world_mut().insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        demo.app_mut().world_mut().insert_resource(pipeline);

        log::info!(
            "preview ready — {}x{} px, format {:?}",
            size.width,
            size.height,
            surface_format
        );

        Self {
            window,
            surface,
            device,
            queue,
            surface_config,
            demo,
            font_atlas: FontAtlas::with_embedded_font(),
            last_frame: std::time::Instant::now(),
        }
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);

        let scale_factor = self.window.scale_factor() as f32;
        let pipeline = self.demo.app_mut().world_mut().resource::<QuadPipeline>();
        pipeline.set_view_projection(
            &self.queue,
            QuadPipeline::ortho(
                size.width as f32 / scale_factor,
                size.height as f32 / scale_factor,
            ),
        );
    }

    fn render(&mut self) {
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = std::time::Instant::now();
        self.demo.tick(dt); // Demo::tick already calls refresh_cascades internally.

        bake_pending_text(
            self.demo.app_mut().world_mut(),
            &mut self.font_atlas,
            &self.queue,
        );

        let instances = proteus_ui::collect_instances(self.demo.app_mut().world_mut());

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return;
            }
            e => {
                log::error!("surface error: {e:?}");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());

        let mut pipeline = self
            .demo
            .app_mut()
            .world_mut()
            .resource_mut::<QuadPipeline>();
        if !instances.is_empty() {
            pipeline.upload_instances(&self.queue, &instances);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview_encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.06,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !instances.is_empty() {
                pipeline.draw(&mut pass);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
    }
}
