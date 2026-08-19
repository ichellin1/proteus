//! `proteus-demo` — the shared, shell-agnostic Proteus reference demo
//! (M12.5).
//!
//! Built once against [`proteus_sdk::Proteus`] and linked by both
//! `proteus-shell-native` and `proteus-shell-web`, replacing what was
//! previously ~17,500 lines of independently hand-duplicated demo logic
//! across the two shells. Content lands screen by screen across M12.5's
//! staged migration (see `PLANNING.md`'s M12.5 entry) — this crate is
//! currently just the scaffold: [`Demo`] wraps a [`proteus_sdk::Proteus`]
//! with no content of its own yet.
//!
//! ## What stays a shell concern
//!
//! Rendering (GPU device/surface setup, `collect_instances`, the actual
//! draw call) is **not** this crate's job — `proteus-sdk` itself is
//! headless, and this crate follows suit. A shell drives [`Demo`] with
//! [`Demo::tick`]/pointer input, then reads [`Demo::app`]'s `world()` to
//! render, exactly like any other `proteus-sdk` consumer. `examples/
//! native_preview.rs` is a minimal reference for how to wire this up — not
//! part of this crate's public API, just a `cargo run --example
//! native_preview -p proteus-demo` harness for visually confirming each
//! migration step as content lands.
//!
//! Per-platform asset loading (reading files from disk, `fetch()`-ing
//! images, decoding video) also stays a shell concern — later steps add
//! `set_*`/`take_*` injection points to [`Demo`] (mirroring
//! `proteus-shell-web`'s existing wasm-bindgen surface, generalized) that
//! each shell calls with bytes/frames it fetched its own way. See
//! `PLANNING.md`'s M12.5 entry for the full reasoning.

use glam::Vec2;

use proteus_sdk::Proteus;

/// The shared reference demo application. One instance per running demo.
pub struct Demo {
    app: Proteus,
}

impl Demo {
    pub fn new() -> Self {
        Self {
            app: Proteus::new(),
        }
    }

    /// Advance one frame.
    pub fn tick(&mut self, dt: f32) {
        self.app.tick(dt);
    }

    /// Pointer position in **world-space** (viewport-center origin, Y-up) —
    /// see [`proteus_sdk::Proteus::pointer_moved`]'s doc for the exact
    /// contract and the conversion a caller needs from window/CSS pixels.
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.app.pointer_moved(pos);
    }

    pub fn pointer_pressed(&mut self) {
        self.app.pointer_pressed();
    }

    pub fn pointer_released(&mut self) {
        self.app.pointer_released();
    }

    /// Read-only access to the underlying [`Proteus`] app — for reading
    /// component state (`Proteus::get`) or, from a shell, rendering via
    /// `Proteus::world()`.
    pub fn app(&self) -> &Proteus {
        &self.app
    }

    /// Mutable access to the underlying [`Proteus`] app — for the escape
    /// hatch (`Proteus::world_mut()`) this crate itself uses internally to
    /// attach components `ComponentSpec` doesn't cover yet, and that a
    /// shell needs for GPU resource setup (`GpuContext`/`QuadPipeline`) and
    /// calling `refresh_cascades()` before rendering.
    pub fn app_mut(&mut self) -> &mut Proteus {
        &mut self.app
    }
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}
