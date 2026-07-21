//! [`GpuContext`] — the top-level GPU handle for a Proteus application.

use wgpu::{Adapter, Device, Instance, Queue};

use crate::GpuError;

/// The primary GPU handle. Wraps a [`wgpu::Device`] and [`wgpu::Queue`]
/// along with surface configuration for the current window/canvas.
pub struct GpuContext {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
}

impl GpuContext {
    /// Request a GPU adapter and device. This is the entry point for all
    /// Proteus GPU work. Async to support both native (`pollster::block_on`)
    /// and WASM (`wasm_bindgen_futures::spawn_local`) callers.
    pub async fn new() -> Result<Self, GpuError> {
        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|_| GpuError::NoAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("proteus-gpu"),
                required_features: wgpu::Features::empty(),
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }
}
