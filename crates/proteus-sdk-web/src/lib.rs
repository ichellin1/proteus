//! `proteus-sdk-web` — thin wasm-bindgen bridge over `proteus-sdk`'s generic
//! app API (M12.4).
//!
//! Every method here delegates straight to `proteus_sdk::Proteus`/`Handle`/
//! `SignalHandle`/`TextureHandle` — no new ECS logic lives in this crate.
//! Data (`ComponentSpec`, `ComponentData`, `TransitionConfig`, ...) crosses
//! the boundary as plain JS objects via `serde-wasm-bindgen` and this
//! crate's own [`dto`] conversions; see that module's top doc for why (and
//! for the entity-handle-as-`f64` precision note). Callback registration
//! (`.onClick` etc.) wraps a `js_sys::Function` in a Rust closure and passes
//! it straight into the matching `proteus-sdk` method — `proteus-sdk`'s
//! callback bounds don't require `Send`, so this needs no callback storage
//! of its own; M12.3's registry does the work.
//!
//! Coordinate space, units, and conventions here are **raw** — world-space
//! (center-origin, Y-up), radians, RGBA floats — matching `proteus-sdk`'s
//! own contracts exactly. Convenience conversions (degrees, hex colors,
//! top-left coordinates) are `ts/`'s hand-authored TypeScript layer's job,
//! not this crate's — see its `convert.ts`.
//!
//! Built with `wasm-pack build --target bundler` (not `--target web`, which
//! every other wasm-pack invocation in this repo uses for
//! `proteus-shell-web`'s zero-build-step demo page) — this crate ships as an
//! `npm install`able package for bundler-based projects instead.
//!
//! ## Handles: by reference, except where Rust itself consumes them
//!
//! `Handle`/`SignalHandle`/`TextureHandle` parameters are taken **by
//! reference** (`&Handle`) everywhere except `destroy`/`signalDestroy` —
//! this was not the original design and was caught by this milestone's own
//! Node smoke test, not assumed correct from a clean compile. wasm-bindgen
//! invalidates a JS-side object wrapper once it's passed *by value* into an
//! exported function, regardless of whether the wrapped Rust type derives
//! `Copy` — `Copy` only governs Rust-side semantics, not the JS↔wasm
//! ownership-transfer convention. Taking every non-consuming parameter by
//! value would mean a `Handle` returned by `component()` could only ever be
//! used in one single subsequent call (`onClick`, `get()`, ...) before
//! becoming permanently invalid — unusable for a handle meant to be kept
//! and reused across many frames. `destroy`/`signalDestroy` are the
//! deliberate exception: they mirror `proteus-sdk`'s own `Handle::destroy`/
//! `SignalHandle::destroy`, which take `self` by value in Rust too — the
//! entity is genuinely gone afterward, so consuming the JS wrapper is
//! correct there.
//!
//! ## Shared ownership (M13.2)
//!
//! `ProteusApp` wraps `Rc<RefCell<sdk::Proteus>>`, not a bare `sdk::Proteus`.
//! Before M13.2 this crate was headless and owned the only `Proteus` in
//! existence, so plain ownership was fine. Now `proteus-host-web` also needs
//! `&mut Proteus` every frame (to drive `Proteus::tick` and
//! `proteus_runtime::Renderer::render`) while the *same* `Proteus` is handed
//! to JS via [`ProteusApp`] — two independent owners of one mutable value is
//! exactly `Rc<RefCell<_>>`'s job. `ProteusApp` is `Clone` (clones the `Rc`,
//! not the data) so the host can keep its own handle after passing one to
//! JS's `setup(app)` callback. [`ProteusApp::from_shared`] /
//! [`ProteusApp::shared`] are the (non-`#[wasm_bindgen]`, Rust-only) seam a
//! host crate uses to construct one and get its `Rc` back out — see
//! `proteus-host-web`'s `JsDriver`.
//!
//! Every method borrows for the duration of the call only, never across a
//! JS re-entrant call — a callback that calls back into another `ProteusApp`
//! method (e.g. an `onClick` handler calling `.get()`) would otherwise
//! double-borrow the same `RefCell` and panic. `signal_set`/`on_click`/etc.
//! all drop their borrow before invoking `proteus-sdk`'s own callback
//! dispatch machinery for this reason — see each method's borrow scoping
//! below.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use proteus_sdk as sdk;

mod dto;
mod handle;

pub use handle::{Handle, JsSignalHandle as SignalHandle, JsTextureHandle as TextureHandle};

use dto::{ComponentDataDto, ComponentSpecDto, TransitionConfigDto, TransitionDroppedDto};

// ---------------------------------------------------------------------------
// Callback wrapping — js_sys::Function -> Rust closure
// ---------------------------------------------------------------------------

fn wrap_plain(cb: js_sys::Function) -> impl FnMut(&mut sdk::Proteus) + 'static {
    move |_app: &mut sdk::Proteus| {
        // Errors thrown by the JS callback are swallowed here — a thin
        // bridge concern for a later pass, not part of M12.4's DoD.
        let _ = cb.call0(&JsValue::NULL);
    }
}

fn wrap_drag(cb: js_sys::Function) -> impl FnMut(&mut sdk::Proteus, glam::Vec2) + 'static {
    move |_app: &mut sdk::Proteus, delta: glam::Vec2| {
        let _ = cb.call2(
            &JsValue::NULL,
            &JsValue::from_f64(delta.x as f64),
            &JsValue::from_f64(delta.y as f64),
        );
    }
}

fn wrap_dropped(
    cb: js_sys::Function,
) -> impl FnMut(&mut sdk::Proteus, proteus_ui::TransitionDropped) + 'static {
    move |_app: &mut sdk::Proteus, dropped: proteus_ui::TransitionDropped| {
        let dto = TransitionDroppedDto::from(&dropped);
        if let Ok(js_val) = serde_wasm_bindgen::to_value(&dto) {
            let _ = cb.call1(&JsValue::NULL, &js_val);
        }
    }
}

// ---------------------------------------------------------------------------
// ProteusApp
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone)]
pub struct ProteusApp(Rc<RefCell<sdk::Proteus>>);

/// Rust-only seam for a host crate (`proteus-host-web`) — not part of the JS
/// API surface. See the module doc's "Shared ownership" section.
impl ProteusApp {
    /// Wrap an already-shared `Proteus` — used when a host (not this crate)
    /// owns the canonical `Rc` and needs to hand JS its own reference-counted
    /// view of the same value.
    pub fn from_shared(inner: Rc<RefCell<sdk::Proteus>>) -> Self {
        ProteusApp(inner)
    }

    /// The underlying shared `Proteus`, for a host to drive `tick()` /
    /// `refresh_cascades()` / rendering directly, outside the methods this
    /// type exposes to JS.
    pub fn shared(&self) -> Rc<RefCell<sdk::Proteus>> {
        self.0.clone()
    }
}

#[wasm_bindgen]
impl ProteusApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ProteusApp {
        ProteusApp(Rc::new(RefCell::new(sdk::Proteus::new())))
    }

    /// `spec` is a plain JS object matching the `ComponentSpec` TS
    /// interface — see `ts/src/types.ts`. `spec.children`, if present, are
    /// `Handle.id()` values (this bridge accepts raw ids here rather than
    /// opaque `Handle` objects nested inside the spec, since the whole spec
    /// deserializes through one `serde-wasm-bindgen` call).
    #[wasm_bindgen]
    pub fn component(&mut self, spec: JsValue) -> Result<Handle, JsValue> {
        let dto: ComponentSpecDto = serde_wasm_bindgen::from_value(spec)
            .map_err(|e| JsValue::from_str(&format!("invalid ComponentSpec: {e}")))?;
        let (mut spec, children_bits) = dto.into_spec_without_children();
        for bits in children_bits {
            let entity = bevy_ecs::prelude::Entity::from_bits(bits as u64);
            spec = spec.child(sdk::Handle::from_entity(entity));
        }
        Ok(Handle(self.0.borrow_mut().component(spec)))
    }

    /// `owner`, if present, is a `Handle.id()` value — not an opaque `Handle`
    /// object. wasm-bindgen doesn't support `Option<&CustomStruct>`
    /// parameters (confirmed by trying; `OptionFromWasmAbi` isn't
    /// implemented for reference types), and taking `Option<Handle>` by
    /// value would consume the caller's `Handle` on every owned-signal
    /// creation — same id-based workaround `ComponentSpec.children` already
    /// uses for the same reason.
    #[wasm_bindgen]
    pub fn signal(&mut self, owner: Option<f64>) -> SignalHandle {
        let owner = owner.map(|bits| {
            sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64))
        });
        SignalHandle(self.0.borrow_mut().signal(owner))
    }

    #[wasm_bindgen(js_name = signalSet)]
    pub fn signal_set(
        &mut self,
        signal: &SignalHandle,
        to: &Handle,
        from: &Handle,
        config: JsValue,
        interruptible: bool,
    ) -> Result<(), JsValue> {
        let dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        signal.0.set(
            &mut self.0.borrow_mut(),
            to.0,
            from.0,
            (&dto).into(),
            interruptible,
        );
        Ok(())
    }

    /// Consumes `signal` — matches `proteus-sdk`'s own `SignalHandle::destroy`,
    /// which takes `self` by value. Further use of the JS `SignalHandle`
    /// object after this call is invalid (same as in Rust).
    #[wasm_bindgen(js_name = signalDestroy)]
    pub fn signal_destroy(&mut self, signal: SignalHandle) {
        signal.0.destroy(&mut self.0.borrow_mut());
    }

    #[wasm_bindgen(js_name = onDropped)]
    pub fn on_dropped(&mut self, signal: &SignalHandle, cb: js_sys::Function) {
        signal
            .0
            .on_dropped(&mut self.0.borrow_mut(), wrap_dropped(cb));
    }

    #[wasm_bindgen]
    pub fn texture(&self, id: f64) -> TextureHandle {
        TextureHandle::from_id(id)
    }

    #[wasm_bindgen(js_name = textureState)]
    pub fn texture_state(&self, handle: &TextureHandle) -> JsValue {
        match handle.0.state(&self.0.borrow()) {
            Some((kind, width, height)) => {
                let dto = dto::TextureStateDto::from_state(kind, width, height);
                serde_wasm_bindgen::to_value(&dto).unwrap_or(JsValue::UNDEFINED)
            }
            None => JsValue::UNDEFINED,
        }
    }

    /// Returns `undefined` if `handle` no longer refers to a live component.
    #[wasm_bindgen]
    pub fn get(&self, handle: &Handle) -> JsValue {
        let Some(data) = self.0.borrow().get(handle.0) else {
            return JsValue::UNDEFINED;
        };
        let children_bits: Vec<f64> = data
            .children
            .iter()
            .map(|h| bevy_ecs::prelude::Entity::to_bits(h.id()) as f64)
            .collect();
        let dto = ComponentDataDto::from_data(&data, children_bits);
        serde_wasm_bindgen::to_value(&dto).unwrap_or(JsValue::UNDEFINED)
    }

    /// Advance one frame. A host that also owns this `ProteusApp`'s shared
    /// `Proteus` (via [`ProteusApp::shared`]) — as `proteus-host-web`'s
    /// `JsDriver` does — drives `tick`/`render` itself instead of calling
    /// this; it's here for headless/standalone use (Node smoke tests, an app
    /// with no host at all).
    #[wasm_bindgen]
    pub fn tick(&mut self, dt: f32) {
        self.0.borrow_mut().tick(dt);
    }

    /// `x`/`y` are **world-space** (viewport-center origin, Y-up) — not
    /// window/CSS pixels. See this crate's top doc.
    #[wasm_bindgen(js_name = pointerMoved)]
    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.0
            .borrow_mut()
            .pointer_moved(Some(glam::Vec2::new(x, y)));
    }

    /// Call when the pointer leaves the window/canvas — distinct from
    /// `pointerMoved`, since `proteus-sdk`'s contract represents "no
    /// position" as `None`, not a sentinel coordinate.
    #[wasm_bindgen(js_name = pointerLeft)]
    pub fn pointer_left(&mut self) {
        self.0.borrow_mut().pointer_moved(None);
    }

    #[wasm_bindgen(js_name = pointerPressed)]
    pub fn pointer_pressed(&mut self) {
        self.0.borrow_mut().pointer_pressed();
    }

    #[wasm_bindgen(js_name = pointerReleased)]
    pub fn pointer_released(&mut self) {
        self.0.borrow_mut().pointer_released();
    }

    #[wasm_bindgen(js_name = onClick)]
    pub fn on_click(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_click(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onHoverEnter)]
    pub fn on_hover_enter(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_hover_enter(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onHoverExit)]
    pub fn on_hover_exit(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_hover_exit(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onPress)]
    pub fn on_press(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_press(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onRelease)]
    pub fn on_release(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_release(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onFocus)]
    pub fn on_focus(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_focus(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onBlur)]
    pub fn on_blur(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_blur(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    #[wasm_bindgen(js_name = onDrag)]
    pub fn on_drag(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_drag(&mut self.0.borrow_mut(), wrap_drag(cb));
    }

    #[wasm_bindgen(js_name = addChild)]
    pub fn add_child(&mut self, parent: &Handle, child: &Handle) {
        parent.0.add_child(&mut self.0.borrow_mut(), child.0);
    }

    #[wasm_bindgen(js_name = removeChild)]
    pub fn remove_child(&mut self, parent: &Handle, child: &Handle, destroy: bool) {
        parent
            .0
            .remove_child(&mut self.0.borrow_mut(), child.0, destroy);
    }

    /// Consumes `handle` — matches `proteus-sdk`'s own `Handle::destroy`,
    /// which takes `self` by value. The entity is despawned; further use of
    /// the JS `Handle` object after this call is invalid (same as in Rust).
    #[wasm_bindgen]
    pub fn destroy(&mut self, handle: Handle) {
        handle.0.destroy(&mut self.0.borrow_mut());
    }

    #[wasm_bindgen(js_name = freeResources)]
    pub fn free_resources(&mut self, handle: &Handle) {
        handle.0.free_resources(&mut self.0.borrow_mut());
    }
}

impl Default for ProteusApp {
    fn default() -> Self {
        Self::new()
    }
}
