use thiserror::Error;

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("No suitable GPU adapter found")]
    NoAdapter,

    #[error("Failed to create GPU device: {0}")]
    DeviceCreation(#[from] wgpu::RequestDeviceError),

    #[error("Surface error: {0}")]
    Surface(#[from] wgpu::SurfaceError),
}
