//! `proteus-shell-web` — Layer 3: WebGL2/WebGPU WASM shell.
//!
//! Compiles to a WASM module exposing a JavaScript-callable API.
//! Hosts a [`proteus_gpu::GpuContext`] on a browser `<canvas>` element
//! and drives the Proteus render + UI stack from the browser event loop.

use wasm_bindgen::prelude::*;

/// Entry point called from JavaScript to boot a Proteus application
/// on the given canvas element ID.
#[wasm_bindgen]
pub async fn proteus_init(canvas_id: String) -> Result<(), JsValue> {
    // Route Rust panics to console.error and log:: macros to console.log.
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    log::info!("Proteus initializing on canvas #{canvas_id}");

    // TODO M1: initialize GpuContext with canvas surface, then hand off
    // to proteus-render and proteus-ui.

    Ok(())
}
