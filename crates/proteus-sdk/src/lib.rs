//! `proteus-sdk` — Layer 2.5: the generic app-authoring API PLANNING.md's
//! Phase A designed, implemented in Rust for the first time (M12.3).
//!
//! ```text
//! Proteus::component(spec) -> Handle       — declare a component
//! Proteus::signal(owner)   -> SignalHandle — declare a signal
//! Proteus::texture(id)     -> TextureHandle — wrap an already-registered texture
//! Proteus::get(handle)     -> ComponentData — read current state
//! Proteus::tick(dt)                         — advance one frame, dispatch callbacks
//! ```
//!
//! ## Rust vs. the JS-oriented Phase A sketch
//!
//! Phase A's TypeScript sketch has handles capture behavior freely in
//! closures — `button.onClick(() => ...)` — relying on JS's implicit shared
//! mutable state. Rust has none, so [`Handle`]/[`SignalHandle`] stay thin
//! `Copy` identity tokens and their behavioral methods take `&mut Proteus`
//! explicitly: `button.on_click(&mut app, |app| { ... })`. This is the
//! idiomatic Rust adaptation, not a literal port.
//!
//! This crate is usable directly by native Rust apps (no wasm required) and
//! is the layer `proteus-sdk-web`'s wasm-bindgen bridge (M12.4) wraps 1:1 for
//! JS/TypeScript.

mod app;
mod callback;
mod data;
mod handle;
mod spec;

pub use app::Proteus;
pub use data::{ComponentData, TransitionData};
pub use handle::{Handle, SignalHandle, TextureHandle};
pub use spec::ComponentSpec;

// Re-exported so callers can build `QuadState`/`StyleOverride`/
// `TransitionConfig` values, pick an easing function, inspect a signal
// drop's reason, and construct the visual/content components
// `ComponentSpec::text`/`image`/`border`/`glow`/`drop_shadow` take — all
// without a direct `proteus-ui` dependency of their own.
pub use proteus_ui::{
    ease_in_out_quad, ease_in_quad, ease_out_cubic, ease_out_quad, linear, Border, DropReason,
    DropShadow, Glow, Image, InteractionStateKind, QuadState, StyleOverride, Text,
    TransitionConfig, TransitionDropped,
};
