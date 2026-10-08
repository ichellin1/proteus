//! JavaScript wrappers for `proteus-sdk`'s handles.
//!
//! Operations on a handle are methods of [`crate::ProteusApp`] that take the
//! handle as an argument (`app.onClick(handle, cb)`), mirroring
//! `proteus-sdk`. The TypeScript layer turns them into methods on the handle
//! (`button.onClick(cb)`).

use slotmap::Key;
use wasm_bindgen::prelude::*;

use proteus_render::TextureId;
use proteus_sdk as sdk;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// A handle to a component.
#[wasm_bindgen]
#[derive(Clone, Copy)]
pub struct Handle(pub(crate) sdk::Handle);

#[wasm_bindgen]
impl Handle {
    /// The component's ID: its entity's bits as a number, which is exact for
    /// any realistic app (see the `dto` module). [`Handle::from_id`] turns it
    /// back into a handle.
    #[wasm_bindgen(js_name = id)]
    pub fn id(&self) -> f64 {
        bevy_ecs::prelude::Entity::to_bits(self.0.id()) as f64
    }

    /// Returns a handle for an ID from [`Handle::id`] or
    /// `ComponentData.children`. The ID isn't checked: if its component has
    /// been destroyed, methods that change it throw and `get` returns
    /// `undefined`.
    #[wasm_bindgen(js_name = fromId)]
    pub fn from_id(id: f64) -> Handle {
        let entity = bevy_ecs::prelude::Entity::from_bits(id as u64);
        Handle(sdk::Handle::from_entity(entity))
    }
}

// ---------------------------------------------------------------------------
// TransitionChannel
// ---------------------------------------------------------------------------

/// A handle to a transition channel. It has no methods of its own; it is
/// passed to `channelSet`, `channelDestroy` and `onDropped`.
#[wasm_bindgen(js_name = TransitionChannel)]
#[derive(Clone, Copy)]
pub struct JsTransitionChannel(pub(crate) sdk::TransitionChannel);

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

/// A handle to a texture in the atlas.
#[wasm_bindgen(js_name = TextureHandle)]
#[derive(Clone, Copy)]
pub struct JsTextureHandle(pub(crate) sdk::TextureHandle);

#[wasm_bindgen(js_class = "TextureHandle")]
impl JsTextureHandle {
    /// The texture's ID, as a number. [`JsTextureHandle::from_id`] turns it
    /// back into a handle.
    #[wasm_bindgen(js_name = id)]
    pub fn id(&self) -> f64 {
        self.0.id().data().as_ffi() as f64
    }

    /// Returns a handle for an ID from [`JsTextureHandle::id`]. To add a
    /// texture, use `loadTexture` or `bakeTexture`.
    #[wasm_bindgen(js_name = fromId)]
    pub fn from_id(id: f64) -> JsTextureHandle {
        let key_data = slotmap::KeyData::from_ffi(id as u64);
        JsTextureHandle(sdk::TextureHandle::from_texture_id(TextureId::from(
            key_data,
        )))
    }
}

// ---------------------------------------------------------------------------
// VideoHandle
// ---------------------------------------------------------------------------

/// A handle to a video whose frames the app supplies. It has no methods of
/// its own; it is passed to `uploadVideoFrame`, `releaseVideo` and
/// `showVideo`.
#[wasm_bindgen(js_name = VideoHandle)]
#[derive(Clone, Copy)]
pub struct JsVideoHandle(pub(crate) sdk::VideoHandle);
