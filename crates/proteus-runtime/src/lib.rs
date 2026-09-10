//! `proteus-runtime` — Layer 2.75: the engine that binds headless
//! [`proteus_sdk::Proteus`] to a GPU surface (M13.1).
//!
//! Everything through M12 left rendering *outside* the framework: the
//! `proteus-ui` schedule's render stage is a stub, and the real per-frame
//! loop (bake pending `Text`/`Image` → `collect_instances` → draw → present)
//! was hand-written and duplicated in `proteus-shell-native` and
//! `proteus-shell-web`, each welded 1:1 to a concrete `Demo`. This crate
//! breaks that apart into four contracts:
//!
//! ```text
//! Renderer       the render primitive — bake + collect + one draw pass into a handed-in target
//! Engine         owns Proteus + Renderer; one `frame()` = App::update → Proteus::tick → Renderer::render
//! App            what an application implements — `setup()` once, `update()` per frame
//! Host           what a platform implements — surface/GPU, native loop, input, viewport
//! HostServices   per-platform asset fulfilment, handed to the App through `Frame`
//! ```
//!
//! Ownership after M13.1 is **host → [`Engine`] → [`proteus_sdk::Proteus`]**;
//! the application owns only its own state and is a `dyn App` the engine
//! calls into.
//!
//! See `PLANNING.md` § M13.1 for the full design and the decisions behind it.
//!
//! ## Status: M13.1 step 2 — `Renderer` implemented
//!
//! [`Renderer`] is real: it owns the font atlas, creates and world-inserts
//! the `QuadPipeline` / `GpuContext`, and does the bake + collect + draw
//! pass (lifted from `proteus-shell-native`). [`Engine`] method bodies are
//! still `todo!()` until step 3 ports the winit event loop into
//! `proteus-host-winit`.

mod app;
mod bake;
mod config;
mod engine;
mod host;
mod renderer;
mod services;
mod viewport;

pub use app::{App, Frame};
pub use config::ProteusConfig;
pub use engine::Engine;
pub use host::Host;
pub use renderer::Renderer;
pub use services::{HostServices, TextureRequest};
pub use viewport::{Insets, Viewport};

// Re-exported so a host crate can depend on `proteus-runtime` alone and
// still name the handful of lower-layer types it legitimately touches.
pub use proteus_sdk::{Proteus, TextureHandle};
