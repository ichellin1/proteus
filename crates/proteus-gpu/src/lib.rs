//! GPU setup for Proteus hosts.
//!
//! [`GpuSurface::create`] turns a platform's surface target, such as a winit
//! window or an HTML canvas, into a [`wgpu`] device, queue and configured swap
//! chain, the same way on every platform. What differs between platforms, the
//! device limits and the initial size, is passed in as a [`SurfaceRequest`].
//!
//! This crate depends on no windowing or browser crate: it only sees
//! `wgpu::SurfaceTarget`. Hosts use it through `proteus-runtime`, which
//! re-exports it.

#![warn(missing_docs)]

/// [`GpuSurface`] and [`SurfaceRequest`].
pub mod context;
/// [`GpuError`].
pub mod error;

pub use context::{GpuSurface, SurfaceRequest};
pub use error::GpuError;
