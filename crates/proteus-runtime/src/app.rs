//! [`App`], what an application implements, and [`Frame`], what it is given
//! on each call.

use std::sync::Arc;

use proteus_render::{GpuContext, QuadPipeline, TextureId};
use proteus_sdk::{Proteus, TextureHandle};

use crate::services::{FetchId, FetchResult, HostServices, VideoStream};
use crate::viewport::Viewport;
use proteus_sdk::TextureRequest;

/// What [`App::setup`] and [`App::update`] are given: the app's [`Proteus`]
/// state, the host's [`HostServices`], and the current [`Viewport`].
pub struct Frame<'a> {
    /// The app's components, channels and callbacks.
    pub proteus: &'a mut Proteus,
    /// Asset loading, fetching and video, provided by the host.
    pub services: &'a mut dyn HostServices,
    /// The current drawable area.
    pub viewport: Viewport,
}

impl Frame<'_> {
    /// Loads an asset's bytes by key; see [`HostServices::load_asset`]. Useful
    /// for a component's image, which the host bakes on the next frame.
    pub fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>> {
        self.services.load_asset(key)
    }

    /// Loads an image asset by key, decodes it, adds it to the atlas and
    /// returns a handle to it. Use this for a texture shown on several
    /// components, or swapped frame by frame.
    ///
    /// A missing or undecodable asset gives a null handle, which draws
    /// nothing. Attach the texture in the same frame; see
    /// [`Proteus::bake_texture`].
    pub fn load_texture(&mut self, key: &str, req: TextureRequest) -> TextureHandle {
        crate::bake::load_texture(self.proteus, self.services, key, req)
    }

    /// Adds RGBA pixels to the atlas and returns a handle to them; see
    /// [`Proteus::bake_texture`]. `rgba` holds `width * height * 4` bytes. A
    /// full atlas gives a null handle. Attach the texture in the same frame.
    pub fn bake_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        req: TextureRequest,
    ) -> TextureHandle {
        crate::bake::bake_texture(self.proteus, width, height, rgba, req)
    }

    /// Starts an asynchronous fetch; see [`HostServices::fetch_async`].
    pub fn fetch_async(&mut self, key_or_url: &str) -> FetchId {
        self.services.fetch_async(key_or_url)
    }

    /// Returns the fetches that have completed; see
    /// [`HostServices::poll_fetches`]. Call once per frame.
    pub fn poll_fetches(&mut self) -> Vec<FetchResult> {
        self.services.poll_fetches()
    }

    /// Cancels a fetch; see [`HostServices::cancel_fetch`].
    pub fn cancel_fetch(&mut self, id: FetchId) {
        self.services.cancel_fetch(id)
    }

    /// Starts playing the video `key`; see [`HostServices::open_video`].
    /// Returns `None` if the host couldn't open it.
    ///
    /// Call [`Frame::poll_video`] every frame to show it on components that
    /// have [`Handle::start_video`](proteus_sdk::Handle::start_video). The GPU
    /// texture is created when the first frame arrives, since some hosts only
    /// learn the video's size then.
    pub fn play_video(&mut self, key: &str) -> Option<PlayingVideo> {
        let stream = self.services.open_video(key)?;
        Some(PlayingVideo {
            stream,
            texture_id: None,
        })
    }

    /// Uploads `playing`'s next frame, if one has been decoded. Returns `true`
    /// if a frame was uploaded, which can drive a loading indicator until the
    /// first frame appears. Call once per frame while the video plays.
    pub fn poll_video(&mut self, playing: &mut PlayingVideo) -> bool {
        let Some(frame) = playing.stream.poll_frame() else {
            return false;
        };
        let world = self.proteus.world_mut();
        let (device, queue) = {
            let gpu = world.resource::<GpuContext>();
            (gpu.device.clone(), gpu.queue.clone())
        };
        let mut pipeline = world.resource_mut::<QuadPipeline>();
        if playing.texture_id.is_none() {
            // Created from the first frame's size; see `play_video`.
            playing.texture_id =
                Some(pipeline.init_video(&device, &queue, frame.width, frame.height));
        }
        pipeline.upload_video_frame(&queue, &frame.rgba);
        true
    }

    /// Cancels `playing`'s initial load, if the host can; see
    /// [`VideoStream::cancel_load`].
    pub fn cancel_video_load(&mut self, playing: &mut PlayingVideo) {
        playing.stream.cancel_load();
    }

    /// Stops `playing`, and releases its decoder and GPU texture.
    pub fn stop_video(&mut self, playing: PlayingVideo) {
        playing.stream.stop();
        if let Some(texture_id) = playing.texture_id {
            let world = self.proteus.world_mut();
            let device = world.resource::<GpuContext>().device.clone();
            let mut pipeline = world.resource_mut::<QuadPipeline>();
            pipeline.suspend_video(&device, texture_id);
            // `suspend_video` only marks the entry evicted, so it can resume.
            // This video is finished, so free the entry as well: nothing else
            // reclaims video entries.
            pipeline.texture_registry.free(texture_id);
        }
    }
}

/// A playing video, from [`Frame::play_video`]. Pass it to
/// [`Frame::poll_video`] every frame, and to [`Frame::stop_video`] when done.
pub struct PlayingVideo {
    stream: Box<dyn VideoStream>,
    /// `None` until the first frame arrives and its texture is created.
    texture_id: Option<TextureId>,
}

/// A Proteus application.
///
/// A host runs it: it calls [`App::setup`] once, then [`App::update`] every
/// frame. The app keeps its own data; the components live in [`Proteus`],
/// which each call receives through [`Frame`].
///
/// # Examples
///
/// ```no_run
/// use proteus_runtime::{App, Frame};
/// use proteus_sdk::{ComponentSpec, QuadState};
///
/// struct Hello;
///
/// impl App for Hello {
///     fn setup(&mut self, f: &mut Frame) {
///         f.proteus.component(ComponentSpec::new(QuadState::default()));
///     }
/// }
/// ```
pub trait App {
    /// Creates the app's initial components. Called once, before the first
    /// frame.
    fn setup(&mut self, f: &mut Frame);

    /// Runs every frame, after [`Proteus::tick`] and before drawing. `dt` is
    /// the time since the previous frame, in seconds.
    ///
    /// React to this frame's events here. Changes to visibility and opacity
    /// are drawn this frame; a transition started here begins on the next.
    /// Optional: an app that does everything with callbacks set up in
    /// [`setup`](App::setup) doesn't need it.
    ///
    /// [`Proteus::tick`]: proteus_sdk::Proteus::tick
    fn update(&mut self, f: &mut Frame, dt: f32) {
        let _ = (f, dt);
    }
}

#[cfg(test)]
pub(crate) mod video_tests {
    use super::*;
    use proteus_render::{AtlasConfig, QuadPipeline, DEFAULT_TRANSITION_ATLAS_SIZE};
    use proteus_sdk::Proteus;

    use crate::services::{FetchId, FetchResult, VideoFrame};
    use crate::viewport::Viewport;

    // One frame, then nothing: enough for `poll_video` to create the texture
    // that `stop_video` has to clean up.
    struct OneFrameStream {
        delivered: bool,
    }

    impl VideoStream for OneFrameStream {
        fn poll_frame(&mut self) -> Option<VideoFrame> {
            if self.delivered {
                return None;
            }
            self.delivered = true;
            Some(VideoFrame {
                width: 4,
                height: 4,
                rgba: Arc::from(vec![255u8; 4 * 4 * 4]),
            })
        }
        fn stop(self: Box<Self>) {}
    }

    struct VideoServices;

    impl HostServices for VideoServices {
        fn load_asset(&mut self, _key: &str) -> Option<Arc<[u8]>> {
            None
        }
        fn fetch_async(&mut self, _key_or_url: &str) -> FetchId {
            FetchId(0)
        }
        fn poll_fetches(&mut self) -> Vec<FetchResult> {
            Vec::new()
        }
        fn cancel_fetch(&mut self, _id: FetchId) {}
        fn open_video(&mut self, _key: &str) -> Option<Box<dyn VideoStream>> {
            Some(Box::new(OneFrameStream { delivered: false }))
        }
    }

    /// A GPU device with no surface, or `None` without an adapter. Shared
    /// with the other modules' GPU tests.
    pub(crate) async fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok()?;
        adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("proteus-runtime-video-test"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await
            .ok()
    }

    // Stopping a video must free its texture registry entry, not just mark it
    // evicted: nothing else ever reclaims a video entry, so each play would
    // leave one behind.
    #[test]
    fn stopping_a_video_frees_its_registry_entry() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };

        let mut proteus = Proteus::new();
        proteus.world_mut().insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        proteus.world_mut().insert_resource(QuadPipeline::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            16,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        ));

        let mut services = VideoServices;
        let mut frame = Frame {
            proteus: &mut proteus,
            services: &mut services,
            viewport: Viewport::new(glam::Vec2::new(64.0, 64.0), 1.0),
        };

        let mut playing = frame
            .play_video("anything")
            .expect("host opened the stream");
        assert!(
            frame.poll_video(&mut playing),
            "the stream's one frame should upload, allocating the texture"
        );
        let texture_id = playing.texture_id.expect("poll_video allocated a texture");

        assert!(
            frame
                .proteus
                .world()
                .resource::<QuadPipeline>()
                .texture_registry
                .info(texture_id)
                .is_some(),
            "registered while playing"
        );

        frame.stop_video(playing);

        assert!(
            frame
                .proteus
                .world()
                .resource::<QuadPipeline>()
                .texture_registry
                .info(texture_id)
                .is_none(),
            "a stopped video's registry entry must be freed, not left marked evicted"
        );
    }
}
