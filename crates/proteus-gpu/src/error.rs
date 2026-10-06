use thiserror::Error;

/// Why GPU setup failed.
#[derive(Debug, Error)]
pub enum GpuError {
    /// The platform's surface target couldn't be turned into a GPU surface.
    #[error("failed to create a GPU surface for this platform target: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),

    /// No GPU on this machine can draw to the surface.
    #[error("no GPU adapter supports this surface")]
    NoAdapter,

    /// The GPU was found, but a device couldn't be created on it, for example
    /// because it doesn't meet the requested limits.
    #[error("failed to create GPU device: {0}")]
    DeviceCreation(#[from] wgpu::RequestDeviceError),
}
