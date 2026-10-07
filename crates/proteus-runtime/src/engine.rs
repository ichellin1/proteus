//! [`Engine`]: owns the [`Proteus`] state and the [`Renderer`], and runs one
//! frame at a time.
//!
//! ## Frame order
//!
//! ```text
//! Proteus::tick(dt)          input, channel requests, transitions, then callbacks
//! App::update(frame, dt)     the app reacts to this frame's events
//! Proteus::refresh_cascades  recompute visibility and opacity after update's changes
//! Renderer::render(target)   bake pending text and images, then draw
//! ```
//!
//! `App::update` runs after the tick so it can react to what just happened,
//! such as a click or a finished transition. A transition it starts begins on
//! the next frame.

use glam::Vec2;

use proteus_sdk::Proteus;

use crate::app::{App, Frame};
use crate::config::ProteusConfig;
use crate::renderer::Renderer;
use crate::services::HostServices;
use crate::viewport::Viewport;

/// Owns the [`Proteus`] state and the [`Renderer`], and runs one frame at a
/// time. A host creates one and calls [`Engine::frame`] every frame.
pub struct Engine {
    proteus: Proteus,
    renderer: Renderer,
    viewport: Viewport,
    /// [`crate::config::FrameConfig::dt_clamp_secs`], which [`Engine::frame`]
    /// applies.
    dt_clamp_secs: f32,
}

impl Engine {
    /// Creates the [`Proteus`] state and the [`Renderer`], then calls
    /// [`App::setup`].
    ///
    /// # Panics
    ///
    /// If `config`'s memory settings don't fit the device; see
    /// [`Renderer::new`].
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        viewport: Viewport,
        config: ProteusConfig,
        app: &mut dyn App,
        services: &mut dyn HostServices,
    ) -> Self {
        let dt_clamp_secs = config.frame.dt_clamp_secs;
        let mut proteus = Proteus::new();
        let renderer = Renderer::new(
            &mut proteus,
            device,
            queue,
            surface_format,
            viewport,
            config,
        );

        {
            let mut frame = Frame {
                proteus: &mut proteus,
                services,
                viewport,
            };
            app.setup(&mut frame);
        }

        Self {
            proteus,
            renderer,
            viewport,
            dt_clamp_secs,
        }
    }

    /// Runs one frame and draws it into `target`, a surface texture the host
    /// has acquired.
    ///
    /// 1. Clamps `dt` to [`crate::config::FrameConfig::dt_clamp_secs`].
    /// 2. Calls [`Proteus::tick`]: input, channel requests, transitions, then callbacks.
    /// 3. Calls [`App::update`], so the app can react to this frame's events.
    /// 4. Recomputes visibility and opacity, so changes `update` made are
    ///    drawn this frame.
    /// 5. Calls [`Renderer::render`]: bakes pending text and images, then
    ///    draws.
    ///
    /// A transition the app starts in `update` begins on the next frame.
    pub fn frame(
        &mut self,
        dt: f32,
        target: &wgpu::TextureView,
        app: &mut dyn App,
        services: &mut dyn HostServices,
    ) {
        let dt = dt.min(self.dt_clamp_secs);
        self.proteus.tick(dt);

        {
            let mut frame = Frame {
                proteus: &mut self.proteus,
                services,
                viewport: self.viewport,
            };
            app.update(&mut frame, dt);
        }

        self.proteus.refresh_cascades();
        self.renderer.render(&mut self.proteus, target);
    }

    /// Reports a new drawable area to the renderer and the app. The host
    /// resizes the GPU surface itself.
    pub fn resize(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.renderer.resize(&mut self.proteus, viewport);
    }

    /// The [`Proteus`] state, read-only, for a host or a test to inspect.
    pub fn proteus(&self) -> &Proteus {
        &self.proteus
    }

    // Input, reported by the host from its platform's events. Positions are
    // world units (origin at the viewport center, y up); the host converts
    // from window coordinates.

    /// Pointer moved to `pos`, or `None` when it leaves the surface.
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.proteus.pointer_moved(pos);
    }

    /// Records that the pointer was pressed.
    pub fn pointer_pressed(&mut self) {
        self.proteus.pointer_pressed();
    }

    /// Records that the pointer was released.
    pub fn pointer_released(&mut self) {
        self.proteus.pointer_released();
    }
}
