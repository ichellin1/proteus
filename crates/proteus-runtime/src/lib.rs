//! Runs a Proteus app on a GPU surface: the contract between an app and the
//! host that runs it.
//!
//! | Type | Role |
//! |---|---|
//! | [`App`] | What an application implements: `setup` once, `update` every frame |
//! | [`Engine`] | Owns the [`Proteus`] app state and the [`Renderer`], and runs one frame at a time |
//! | [`Renderer`] | Bakes pending text and images, then draws everything in one pass |
//! | [`HostServices`] | Loads assets, fetches data and plays video for the app, per platform |
//! | [`ProteusConfig`] | Engine settings: memory, rendering, frame timing and more |
//!
//! A host is a crate rather than a trait: it owns the window or canvas, the GPU
//! surface (see [`GpuSurface`]), the platform's event loop, input and the
//! viewport, and provides its own `run` function. `proteus-host-winit` and
//! `proteus-host-web` are the two hosts. The host owns the [`Engine`], which
//! owns the [`Proteus`] state; the app owns only its own data.
//!
//! This crate re-exports what a host needs from the layers below it, including
//! `wgpu` and `glam`, so a host can depend on `proteus-runtime` alone.
//!
// DOC-REVIEW
//! # Examples
//!
//! An app with one button that grows when it's clicked. A host runs it:
//! `proteus_host_winit::run` natively, or `proteus_host_web::run` on the web.
//!
//! ```
//! use proteus_runtime::glam::Vec2;
//! use proteus_runtime::{App, Frame};
//! use proteus_sdk::{ComponentSpec, QuadState, TransitionConfig};
//!
//! struct GrowingButton;
//!
//! impl App for GrowingButton {
//!     fn setup(&mut self, f: &mut Frame) {
//!         let small = QuadState {
//!             size: Vec2::new(160.0, 48.0),
//!             ..QuadState::default()
//!         };
//!         let large = QuadState {
//!             size: Vec2::new(320.0, 96.0),
//!             ..small.clone()
//!         };
//!         let button = f.proteus.component(ComponentSpec::new(small));
//!         button.on_click(f.proteus, move |app| {
//!             let _ = button.animate_to(app, large.clone(), TransitionConfig::default());
//!         });
//!     }
//! }
//! ```

#![warn(missing_docs)]

mod app;
mod bake;
/// [`ProteusConfig`] and its sections.
pub mod config;
mod config_dto;
mod engine;
mod renderer;
mod services;
mod viewport;

pub use app::{App, Frame, PlayingVideo};
pub use config::ProteusConfig;
pub use config_dto::ProteusConfigDto;
pub use engine::Engine;
/// Re-exported so a host can check a config before passing it to
/// [`Renderer::new`], which panics on an invalid one. A host that takes config
/// from outside the program, such as `mount` from JavaScript, reports an error
/// instead.
pub use proteus_render::{validate_atlas_config, validate_render_config};
/// Re-exported from `proteus-sdk`.
pub use proteus_sdk::TextureRequest;
pub use renderer::Renderer;
pub use services::{FetchId, FetchResult, HostServices, VideoFrame, VideoStream};
pub use viewport::{Insets, Viewport};

// Re-exported so a host can depend on `proteus-runtime` alone, and uses the
// same `wgpu` and `glam` versions this crate does.
pub use glam;
pub use proteus_gpu::{GpuError, GpuSurface, SurfaceRequest};
pub use proteus_sdk::{Proteus, TextureHandle};
pub use wgpu;
