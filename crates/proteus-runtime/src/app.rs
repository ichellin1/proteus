//! [`App`] — what an application implements, and [`Frame`], the per-call
//! context it receives.

use proteus_sdk::Proteus;

use crate::services::HostServices;
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

    /// Per-frame application logic, run before [`Proteus::tick`] advances the
    /// schedule. Optional — most apps wire everything with signals and
    /// callbacks in [`setup`](App::setup) and never implement this.
    /// `proteus-demo`'s `advance_*` steps land here.
    fn update(&mut self, f: &mut Frame, dt: f32) {
        let _ = (f, dt);
    }
}
