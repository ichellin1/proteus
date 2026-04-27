//! `proteus-shell-web` — Layer 4: WebGPU/WASM web shell.
//!
//! Compiles to a WASM module exposing a JavaScript-callable API.
//! Hosts a [`proteus_gpu::GpuContext`] on a browser `<canvas>` element
//! and drives the Proteus render + UI stack from the browser event loop.

use wasm_bindgen::prelude::*;

/// Entry point called from JavaScript to boot a Proteus application
/// on the given canvas element ID.
#[wasm_bindgen]
pub async fn proteus_init(canvas_id: String) -> Result<(), JsValue> {
    console_error_panic_hook();

    web_sys::console::log_1(&format!("Proteus initializing on canvas #{canvas_id}").into());

    // TODO Phase 1: initialize GpuContext with canvas surface, then hand off
    // to proteus-render and proteus-ui.

    Ok(())
}

fn console_error_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}
