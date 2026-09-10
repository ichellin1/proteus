//! [`App`] — what an application implements, and [`Frame`], the per-call
//! context it receives.

use std::sync::Arc;

use proteus_sdk::{Proteus, TextureHandle};

use crate::services::{HostServices, TextureRequest};
use crate::viewport::Viewport;

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
        crate::bake::load_texture(self.proteus.world_mut(), self.services, key, req)
    }
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
