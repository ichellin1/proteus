//! The wasm bridge between the TypeScript SDK (`ts/`) and [`proteus_sdk`].
//!
//! Each method delegates to the matching `proteus-sdk` method; no engine logic
//! lives here. Values such as `ComponentSpec` and `TransitionConfig` cross as
//! plain JavaScript objects, converted by the `dto` module. The TypeScript
//! layer adds the conveniences: handle objects with methods, and unit
//! conversions. This crate is built with `wasm-pack --target bundler` and ships
//! inside the npm package.
//!
//! ## Handles are passed by reference
//!
//! `wasm-bindgen` invalidates a JavaScript object once it is passed by value
//! into Rust, even for a `Copy` type. So every method takes `&Handle`,
//! `&TransitionChannel` and `&TextureHandle`, which leaves the caller's object usable.
//! The exceptions are `destroy` and `channelDestroy`, which consume the handle,
//! as their `proteus-sdk` counterparts do. Lists of handles, and the optional
//! owner of a channel, cross as IDs instead, since `wasm-bindgen` can't take
//! references to them.
//!
//! ## Shared ownership
//!
//! [`ProteusApp`] wraps an `Rc<RefCell<proteus_sdk::Proteus>>`, because a host
//! (`proteus-host-web`) needs the same `Proteus` every frame to tick and render
//! it while JavaScript holds it too. [`ProteusApp::from_shared`] and
//! [`ProteusApp::shared`] are the Rust-only way a host creates one and gets the
//! `Rc` back.
//!
//! Each method borrows the `RefCell` only for its own call. The exception is
//! callback dispatch, which happens inside `tick` while it holds the borrow;
//! see `wrap_plain` for how callbacks avoid borrowing it a second time.

#![warn(missing_docs)]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use proteus_sdk as sdk;

mod dto;
mod handle;

pub use handle::{
    Handle, JsTextureHandle as TextureHandle, JsTransitionChannel as TransitionChannel,
    JsVideoHandle as VideoHandle,
};

use dto::{
    ComponentDataDto, ComponentSpecDto, ImageDto, MergeLayoutDto, QuadStateDto, SplitStrategyDto,
    TargetStateDto, TextDto, TransitionConfigDto, TransitionDroppedDto, Vec2Dto,
};

// ---------------------------------------------------------------------------
// Callback wrapping — js_sys::Function -> Rust closure
// ---------------------------------------------------------------------------

// `proteus-sdk` calls these closures during `tick`, while the `RefCell` around
// `Proteus` is borrowed. If the JavaScript callback ran right away and called
// back into any `ProteusApp` method, such as an `onClick` handler creating a
// component, that method would borrow the `RefCell` again and panic.
//
// So the callback is never called directly: `spawn_local` schedules it as a
// microtask, which runs once `tick` has returned and released the borrow.
// Microtasks run before the next animation frame, so callbacks still happen
// in the same frame.
//
// An exception thrown by a callback is logged with `report_throw`, as one
// thrown by `update` is, rather than discarded.
fn wrap_plain(cb: js_sys::Function) -> impl FnMut(&mut sdk::Proteus) + 'static {
    move |_app: &mut sdk::Proteus| {
        let cb = cb.clone();
        wasm_bindgen_futures::spawn_local(async move {
            report_throw(cb.call0(&JsValue::NULL));
        });
    }
}

fn wrap_drag(cb: js_sys::Function) -> impl FnMut(&mut sdk::Proteus, glam::Vec2) + 'static {
    move |_app: &mut sdk::Proteus, delta: glam::Vec2| {
        let cb = cb.clone();
        wasm_bindgen_futures::spawn_local(async move {
            report_throw(cb.call2(
                &JsValue::NULL,
                &JsValue::from_f64(delta.x as f64),
                &JsValue::from_f64(delta.y as f64),
            ));
        });
    }
}

fn wrap_dropped(
    cb: js_sys::Function,
) -> impl FnMut(&mut sdk::Proteus, proteus_ui::TransitionDropped) + 'static {
    move |_app: &mut sdk::Proteus, dropped: proteus_ui::TransitionDropped| {
        let cb = cb.clone();
        let dto = TransitionDroppedDto::from(&dropped);
        if let Ok(js_val) = serde_wasm_bindgen::to_value(&dto) {
            wasm_bindgen_futures::spawn_local(async move {
                report_throw(cb.call1(&JsValue::NULL, &js_val));
            });
        }
    }
}

/// Logs an exception thrown by a JavaScript callback, with its stack. The web
/// host sends the log to the browser console.
fn report_throw(result: Result<JsValue, JsValue>) {
    if let Err(e) = result {
        log::error!("proteus: a callback threw: {e:?}");
    }
}

// ---------------------------------------------------------------------------
// Error conversion
// ---------------------------------------------------------------------------

/// Calls a JavaScript `(index, total) => TransitionConfig` once per child. A
/// throw or an invalid return value becomes an `Err` naming the index.
fn resolve_child_behavior(
    f: &js_sys::Function,
    total: usize,
) -> Result<Vec<sdk::TransitionConfig>, JsValue> {
    (0..total)
        .map(|i| {
            let raw = f.call2(&JsValue::NULL, &(i as f64).into(), &(total as f64).into())?;
            let dto: TransitionConfigDto = serde_wasm_bindgen::from_value(raw).map_err(|e| {
                JsValue::from_str(&format!(
                    "childBehavior({i}, {total}) returned an invalid TransitionConfig: {e}"
                ))
            })?;
            Ok((&dto).into())
        })
        .collect()
}

/// Reads a `TextureRequest` (`{ maxSide?, eternal? }`) from JavaScript. An
/// absent or invalid value gives the default request, since its fields only
/// affect how the texture is stored.
fn texture_request_from(value: JsValue) -> sdk::TextureRequest {
    if value.is_null() || value.is_undefined() {
        return sdk::TextureRequest::default();
    }
    match serde_wasm_bindgen::from_value::<dto::TextureRequestDto>(value) {
        Ok(dto) => (&dto).into(),
        Err(e) => {
            log::warn!("invalid TextureRequest ({e}) — using defaults");
            sdk::TextureRequest::default()
        }
    }
}

/// Converts a [`HandleError`](sdk::HandleError) into the value thrown to
/// JavaScript. Like every error this bridge throws, it is a string rather than
/// an `Error` object.
fn handle_err(e: sdk::HandleError) -> JsValue {
    JsValue::from_str(&format!("proteus: {e}"))
}

// ---------------------------------------------------------------------------
// ProteusApp
// ---------------------------------------------------------------------------

/// A [`Proteus`](sdk::Proteus) app, as JavaScript sees it. Cloning shares the
/// same app.
#[wasm_bindgen]
#[derive(Clone)]
pub struct ProteusApp(Rc<RefCell<sdk::Proteus>>);

// Rust-only: for a host crate, not part of the JavaScript API.
impl ProteusApp {
    /// Wraps a `Proteus` that a host already shares, so JavaScript and the
    /// host use the same app.
    pub fn from_shared(inner: Rc<RefCell<sdk::Proteus>>) -> Self {
        ProteusApp(inner)
    }

    /// The shared `Proteus`, for a host to tick and render directly.
    pub fn shared(&self) -> Rc<RefCell<sdk::Proteus>> {
        self.0.clone()
    }
}

#[wasm_bindgen]
impl ProteusApp {
    /// Creates a standalone app, with no host, that draws nothing.
    #[wasm_bindgen(constructor)]
    pub fn new() -> ProteusApp {
        ProteusApp(Rc::new(RefCell::new(sdk::Proteus::new())))
    }

    /// Calls [`Proteus::component`](sdk::Proteus::component). `spec` is a
    /// `ComponentSpec` object; its `children` are handle IDs.
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

    /// Calls [`Proteus::transition_channel`](sdk::Proteus::transition_channel).
    /// `owner` is a handle ID, since `wasm-bindgen` can't take an optional
    /// reference.
    #[wasm_bindgen(js_name = transitionChannel)]
    pub fn transition_channel(&mut self, owner: Option<f64>) -> TransitionChannel {
        let owner = owner.map(|bits| {
            sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64))
        });
        TransitionChannel(self.0.borrow_mut().transition_channel(owner))
    }

    /// Calls [`TransitionChannel::set`](sdk::TransitionChannel::set).
    #[wasm_bindgen(js_name = channelSet)]
    pub fn channel_set(
        &mut self,
        channel: &TransitionChannel,
        to: &Handle,
        from: &Handle,
        config: JsValue,
        interruptible: bool,
    ) -> Result<(), JsValue> {
        let dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        channel.0.set(
            &mut self.0.borrow_mut(),
            to.0,
            from.0,
            (&dto).into(),
            interruptible,
        );
        Ok(())
    }

    /// Calls [`TransitionChannel::destroy`](sdk::TransitionChannel::destroy). Consumes
    /// `channel`: the JavaScript object can't be used afterwards.
    #[wasm_bindgen(js_name = channelDestroy)]
    pub fn channel_destroy(&mut self, channel: TransitionChannel) {
        channel.0.destroy(&mut self.0.borrow_mut());
    }

    /// Calls [`Handle::split_to`](sdk::Handle::split_to). The targets cross
    /// as handle IDs.
    #[wasm_bindgen(js_name = splitTo)]
    pub fn split_to(
        &mut self,
        handle: &Handle,
        target_ids: Vec<f64>,
        config: JsValue,
        strategy: JsValue,
    ) -> Result<(), JsValue> {
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        let strategy_dto: SplitStrategyDto = serde_wasm_bindgen::from_value(strategy)
            .map_err(|e| JsValue::from_str(&format!("invalid SplitStrategy: {e}")))?;
        let targets: Vec<sdk::Handle> = target_ids
            .into_iter()
            .map(|bits| sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64)))
            .collect();
        handle
            .0
            .split_to(
                &mut self.0.borrow_mut(),
                &targets,
                (&config_dto).into(),
                (&strategy_dto).into(),
            )
            .map_err(handle_err)
    }

    /// Calls [`Handle::merge_from`](sdk::Handle::merge_from). The sources
    /// cross as handle IDs.
    #[wasm_bindgen(js_name = mergeFrom)]
    pub fn merge_from(
        &mut self,
        handle: &Handle,
        source_ids: Vec<f64>,
        config: JsValue,
        layout: JsValue,
    ) -> Result<(), JsValue> {
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        let layout_dto: MergeLayoutDto = serde_wasm_bindgen::from_value(layout)
            .map_err(|e| JsValue::from_str(&format!("invalid MergeLayout: {e}")))?;
        let sources: Vec<sdk::Handle> = source_ids
            .into_iter()
            .map(|bits| sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64)))
            .collect();
        handle
            .0
            .merge_from(
                &mut self.0.borrow_mut(),
                &sources,
                (&config_dto).into(),
                (&layout_dto).into(),
            )
            .map_err(handle_err)
    }

    /// Calls [`Handle::split_to_with_behavior`](sdk::Handle::split_to_with_behavior).
    /// `child_behavior` is called once per target, before the split starts.
    #[wasm_bindgen(js_name = splitToWithBehavior)]
    pub fn split_to_with_behavior(
        &mut self,
        handle: &Handle,
        target_ids: Vec<f64>,
        config: JsValue,
        strategy: JsValue,
        child_behavior: js_sys::Function,
    ) -> Result<(), JsValue> {
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        let strategy_dto: SplitStrategyDto = serde_wasm_bindgen::from_value(strategy)
            .map_err(|e| JsValue::from_str(&format!("invalid SplitStrategy: {e}")))?;
        let targets: Vec<sdk::Handle> = target_ids
            .into_iter()
            .map(|bits| sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64)))
            .collect();
        let child_configs = resolve_child_behavior(&child_behavior, targets.len())?;
        handle
            .0
            .split_to_with_behavior(
                &mut self.0.borrow_mut(),
                &targets,
                (&config_dto).into(),
                (&strategy_dto).into(),
                |i, _total| child_configs[i],
            )
            .map_err(handle_err)
    }

    /// Calls [`Handle::merge_from_with_behavior`](sdk::Handle::merge_from_with_behavior).
    /// `child_behavior` is called once per source, before the merge starts.
    #[wasm_bindgen(js_name = mergeFromWithBehavior)]
    pub fn merge_from_with_behavior(
        &mut self,
        handle: &Handle,
        source_ids: Vec<f64>,
        config: JsValue,
        layout: JsValue,
        child_behavior: js_sys::Function,
    ) -> Result<(), JsValue> {
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        let layout_dto: MergeLayoutDto = serde_wasm_bindgen::from_value(layout)
            .map_err(|e| JsValue::from_str(&format!("invalid MergeLayout: {e}")))?;
        let sources: Vec<sdk::Handle> = source_ids
            .into_iter()
            .map(|bits| sdk::Handle::from_entity(bevy_ecs::prelude::Entity::from_bits(bits as u64)))
            .collect();
        let child_configs = resolve_child_behavior(&child_behavior, sources.len())?;
        handle
            .0
            .merge_from_with_behavior(
                &mut self.0.borrow_mut(),
                &sources,
                (&config_dto).into(),
                (&layout_dto).into(),
                |i, _total| child_configs[i],
            )
            .map_err(handle_err)
    }

    /// Calls [`Handle::split_to_with_states`](sdk::Handle::split_to_with_states).
    /// `targets` is an array of `{ id, state }` objects.
    #[wasm_bindgen(js_name = splitToWithStates)]
    pub fn split_to_with_states(
        &mut self,
        handle: &Handle,
        targets: JsValue,
        config: JsValue,
        strategy: JsValue,
    ) -> Result<(), JsValue> {
        let targets_dto: Vec<TargetStateDto> = serde_wasm_bindgen::from_value(targets)
            .map_err(|e| JsValue::from_str(&format!("invalid target state list: {e}")))?;
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        let strategy_dto: SplitStrategyDto = serde_wasm_bindgen::from_value(strategy)
            .map_err(|e| JsValue::from_str(&format!("invalid SplitStrategy: {e}")))?;
        let targets: Vec<(sdk::Handle, proteus_sdk::QuadState)> = targets_dto
            .iter()
            .map(|t| {
                let entity = bevy_ecs::prelude::Entity::from_bits(t.id as u64);
                (sdk::Handle::from_entity(entity), (&t.state).into())
            })
            .collect();
        handle
            .0
            .split_to_with_states(
                &mut self.0.borrow_mut(),
                &targets,
                (&config_dto).into(),
                (&strategy_dto).into(),
            )
            .map_err(handle_err)
    }

    /// Calls [`TransitionChannel::on_dropped`](sdk::TransitionChannel::on_dropped).
    #[wasm_bindgen(js_name = onDropped)]
    pub fn on_dropped(&mut self, channel: &TransitionChannel, cb: js_sys::Function) {
        channel
            .0
            .on_dropped(&mut self.0.borrow_mut(), wrap_dropped(cb));
    }

    /// Returns a texture handle for a texture ID.
    #[wasm_bindgen]
    pub fn texture(&self, id: f64) -> TextureHandle {
        TextureHandle::from_id(id)
    }

    /// Calls [`TextureHandle::state`](sdk::TextureHandle::state). Returns
    /// `undefined` if the texture has been evicted or never existed.
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

    /// Calls [`Proteus::get`](sdk::Proteus::get). Returns `undefined` if the
    /// component has been destroyed.
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

    /// Calls [`Proteus::tick`](sdk::Proteus::tick). For a standalone app only:
    /// a host ticks the shared `Proteus` itself.
    #[wasm_bindgen]
    pub fn tick(&mut self, dt: f32) {
        self.0.borrow_mut().tick(dt);
    }

    /// Calls [`Proteus::pointer_moved`](sdk::Proteus::pointer_moved). `x` and
    /// `y` are world units: origin at the viewport center, y up.
    #[wasm_bindgen(js_name = pointerMoved)]
    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.0
            .borrow_mut()
            .pointer_moved(Some(glam::Vec2::new(x, y)));
    }

    /// Records that the pointer left the viewport: `pointer_moved(None)`.
    #[wasm_bindgen(js_name = pointerLeft)]
    pub fn pointer_left(&mut self) {
        self.0.borrow_mut().pointer_moved(None);
    }

    /// Calls [`Proteus::pointer_pressed`](sdk::Proteus::pointer_pressed).
    #[wasm_bindgen(js_name = pointerPressed)]
    pub fn pointer_pressed(&mut self) {
        self.0.borrow_mut().pointer_pressed();
    }

    /// Calls [`Proteus::pointer_released`](sdk::Proteus::pointer_released).
    #[wasm_bindgen(js_name = pointerReleased)]
    pub fn pointer_released(&mut self) {
        self.0.borrow_mut().pointer_released();
    }

    /// Calls [`Handle::on_click`](sdk::Handle::on_click).
    #[wasm_bindgen(js_name = onClick)]
    pub fn on_click(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_click(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_transition_complete`](sdk::Handle::on_transition_complete).
    #[wasm_bindgen(js_name = onTransitionComplete)]
    pub fn on_transition_complete(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_transition_complete(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_hover_enter`](sdk::Handle::on_hover_enter).
    #[wasm_bindgen(js_name = onHoverEnter)]
    pub fn on_hover_enter(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_hover_enter(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_hover_exit`](sdk::Handle::on_hover_exit).
    #[wasm_bindgen(js_name = onHoverExit)]
    pub fn on_hover_exit(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_hover_exit(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_press`](sdk::Handle::on_press).
    #[wasm_bindgen(js_name = onPress)]
    pub fn on_press(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_press(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_release`](sdk::Handle::on_release).
    #[wasm_bindgen(js_name = onRelease)]
    pub fn on_release(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle
            .0
            .on_release(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_focus`](sdk::Handle::on_focus).
    #[wasm_bindgen(js_name = onFocus)]
    pub fn on_focus(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_focus(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_blur`](sdk::Handle::on_blur).
    #[wasm_bindgen(js_name = onBlur)]
    pub fn on_blur(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_blur(&mut self.0.borrow_mut(), wrap_plain(cb));
    }

    /// Calls [`Handle::on_drag`](sdk::Handle::on_drag). `cb` receives the
    /// movement as two numbers, `x` and `y`.
    #[wasm_bindgen(js_name = onDrag)]
    pub fn on_drag(&mut self, handle: &Handle, cb: js_sys::Function) {
        handle.0.on_drag(&mut self.0.borrow_mut(), wrap_drag(cb));
    }

    /// Calls [`Handle::add_child`](sdk::Handle::add_child).
    #[wasm_bindgen(js_name = addChild)]
    pub fn add_child(&mut self, parent: &Handle, child: &Handle) -> Result<(), JsValue> {
        parent
            .0
            .add_child(&mut self.0.borrow_mut(), child.0)
            .map_err(handle_err)
    }

    /// Calls [`Handle::remove_child`](sdk::Handle::remove_child).
    #[wasm_bindgen(js_name = removeChild)]
    pub fn remove_child(&mut self, parent: &Handle, child: &Handle) -> Result<(), JsValue> {
        parent
            .0
            .remove_child(&mut self.0.borrow_mut(), child.0)
            .map_err(handle_err)
    }

    /// Calls [`Handle::destroy`](sdk::Handle::destroy). Consumes `handle`: the
    /// JavaScript object can't be used afterwards.
    #[wasm_bindgen]
    pub fn destroy(&mut self, handle: Handle) -> Result<(), JsValue> {
        handle
            .0
            .destroy(&mut self.0.borrow_mut())
            .map_err(handle_err)
    }

    /// Calls [`Handle::free_resources`](sdk::Handle::free_resources).
    #[wasm_bindgen(js_name = freeResources)]
    pub fn free_resources(&mut self, handle: &Handle) -> Result<(), JsValue> {
        handle
            .0
            .free_resources(&mut self.0.borrow_mut())
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_text`](sdk::Handle::set_text).
    #[wasm_bindgen(js_name = setText)]
    pub fn set_text(&mut self, handle: &Handle, text: JsValue) -> Result<(), JsValue> {
        let dto: TextDto = serde_wasm_bindgen::from_value(text)
            .map_err(|e| JsValue::from_str(&format!("invalid TextSpec: {e}")))?;
        handle
            .0
            .set_text(&mut self.0.borrow_mut(), (&dto).into())
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_image`](sdk::Handle::set_image).
    #[wasm_bindgen(js_name = setImage)]
    pub fn set_image(&mut self, handle: &Handle, image: JsValue) -> Result<(), JsValue> {
        let dto: ImageDto = serde_wasm_bindgen::from_value(image)
            .map_err(|e| JsValue::from_str(&format!("invalid ImageSpec: {e}")))?;
        handle
            .0
            .set_image(&mut self.0.borrow_mut(), (&dto).into())
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_declared_geometry`](sdk::Handle::set_declared_geometry).
    #[wasm_bindgen(js_name = setDeclaredGeometry)]
    pub fn set_declared_geometry(
        &mut self,
        handle: &Handle,
        state: JsValue,
    ) -> Result<(), JsValue> {
        let dto: QuadStateDto = serde_wasm_bindgen::from_value(state)
            .map_err(|e| JsValue::from_str(&format!("invalid QuadState: {e}")))?;
        handle
            .0
            .set_declared_geometry(&mut self.0.borrow_mut(), (&dto).into())
            .map_err(handle_err)
    }

    /// Calls [`Handle::animate_to`](sdk::Handle::animate_to).
    #[wasm_bindgen(js_name = animateTo)]
    pub fn animate_to(
        &mut self,
        handle: &Handle,
        to: JsValue,
        config: JsValue,
    ) -> Result<(), JsValue> {
        let to_dto: QuadStateDto = serde_wasm_bindgen::from_value(to)
            .map_err(|e| JsValue::from_str(&format!("invalid QuadState: {e}")))?;
        let config_dto: TransitionConfigDto = serde_wasm_bindgen::from_value(config)
            .map_err(|e| JsValue::from_str(&format!("invalid TransitionConfig: {e}")))?;
        handle
            .0
            .animate_to(
                &mut self.0.borrow_mut(),
                (&to_dto).into(),
                (&config_dto).into(),
            )
            .map_err(handle_err)
    }

    /// Calls [`Handle::baked_text_size`](sdk::Handle::baked_text_size).
    /// Returns `undefined` if there is no baked text.
    #[wasm_bindgen(js_name = bakedTextSize)]
    pub fn baked_text_size(&self, handle: &Handle) -> JsValue {
        match handle.0.baked_text_size(&self.0.borrow()) {
            Some(size) => serde_wasm_bindgen::to_value(&Vec2Dto {
                x: size.x,
                y: size.y,
            })
            .unwrap_or(JsValue::UNDEFINED),
            None => JsValue::UNDEFINED,
        }
    }

    /// Calls [`Handle::baked_image_size`](sdk::Handle::baked_image_size).
    /// Returns `undefined` if there is no baked image.
    #[wasm_bindgen(js_name = bakedImageSize)]
    pub fn baked_image_size(&self, handle: &Handle) -> JsValue {
        match handle.0.baked_image_size(&self.0.borrow()) {
            Some(size) => serde_wasm_bindgen::to_value(&Vec2Dto {
                x: size.x,
                y: size.y,
            })
            .unwrap_or(JsValue::UNDEFINED),
            None => JsValue::UNDEFINED,
        }
    }

    /// Calls [`Handle::copy_baked_image_from`](sdk::Handle::copy_baked_image_from).
    #[wasm_bindgen(js_name = copyBakedImageFrom)]
    pub fn copy_baked_image_from(
        &mut self,
        handle: &Handle,
        source: &Handle,
    ) -> Result<bool, JsValue> {
        handle
            .0
            .copy_baked_image_from(&mut self.0.borrow_mut(), source.0)
            .map_err(handle_err)
    }

    /// Calls [`Handle::crop_image`](sdk::Handle::crop_image). Throws if
    /// `crop` isn't a valid `ImageCrop`.
    #[wasm_bindgen(js_name = cropImage)]
    pub fn crop_image(&mut self, handle: &Handle, crop: JsValue) -> Result<bool, JsValue> {
        let dto: dto::ImageCropDto = serde_wasm_bindgen::from_value(crop)
            .map_err(|e| JsValue::from_str(&format!("invalid ImageCrop: {e}")))?;
        handle
            .0
            .crop_image(&mut self.0.borrow_mut(), (&dto).into())
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_interactive`](sdk::Handle::set_interactive).
    #[wasm_bindgen(js_name = setInteractive)]
    pub fn set_interactive(&mut self, handle: &Handle, interactive: bool) -> Result<(), JsValue> {
        handle
            .0
            .set_interactive(&mut self.0.borrow_mut(), interactive)
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_visible`](sdk::Handle::set_visible).
    #[wasm_bindgen(js_name = setVisible)]
    pub fn set_visible(&mut self, handle: &Handle, visible: bool) -> Result<(), JsValue> {
        handle
            .0
            .set_visible(&mut self.0.borrow_mut(), visible)
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_opacity`](sdk::Handle::set_opacity).
    #[wasm_bindgen(js_name = setOpacity)]
    pub fn set_opacity(&mut self, handle: &Handle, opacity: f32) -> Result<(), JsValue> {
        handle
            .0
            .set_opacity(&mut self.0.borrow_mut(), opacity)
            .map_err(handle_err)
    }

    /// Calls [`Proteus::load_texture`](sdk::Proteus::load_texture). Returns
    /// `undefined` if the bytes can't be decoded.
    #[wasm_bindgen(js_name = loadTexture)]
    pub fn load_texture(&mut self, bytes: &[u8], request: JsValue) -> Option<TextureHandle> {
        let req = texture_request_from(request);
        self.0
            .borrow_mut()
            .load_texture(bytes, req)
            .map(TextureHandle)
    }

    /// Calls [`Proteus::bake_texture`](sdk::Proteus::bake_texture).
    #[wasm_bindgen(js_name = bakeTexture)]
    pub fn bake_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        request: JsValue,
    ) -> TextureHandle {
        let req = texture_request_from(request);
        TextureHandle(self.0.borrow_mut().bake_texture(width, height, rgba, req))
    }

    /// Calls [`Proteus::create_video`](sdk::Proteus::create_video).
    #[wasm_bindgen(js_name = createVideo)]
    pub fn create_video(&mut self) -> VideoHandle {
        VideoHandle(self.0.borrow_mut().create_video())
    }

    /// Calls [`VideoHandle::upload_frame`](sdk::VideoHandle::upload_frame).
    #[wasm_bindgen(js_name = uploadVideoFrame)]
    pub fn upload_video_frame(
        &mut self,
        video: &VideoHandle,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> bool {
        video
            .0
            .upload_frame(&mut self.0.borrow_mut(), width, height, rgba)
    }

    /// Calls [`VideoHandle::release`](sdk::VideoHandle::release).
    #[wasm_bindgen(js_name = releaseVideo)]
    pub fn release_video(&mut self, video: &VideoHandle) {
        video.0.release(&mut self.0.borrow_mut());
    }

    /// Calls [`Handle::show_video`](sdk::Handle::show_video).
    #[wasm_bindgen(js_name = showVideo)]
    pub fn show_video(&mut self, handle: &Handle, video: &VideoHandle) -> Result<bool, JsValue> {
        handle
            .0
            .show_video(&mut self.0.borrow_mut(), &video.0)
            .map_err(handle_err)
    }

    /// Calls [`Handle::hide_video`](sdk::Handle::hide_video).
    #[wasm_bindgen(js_name = hideVideo)]
    pub fn hide_video(&mut self, handle: &Handle) -> Result<(), JsValue> {
        handle
            .0
            .hide_video(&mut self.0.borrow_mut())
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_video_crossfade`](sdk::Handle::set_video_crossfade).
    #[wasm_bindgen(js_name = setVideoCrossfade)]
    pub fn set_video_crossfade(&mut self, handle: &Handle, video_t: f32) -> Result<bool, JsValue> {
        handle
            .0
            .set_video_crossfade(&mut self.0.borrow_mut(), video_t)
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_disabled`](sdk::Handle::set_disabled).
    #[wasm_bindgen(js_name = setDisabled)]
    pub fn set_disabled(&mut self, handle: &Handle, disabled: bool) -> Result<(), JsValue> {
        handle
            .0
            .set_disabled(&mut self.0.borrow_mut(), disabled)
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_transition_interaction`](sdk::Handle::set_transition_interaction).
    /// `null` or `undefined` restores the default.
    #[wasm_bindgen(js_name = setTransitionInteractionConfig)]
    pub fn set_transition_interaction(
        &mut self,
        handle: &Handle,
        config: JsValue,
    ) -> Result<(), JsValue> {
        let parsed = if config.is_null() || config.is_undefined() {
            None
        } else {
            let dto: dto::TransitionInteractionConfigDto = serde_wasm_bindgen::from_value(config)
                .map_err(|e| {
                JsValue::from_str(&format!("invalid TransitionInteractionConfig: {e}"))
            })?;
            Some((&dto).into())
        };
        handle
            .0
            .set_transition_interaction(&mut self.0.borrow_mut(), parsed)
            .map_err(handle_err)
    }

    /// Calls [`Handle::set_texture`](sdk::Handle::set_texture).
    #[wasm_bindgen(js_name = setTexture)]
    pub fn set_texture(
        &mut self,
        handle: &Handle,
        texture: &TextureHandle,
    ) -> Result<bool, JsValue> {
        handle
            .0
            .set_texture(&mut self.0.borrow_mut(), texture.0)
            .map_err(handle_err)
    }
}

impl Default for ProteusApp {
    fn default() -> Self {
        Self::new()
    }
}
