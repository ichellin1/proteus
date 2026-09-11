//! [`Engine`] — owns [`Proteus`] and the [`Renderer`], and drives one frame.
//!
//! *"`Proteus` ownership moves from the app to the host"* is this type: the
//! host holds an `Engine`, the `Engine` holds `Proteus`, and the application
//! is a `&mut dyn App` the engine calls into. [`Engine::new`] (via
//! [`Renderer::new`]) is also what inserts `proteus_render::GpuContext` and
//! `QuadPipeline` into the world (the M12 shells did this themselves) so
//! `bake_system` keeps working.
//!
//! ## Frame order
//!
//! ```text
//! Proteus::tick(dt)         run the ECS schedule — input, signals, transitions, cascades
//! App::update(frame, dt)    application reacts to this frame's events; may mutate the world directly
//! Proteus::refresh_cascades re-cascade Visibility/Opacity after any direct mutation in update()
//! Renderer::render(target)  bake pending Text/Image, collect, one draw pass
//! ```
//!
//! `App::update` runs **after** the schedule, not before: the M12 reference
//! demo's per-frame logic is reactive (it reads the click/transition events
//! the schedule just produced, then imperatively adjusts geometry), and a
//! "late update" hook models that directly. Signals fired from `update` are
//! picked up by `signal_dispatch_system` on the next frame, unchanged from
//! how `signal.set()` already behaves.

use glam::Vec2;

use proteus_sdk::Proteus;

use crate::app::{App, Frame};
use crate::config::ProteusConfig;
use crate::renderer::Renderer;
use crate::services::HostServices;
use crate::viewport::Viewport;

/// See the module docs.
pub struct Engine {
    proteus: Proteus,
    renderer: Renderer,
    viewport: Viewport,
}

impl Engine {
    /// Construct `Proteus` + [`Renderer`], wire them (the renderer inserts
    /// the GPU resources into the world and builds the projection), and run
    /// [`App::setup`] once.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        viewport: Viewport,
        config: ProteusConfig,
        app: &mut dyn App,
        services: &mut dyn HostServices,
    ) -> Self {
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
        }
    }

    /// One frame — see the module docs for the order. `target` is a surface
    /// texture view the host has already acquired.
    pub fn frame(
        &mut self,
        dt: f32,
        target: &wgpu::TextureView,
        app: &mut dyn App,
        services: &mut dyn HostServices,
    ) {
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

    /// Report a new drawable area — updates the renderer's projection and the
    /// copy delivered to the app via [`Frame`]. The host is separately
    /// responsible for reconfiguring the `wgpu::Surface`.
    pub fn resize(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.renderer.resize(&mut self.proteus, viewport);
    }

    /// Read-only access to the underlying app object — for reading component
    /// state (`Proteus::get`) from a host or a test.
    pub fn proteus(&self) -> &Proteus {
        &self.proteus
    }

    /// Mutable access to the underlying `Proteus` — for a host that still
    /// does per-frame work outside the `App` contract (M13.1: the native
    /// shell's video / gallery / texture-churn shims, pending M13.4).
    pub fn proteus_mut(&mut self) -> &mut Proteus {
        &mut self.proteus
    }

    // ── Input forwarding — the host calls these from its native event stream.
    //    Coordinates are world-space (viewport-centre origin, Y-up); the host
    //    does the window/CSS-pixel → world conversion, as the M12 shells do. ─

    /// Pointer moved to `pos`, or `None` when it leaves the surface.
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.proteus.pointer_moved(pos);
    }

    /// Primary button pressed.
    pub fn pointer_pressed(&mut self) {
        self.proteus.pointer_pressed();
    }

    /// Primary button released.
    pub fn pointer_released(&mut self) {
        self.proteus.pointer_released();
    }
}
