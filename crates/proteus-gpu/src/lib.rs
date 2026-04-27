//! `proteus-gpu` — Layer 0: GPU device abstraction.
//!
//! A thin, safe wrapper over [`wgpu`] that provides a single unified API
//! over WebGPU (web/WASM), Vulkan, Metal, and DirectX 12 (native).
//!
//! This crate has no opinion about user interfaces. It is a general-purpose
//! GPU runtime intended to serve as the foundation for `proteus-render`.

pub mod context;
pub mod error;

pub use context::GpuContext;
pub use error::GpuError;
