//! [`Engine`] — owns [`Proteus`] and the [`Renderer`], and drives one frame.
//!
//! *"`Proteus` ownership moves from the app to the host"* is this type: the
//! host holds an `Engine`, the `Engine` holds `Proteus`, and the application
//! is a `&mut dyn App` the engine calls into. [`Engine::new`] is also what
//! inserts `proteus_render::GpuContext` and `QuadPipeline` into the world
//! (the M12 shells did this themselves) so `bake_system` keeps working.

use glam::Vec2;

use proteus_sdk::Proteus;

use crate::app::App;
use crate::config::ProteusConfig;
use crate::renderer::Renderer;
use crate::services::HostServices;
use crate::viewport::Viewport;

/// See the module docs.
//
// M13.1 step 1: fields/signatures only. Bodies land in step 3 when the winit
// event loop is ported into `proteus-host-winit`.
#[allow(dead_code)]
pub struct Engine {
    proteus: Proteus,
    renderer: Renderer,
    viewport: Viewport,
}

impl Engine {
    /// Construct `Proteus` + [`Renderer`] together, wire them (insert the GPU
    /// resources into the world, build the projection), and run
    /// [`App::setup`] once.
    pub fn new(
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _surface_format: wgpu::TextureFormat,
        _viewport: Viewport,
        _config: ProteusConfig,
        _app: &mut dyn App,
        _services: &mut dyn HostServices,
    ) -> Self {
        todo!("M13.1 step 3: build Proteus + Renderer, insert GpuContext/QuadPipeline, run App::setup")
    }

    /// One frame: [`App::update`] → [`Proteus::tick`] → [`Renderer::render`]
    /// into `target` (a surface texture view the host has already acquired).
    pub fn frame(
        &mut self,
        _dt: f32,
        _target: &wgpu::TextureView,
        _app: &mut dyn App,
        _services: &mut dyn HostServices,
    ) {
        todo!("M13.1 step 3")
    }

    /// Report a new drawable area — forwards to the renderer's projection and
    /// updates the copy delivered to the app via [`Frame`](crate::Frame).
    pub fn resize(&mut self, _viewport: Viewport) {
        todo!("M13.1 step 3")
    }

    /// Read-only access to the underlying app object — for reading component
    /// state (`Proteus::get`) from a host or a test.
    pub fn proteus(&self) -> &Proteus {
        &self.proteus
    }

    // ── Input forwarding — the host calls these from its native event stream.
    //    Coordinates are world-space (viewport-center origin, Y-up); the host
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
