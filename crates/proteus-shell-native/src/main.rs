//! `proteus-shell-native` — native desktop entry point.
//!
//! Creates a winit window, initializes a wgpu surface, and boots the
//! Proteus render + UI stack. Targets macOS (Metal), Linux (Vulkan),
//! and Windows (DX12 / Vulkan).

fn main() {
    env_logger::init();
    log::info!("Proteus native shell starting…");

    // TODO Phase E (Build): set up winit event loop, create GpuContext with
    // window surface, run render loop.
    println!("Proteus — M0 scaffold. Render loop coming in Phase E (Build).");
}
