//! [`Host`] — what a platform integration crate implements.
//!
//! Internal-facing: an [`App`](crate::App) never names `Host`. It is
//! implemented once per platform — `proteus-host-winit` (M13.1 minimal /
//! M13.3 full), `proteus-host-web` (M13.2), and the M13.7 targets post-V1.
//!
//! A host is responsible for:
//!
//! - creating the `wgpu` surface + device/queue and choosing a surface format
//! - owning the platform's native run loop (winit's `ApplicationHandler`,
//!   the browser's `requestAnimationFrame`) — because both invert control,
//!   the shared driver is **not** a portable `loop {}`
//! - translating native input into [`Engine::pointer_moved`] /
//!   [`pointer_pressed`](Engine::pointer_pressed) /
//!   [`pointer_released`](Engine::pointer_released) (window/CSS-pixel →
//!   world-space conversion is the host's job, exactly as in the M12 shells)
//! - reporting the [`Viewport`] to [`Engine::resize`] on every size change
//! - acquiring each frame's surface texture, calling [`Engine::frame`], and
//!   presenting — plus `SurfaceError` / GPU-context-loss handling (M13.2)
//!
//! Each host crate additionally exposes a free `run<A: App>(app: A)` entry
//! point that constructs the host, builds an [`Engine`], and starts the loop.
//! That is not a trait method: its signature and control flow differ per
//! platform (native blocks, web returns and is driven by rAF).
//!
//! [`Engine`]: crate::Engine
//! [`Engine::frame`]: crate::Engine::frame
//! [`Engine::resize`]: crate::Engine::resize
//! [`Engine::pointer_moved`]: crate::Engine::pointer_moved

use crate::viewport::Viewport;

/// The accessors a host exposes to its own `run` driver and to [`Engine`]
/// construction. See the module docs for the full contract.
///
/// [`Engine`]: crate::Engine
pub trait Host {
    /// The GPU device, shared into the ECS world for bake systems.
    fn device(&self) -> &wgpu::Device;
    /// The GPU queue, paired with [`device`](Host::device).
    fn queue(&self) -> &wgpu::Queue;
    /// The swap-chain texture format the renderer must target.
    fn surface_format(&self) -> wgpu::TextureFormat;
    /// The current drawable area.
    fn viewport(&self) -> Viewport;
}
