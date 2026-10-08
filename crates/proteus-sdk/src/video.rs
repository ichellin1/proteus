//! Video: frames from the app's own player, shown on components.
//!
//! **Experimental.** Proteus shows video; it doesn't play it. The app brings
//! its own player, such as the browser's `<video>` element, `ffmpeg` or a
//! hardware decoder, and hands Proteus each frame:
//!
//! ```text
//! Proteus::create_video          a VideoHandle
//!         │  each frame the player decodes:
//!         ▼
//! VideoHandle::upload_frame      RGBA pixels into the video texture
//!         │
//!         ▼
//! Handle::show_video             components draw the video texture
//! ```
//!
//! V1 supports one video at a time. Creating a video while another exists
//! replaces it: its handle stops working, and every component showing video
//! shows the new one. Playback controls are the player's own.

use proteus_render::{GpuContext, QuadPipeline, TextureId};

use crate::Proteus;

/// A video whose frames the app supplies. From [`Proteus::create_video`].
///
/// Upload each frame with [`VideoHandle::upload_frame`], show the video on
/// components with [`Handle::show_video`](crate::Handle::show_video), and
/// release it with [`VideoHandle::release`] when done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoHandle(u64);

/// The current video, if any. One field of [`Proteus`].
#[derive(Debug, Default)]
pub(crate) struct VideoState {
    current: Option<CurrentVideo>,
    next_id: u64,
}

#[derive(Debug)]
struct CurrentVideo {
    id: u64,
    /// The video texture and its size, once the first frame has arrived.
    texture: Option<(TextureId, u32, u32)>,
}

impl VideoState {
    pub(crate) fn is_current(&self, video: VideoHandle) -> bool {
        self.current.as_ref().is_some_and(|c| c.id == video.0)
    }
}

impl Proteus {
    /// Creates a video, whose frames the app's player supplies through
    /// [`VideoHandle::upload_frame`]. **Experimental**; see the
    /// [`video`](crate::video) module.
    ///
    /// There is one video at a time. If one already exists, it is released,
    /// with a warning, and its handle stops working.
    pub fn create_video(&mut self) -> VideoHandle {
        if let Some(old) = self.video.current.take() {
            log::warn!(
                "Proteus::create_video: replacing video {} — there is one video at a time",
                old.id
            );
            release_texture(self, old.texture);
        }
        let id = self.video.next_id;
        self.video.next_id += 1;
        self.video.current = Some(CurrentVideo { id, texture: None });
        VideoHandle(id)
    }
}

impl VideoHandle {
    /// Uploads one frame of `width × height` RGBA pixels, `width * height * 4`
    /// bytes, which every component showing this video draws from the next
    /// frame. Call it for each frame the player decodes.
    ///
    /// The texture is created at the first frame's size, and replaced when
    /// the size changes, as it can in adaptive streaming.
    ///
    /// Returns `false`, uploading nothing, if the video has been released or
    /// replaced, if `rgba` has the wrong length (with a warning), or if no GPU
    /// is set up, as in a headless test.
    pub fn upload_frame(&self, app: &mut Proteus, width: u32, height: u32, rgba: &[u8]) -> bool {
        let Some(current) = app.video.current.as_ref().filter(|c| c.id == self.0) else {
            log::warn!(
                "VideoHandle::upload_frame: video {} was released or replaced — frame ignored",
                self.0
            );
            return false;
        };
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected || expected == 0 {
            log::warn!(
                "VideoHandle::upload_frame: a {width}×{height} frame needs {expected} bytes, got {} — frame ignored",
                rgba.len()
            );
            return false;
        }
        let old_texture = current.texture;

        let world = &mut app.world.world;
        let Some((device, queue)) = world
            .get_resource::<GpuContext>()
            .map(|g| (g.device.clone(), g.queue.clone()))
        else {
            return false;
        };
        let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
            return false;
        };
        let texture = match old_texture {
            Some((id, w, h)) if (w, h) == (width, height) => id,
            _ => {
                if let Some((old_id, _, _)) = old_texture {
                    pipeline.texture_registry.free(old_id);
                }
                pipeline.init_video(&device, &queue, width, height)
            }
        };
        pipeline.upload_video_frame(&queue, rgba);
        if let Some(current) = app.video.current.as_mut() {
            current.texture = Some((texture, width, height));
        }
        true
    }

    /// Releases this video and its GPU texture. Components showing video then
    /// show nothing in its place until [`Handle::hide_video`] or another
    /// video. Does nothing if the video was already released or replaced.
    ///
    /// [`Handle::hide_video`]: crate::Handle::hide_video
    pub fn release(self, app: &mut Proteus) {
        if !app.video.is_current(self) {
            return;
        }
        let texture = app.video.current.take().and_then(|c| c.texture);
        release_texture(app, texture);
    }
}

/// Shrinks the video texture to a 1×1 placeholder and frees its registry
/// entry, if it has one.
fn release_texture(app: &mut Proteus, texture: Option<(TextureId, u32, u32)>) {
    let Some((id, _, _)) = texture else {
        return;
    };
    let world = &mut app.world.world;
    let Some(device) = world.get_resource::<GpuContext>().map(|g| g.device.clone()) else {
        return;
    };
    let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
        return;
    };
    pipeline.suspend_video(&device, id);
    // `suspend_video` only marks the entry evicted, so it can resume. This
    // video is finished, so free the entry too: nothing else reclaims video
    // entries.
    pipeline.texture_registry.free(id);
}

#[cfg(test)]
mod tests {
    use proteus_render::{AtlasConfig, DEFAULT_TRANSITION_ATLAS_SIZE};

    use super::*;

    fn gpu_app() -> Option<Proteus> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .ok()?;
        let mut app = Proteus::new();
        app.world_mut().insert_resource(QuadPipeline::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            16,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        ));
        app.world_mut()
            .insert_resource(GpuContext { device, queue });
        Some(app)
    }

    fn texture(app: &Proteus) -> Option<(TextureId, u32, u32)> {
        app.video.current.as_ref().and_then(|c| c.texture)
    }

    fn registered(app: &Proteus, id: TextureId) -> bool {
        app.world
            .world
            .resource::<QuadPipeline>()
            .texture_registry
            .info(id)
            .is_some()
    }

    // The first frame creates the video texture, a frame of a new size
    // replaces it, and releasing the video frees it. Nothing else reclaims a
    // video's registry entry, so each of these must free the one before.
    #[test]
    fn the_video_texture_follows_the_frames_and_is_freed_on_release() {
        let Some(mut app) = gpu_app() else {
            eprintln!("proteus-sdk: no GPU adapter available — skipping");
            return;
        };
        let video = app.create_video();
        assert!(texture(&app).is_none(), "created with the first frame");

        assert!(!video.upload_frame(&mut app, 2, 2, &[0; 4]), "wrong length");
        assert!(video.upload_frame(&mut app, 2, 2, &[255; 16]));
        let (first, _, _) = texture(&app).unwrap();
        assert!(video.upload_frame(&mut app, 2, 2, &[255; 16]));
        assert_eq!(texture(&app).unwrap().0, first, "same size, same texture");

        assert!(video.upload_frame(&mut app, 4, 2, &[255; 32]));
        let (second, w, h) = texture(&app).unwrap();
        assert_eq!((w, h), (4, 2));
        assert!(!registered(&app, first), "the old size is freed");

        video.release(&mut app);
        assert!(!registered(&app, second), "released");
    }

    // A new video replaces the old one and frees its texture.
    #[test]
    fn creating_a_video_frees_the_one_it_replaces() {
        let Some(mut app) = gpu_app() else {
            eprintln!("proteus-sdk: no GPU adapter available — skipping");
            return;
        };
        let first = app.create_video();
        assert!(first.upload_frame(&mut app, 2, 2, &[255; 16]));
        let (id, _, _) = texture(&app).unwrap();

        let _second = app.create_video();
        assert!(!registered(&app, id));
    }
}
