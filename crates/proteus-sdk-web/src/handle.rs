//! `Handle`/`SignalHandle`/`TextureHandle` — opaque wasm-bindgen wrappers
//! around `proteus-sdk`'s identity tokens.
//!
//! Mutating operations (`.onClick`, `.addChild`, `.destroy`, ...) live on
//! [`crate::ProteusApp`], taking a handle argument, mirroring
//! `proteus-sdk`'s own `handle.on_click(&mut app, cb)` shape exactly (JS:
//! `app.onClick(handle, cb)`) — the raw bridge stays a faithful 1:1 mirror;
//! `ts/`'s hand-authored layer restores `button.onClick(cb)` ergonomics by
//! having its `Handle` class close over its owning `ProteusApp`.

use slotmap::Key;
use wasm_bindgen::prelude::*;

use proteus_render::TextureId;
use proteus_sdk as sdk;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct Handle(pub(crate) sdk::Handle);

#[wasm_bindgen]
impl Handle {
    /// This component's entity id, as `Entity::to_bits()` cast to `f64` —
    /// see `dto.rs`'s top doc for the precision note. Round-trips through
    /// [`Handle::from_id`].
    #[wasm_bindgen(js_name = id)]
    pub fn id(&self) -> f64 {
        bevy_ecs::prelude::Entity::to_bits(self.0.id()) as f64
    }

    /// Reconstruct a `Handle` from an id previously obtained from
    /// [`Handle::id`] or a `ComponentData.children` entry. Does not check
    /// the entity is still alive — exactly like holding onto any other stale
    /// `Handle` (methods become no-ops, `get()` returns `undefined`).
    #[wasm_bindgen(js_name = fromId)]
    pub fn from_id(id: f64) -> Handle {
        let entity = bevy_ecs::prelude::Entity::from_bits(id as u64);
        Handle(sdk::Handle::from_entity(entity))
    }
}

// ---------------------------------------------------------------------------
// SignalHandle
// ---------------------------------------------------------------------------

// No methods of its own yet — `SignalHandle` is currently only ever passed
// around opaquely (`app.signal()` → pass to `signalSet`/`signalDestroy`/
// `onDropped`). No `impl` block needed for a struct with no exposed methods.
#[wasm_bindgen(js_name = SignalHandle)]
#[derive(Clone, Copy)]
pub struct JsSignalHandle(pub(crate) sdk::SignalHandle);

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

#[wasm_bindgen(js_name = TextureHandle)]
#[derive(Clone, Copy)]
pub struct JsTextureHandle(pub(crate) sdk::TextureHandle);

#[wasm_bindgen(js_class = "TextureHandle")]
impl JsTextureHandle {
    /// This texture's id, as `KeyData::as_ffi()` cast to `f64` — the
    /// `slotmap` crate's own documented opaque-FFI-handle round-trip
    /// (`as_ffi`/`from_ffi`), same shape as `Handle`'s entity-bits
    /// conversion. Round-trips through [`JsTextureHandle::from_id`].
    #[wasm_bindgen(js_name = id)]
    pub fn id(&self) -> f64 {
        self.0.id().data().as_ffi() as f64
    }

    /// Reconstruct a `TextureHandle` from an id previously obtained from
    /// [`JsTextureHandle::id`]. Real texture *registration* (turning bytes
    /// into a `main_atlas` region) isn't exposed by this crate yet — see
    /// `proteus-sdk`'s own `TextureHandle` doc for why (M11's ref-counting
    /// is entity-scoped, not an independent resource).
    #[wasm_bindgen(js_name = fromId)]
    pub fn from_id(id: f64) -> JsTextureHandle {
        let key_data = slotmap::KeyData::from_ffi(id as u64);
        JsTextureHandle(sdk::TextureHandle::from_texture_id(TextureId::from(
            key_data,
        )))
    }
}
