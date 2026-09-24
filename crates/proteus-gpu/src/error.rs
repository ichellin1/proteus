use thiserror::Error;

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("failed to create a GPU surface for this platform target: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),

    #[error("no GPU adapter supports this surface")]
    NoAdapter,

    #[error("failed to create GPU device: {0}")]
    DeviceCreation(#[from] wgpu::RequestDeviceError),
}
