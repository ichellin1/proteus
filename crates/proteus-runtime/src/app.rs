//! [`App`], what an application implements, and [`Frame`], what it is given
//! on each call.

use std::sync::Arc;

use proteus_sdk::{Proteus, TextureHandle};

use crate::services::{FetchId, FetchResult, HostServices};
use crate::viewport::Viewport;
use proteus_sdk::TextureRequest;

/// What [`App::setup`] and [`App::update`] are given: the app's [`Proteus`]
/// state, the host's [`HostServices`], and the current [`Viewport`].
pub struct Frame<'a> {
    /// The app's components, channels and callbacks.
    pub proteus: &'a mut Proteus,
    /// Asset loading and fetching, provided by the host.
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
pub(crate) mod gpu_tests {
    use std::sync::Arc;

    use proteus_sdk::Proteus;

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
                label: Some("proteus-runtime-test"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await
            .ok()
    }

    // A font that can't be read doesn't stop the renderer: it logs an error
    // and draws text in the embedded font instead.
    #[test]
    fn a_font_that_cant_be_read_falls_back_to_the_embedded_font() {
        let Some((device, queue)) = pollster::block_on(headless_device()) else {
            eprintln!("proteus-runtime: no GPU adapter available — skipping");
            return;
        };
        assert!(crate::config::FontSource::from_bytes(vec![1u8, 2, 3]).is_err());

        let mut proteus = Proteus::new();
        let mut config = crate::ProteusConfig::default();
        config.text.default_font = crate::config::FontSource::Bytes(Arc::from(vec![1u8, 2, 3]));
        let mut renderer = crate::Renderer::new(
            &mut proteus,
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            crate::Viewport::new(glam::Vec2::new(64.0, 64.0), 1.0),
            config,
        );
        let label = proteus.component(
            proteus_sdk::ComponentSpec::new(proteus_ui::QuadState::default())
                .text(proteus_ui::Text::new("Hi", 16.0)),
        );
        let target = device.create_texture(&wgpu::TextureDescriptor {
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
        });
        renderer.render(&mut proteus, &target.create_view(&Default::default()));

        assert!(
            label.baked_text_size(&proteus).is_some(),
            "the text was drawn, in the embedded font"
        );
    }
}
