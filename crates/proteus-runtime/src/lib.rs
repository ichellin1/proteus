//! `proteus-runtime` — Layer 2.75: the engine that binds headless
//! [`proteus_sdk::Proteus`] to a GPU surface (M13.1).
//!
//! Everything through M12 left rendering *outside* the framework: the
//! `proteus-ui` schedule's render stage is a stub, and the real per-frame
//! loop (bake pending `Text`/`Image` → `collect_instances` → draw → present)
//! was hand-written and duplicated in `proteus-shell-native` and
//! `proteus-shell-web`, each welded 1:1 to a concrete `Demo`. This crate
//! breaks that apart into three contracts, plus a host crate per platform:
//!
//! ```text
//! Renderer       the render primitive — bake + collect + one draw pass into a handed-in target
//! Engine         owns Proteus + Renderer; one `frame()` = App::update → Proteus::tick → Renderer::render
//! App            what an application implements — `setup()` once, `update()` per frame
//! HostServices   per-platform asset fulfilment, handed to the App through `Frame`
//! ```
//!
//! A *host* is a crate, not a trait: it owns the surface/GPU (see
//! [`GpuSurface`]), the platform's native loop, input translation and the
//! viewport, and exposes its own `run()`. M13.1 sketched a `Host` trait for
//! this, but nothing ever dispatched through it — `Engine::new`/`frame` take
//! `device`/`queue`/`surface_format`/`viewport` as plain arguments, and the
//! web host never implemented it at all — so it was dropped rather than kept
//! as decoration.
//!
//! Ownership after M13.1 is **host → [`Engine`] → [`proteus_sdk::Proteus`]**;
//! the application owns only its own state and is a `dyn App` the engine
//! calls into.
//!
//! See `PLANNING.md` § M13.1 for the full design and the decisions behind it.
//!
//! ## Status: M13.5 — `ProteusConfig`'s shape locked
//!
//! `ProteusConfig` is now the single nested config surface (`memory` /
//! `render` / `frame` / `input` / `transitions` / `text` / `resources` /
//! `debug` — see [`config`]). Wired in this pass: all of `memory`,
//! `render.{clear_color,present_mode,power_preference}`, and
//! `frame.dt_clamp_secs`. Everything else is a real, documented field with a
//! safe default, plumbed in incrementally as later milestones touch that
//! area.

mod app;
mod bake;
pub mod config;
mod engine;
mod renderer;
mod services;
mod viewport;

pub use app::{App, Frame, PlayingVideo};
pub use config::ProteusConfig;
pub use engine::Engine;
pub use renderer::Renderer;
pub use services::{FetchId, FetchResult, HostServices, TextureRequest, VideoFrame, VideoStream};
pub use viewport::{Insets, Viewport};

// Re-exported so a host crate can depend on `proteus-runtime` alone and
// still name the handful of lower-layer types it legitimately touches —
// including `wgpu` and `glam` at the exact versions this crate builds
// against, so a host can never drift onto a mismatched copy.
pub use glam;
pub use proteus_gpu::{GpuError, GpuSurface, SurfaceRequest};
pub use proteus_sdk::{Proteus, TextureHandle};
pub use wgpu;
