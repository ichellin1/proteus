//! `proteus-gpu` — platform GPU bring-up.
//!
//! One job: turn whatever surface target a platform hands over — an
//! `Arc<winit::Window>`, an `HtmlCanvasElement` — into a live [`wgpu`] device,
//! queue and configured swap chain, identically on every host.
//!
//! [`GpuSurface::create`] is the sequence `proteus-host-winit` and
//! `proteus-host-web` used to carry a copy of each: instance → surface →
//! adapter → device → surface-format choice → configure. Only the device
//! limits and the initial size genuinely differ per platform, and those are
//! inputs ([`SurfaceRequest`]) rather than forks.
//!
//! This crate has no UI opinion and no platform dependencies: it never names
//! winit or web-sys, only `wgpu::SurfaceTarget`, which both satisfy. Hosts
//! reach it through `proteus-runtime`'s re-export rather than depending on it
//! directly, keeping "a host depends on `proteus-runtime` alone" true.

pub mod context;
pub mod error;

pub use context::{GpuSurface, SurfaceRequest};
pub use error::GpuError;
