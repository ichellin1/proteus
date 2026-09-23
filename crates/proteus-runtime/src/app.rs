//! [`App`] — what an application implements, and [`Frame`], the per-call
//! context it receives.

use std::sync::Arc;

use proteus_render::{GpuContext, QuadPipeline, TextureId};
use proteus_sdk::{Proteus, TextureHandle};

use crate::services::{FetchId, FetchResult, HostServices, VideoStream};
use crate::viewport::Viewport;
use proteus_sdk::TextureRequest;

/// The per-call context handed to [`App::setup`] and [`App::update`].
///
/// Bundles the three things application code touches: the headless
/// [`Proteus`] world, the host's asset [`services`](HostServices), and the
/// current [`Viewport`]. Held by the [`Engine`] and borrowed out for the
/// duration of each `App` call.
///
/// [`Engine`]: crate::Engine
pub struct Frame<'a> {
    pub proteus: &'a mut Proteus,
    pub services: &'a mut dyn HostServices,
    pub viewport: Viewport,
}

impl Frame<'_> {
    /// Fetch an asset's raw bytes by key (see [`HostServices::load_asset`]).
    /// Convenience for attaching an [`Image`](proteus_ui::Image) component;
    /// the [`Renderer`](crate::Renderer) bakes those each frame.
    pub fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>> {
        self.services.load_asset(key)
    }

    /// Fetch an asset, decode + downscale it, upload it to `main_atlas`, and
    /// return a [`TextureHandle`] — for a texture shown on more than one
    /// entity or frame-swapped (an animation set), where an `Image`
    /// component per use won't do. A missing or undecodable asset yields a
    /// null handle that renders as nothing.
    pub fn load_texture(&mut self, key: &str, req: TextureRequest) -> TextureHandle {
        crate::bake::load_texture(self.proteus, self.services, key, req)
    }

    /// Bake already-decoded RGBA pixels (`rgba.len() == width * height * 4`)
    /// directly into `main_atlas` and return a [`TextureHandle`] — the
    /// bake-alone half of [`Self::load_texture`], for a caller that already
    /// has bytes in hand (e.g. procedurally generated content) rather than
    /// an asset key to fetch (M13.4). A full atlas yields a null handle,
    /// same graceful degradation as [`Self::load_texture`].
    pub fn bake_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        req: TextureRequest,
    ) -> TextureHandle {
        crate::bake::bake_texture(self.proteus, width, height, rgba, req)
    }

    /// Start an async fetch (see [`HostServices::fetch_async`]). Thin
    /// pass-through so app code never needs to reach past `Frame` into
    /// `self.services` directly.
    pub fn fetch_async(&mut self, key_or_url: &str) -> FetchId {
        self.services.fetch_async(key_or_url)
    }

    /// Drain completed fetches (see [`HostServices::poll_fetches`]). Call
    /// once per frame.
    pub fn poll_fetches(&mut self) -> Vec<FetchResult> {
        self.services.poll_fetches()
    }

    /// Cancel an in-flight fetch (see [`HostServices::cancel_fetch`]).
    pub fn cancel_fetch(&mut self, id: FetchId) {
        self.services.cancel_fetch(id)
    }

    /// Start playing video `key` (see [`HostServices::open_video`]). `None`
    /// if the host couldn't open it. No GPU texture is allocated yet — the
    /// first [`Self::poll_video`] call that actually receives a frame does
    /// that lazily, from that frame's own reported dimensions (sound
    /// whether the host knows dimensions synchronously, e.g. native's
    /// `ffprobe`, or only learns them asynchronously, e.g. the web host's
    /// `<video>` `loadedmetadata`).
    pub fn play_video(&mut self, key: &str) -> Option<PlayingVideo> {
        let stream = self.services.open_video(key)?;
        Some(PlayingVideo {
            stream,
            texture_id: None,
        })
    }

    /// Poll `playing` for a new frame and upload it if one landed. Returns
    /// `true` iff a frame was actually uploaded this call — e.g. to drive a
    /// loading indicator until the first real frame shows. Call once per
    /// frame while `playing` is live.
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
            // Lazily, from the first frame's own dimensions — see
            // `play_video`'s doc for why this can't happen at open time.
            playing.texture_id =
                Some(pipeline.init_video(&device, &queue, frame.width, frame.height));
        }
        pipeline.upload_video_frame(&queue, &frame.rgba);
        true
    }

    /// Best-effort: abort `playing`'s *initial* load without fully stopping
    /// (see [`VideoStream::cancel_load`]).
    pub fn cancel_video_load(&mut self, playing: &mut PlayingVideo) {
        playing.stream.cancel_load();
    }

    /// Stop `playing`: releases the decode stream and, if a GPU texture was
    /// ever allocated for it, suspends it via `QuadPipeline::suspend_video`.
    pub fn stop_video(&mut self, playing: PlayingVideo) {
        playing.stream.stop();
        if let Some(texture_id) = playing.texture_id {
            let world = self.proteus.world_mut();
            let device = world.resource::<GpuContext>().device.clone();
            let mut pipeline = world.resource_mut::<QuadPipeline>();
            pipeline.suspend_video(&device, texture_id);
            // `suspend_video` only marks the entry `Evicted` — it's built to be
            // resumable. This video is finished, so drop the registry entry too;
            // otherwise every play leaves a permanent record behind, and since
            // eviction only ever considers `main_atlas` entries nothing would
            // ever reclaim them.
            pipeline.texture_registry.free(texture_id);
        }
    }
}

/// A video stream handed to the app by [`Frame::play_video`] — advance it
/// each frame via [`Frame::poll_video`]. Opaque: the only thing an app does
/// with one is pass it back into `Frame`'s own video methods.
pub struct PlayingVideo {
    stream: Box<dyn VideoStream>,
    /// `None` until the first frame lands and `Frame::poll_video` allocates
    /// the GPU texture from its dimensions.
    texture_id: Option<TextureId>,
}

/// A Proteus application.
///
/// The [`Engine`] owns [`Proteus`] and the frame loop; an `App` is a
/// `dyn App` the engine calls into. This replaces the M12 pattern where
/// `proteus-demo`'s `Demo` owned `Proteus` and each shell owned a concrete
/// `Demo` field.
///
/// [`Engine`]: crate::Engine
pub trait App {
    /// Build the initial component tree. Called once by [`Engine::new`],
    /// after the world and renderer exist. `proteus-demo`'s `Demo::new` body
    /// moves here.
    ///
    /// [`Engine::new`]: crate::Engine::new
    fn setup(&mut self, f: &mut Frame);

    /// Per-frame application logic, run **after** [`Proteus::tick`] has
    /// advanced the schedule and before the frame is rendered — a "late
    /// update". React to this frame's interaction / transition events here,
    /// and mutate the world directly if needed; the engine re-runs the
    /// Visibility/Opacity cascade afterwards. Signals fired here are
    /// dispatched on the next frame. Optional — an app that wires everything
    /// with signals and callbacks in [`setup`](App::setup) never needs it.
    /// `proteus-demo`'s `advance_*` steps land here.
    ///
    /// [`Proteus::tick`]: proteus_sdk::Proteus::tick
    fn update(&mut self, f: &mut Frame, dt: f32) {
        let _ = (f, dt);
    }
}

#[cfg(test)]
mod video_tests {
    use super::*;
    use proteus_render::{AtlasConfig, QuadPipeline, DEFAULT_TRANSITION_ATLAS_SIZE};
    use proteus_sdk::Proteus;

    use crate::services::{FetchId, FetchResult, VideoFrame};
    use crate::viewport::Viewport;

    /// One frame, then nothing — enough to make `poll_video` allocate the GPU
    /// texture and register it, which is the state `stop_video` has to clean up.
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

    async fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
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

    /// Stopping a video must drop its `TextureRegistry` entry, not just mark it
    /// evicted.
    ///
    /// `suspend_video` is deliberately resumable — it swaps the texture for a
    /// 1×1 placeholder and sets the state to `Evicted`, leaving the entry in
    /// place. That's right for backgrounding, wrong for "this video is over":
    /// nothing else ever reclaims a `Video` entry, because eviction only
    /// considers `main_atlas` ones (`is_eviction_candidate` requires
    /// `AtlasRegion::Main`). Every play therefore left a permanent row behind.
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
