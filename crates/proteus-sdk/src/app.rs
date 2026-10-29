//! [`Proteus`], holds an app's components, channels and callbacks.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::Component;
use glam::Vec2;

use proteus_ui::{
    create_channel, AcceptsDevices, ActiveTransition, AppDevices, Baked, DefaultFocus,
    EffectiveOpacity, EffectiveVisibility, FocusBlocked, FocusState, InputDevice, InputDevices,
    InputMode, Interactable, InteractionDef, KeyEvent, KeyMap, KeyPressedEntity, KeyRepeat,
    KeyState, NavigatingActions, NavigationRequests, ProteusWorld, QuadState, TabWraps, Visibility,
};
use proteus_ui::{Action, AppAction, KeyBinding};

use proteus_render::{
    decode_image, resize_to_fit, DecodedImage, GpuContext, QuadPipeline, TextureId,
};

use crate::callback::{self, CallbackRegistry};
use crate::data::{ComponentData, TransitionData};
use crate::handle::{Handle, TextureHandle, TextureRequest, TransitionChannel};
use crate::spec::ComponentSpec;

/// A component's declared geometry. It is stored separately from the
/// component's `QuadState`, which changes during transitions.
#[derive(Component, Debug, Clone)]
pub(crate) struct DeclaredGeometry(pub QuadState);

/// An app's components, channels and callbacks.
///
/// An app normally uses a single instance of `Proteus`; on a host, the host
/// creates it and passes it to the app each frame. Separate instances are
/// independent, and a handle only works with the `Proteus` that created it.
/// Handle methods that read or change a component, channel or texture take
/// the `Proteus` as an argument.
pub struct Proteus {
    pub(crate) world: ProteusWorld,
    pub(crate) callbacks: CallbackRegistry,
    pub(crate) video: crate::video::VideoState,
}

impl Proteus {
    /// Returns a new instance of `Proteus`.
    pub fn new() -> Self {
        Self {
            world: ProteusWorld::new(),
            callbacks: CallbackRegistry::default(),
            video: crate::video::VideoState::default(),
        }
    }

    /// Returns a mutable reference to the underlying `bevy_ecs` world.
    ///
    /// Changes made here bypass this crate. For example, despawning an
    /// entity directly leaves its callbacks registered; use
    /// [`Handle::destroy`] instead.
    pub fn world_mut(&mut self) -> &mut bevy_ecs::world::World {
        &mut self.world.world
    }

    /// Returns a read-only reference to the underlying `bevy_ecs` world.
    pub fn world(&self) -> &bevy_ecs::world::World {
        &self.world.world
    }

    /// Creates a component from `spec` and returns its handle.
    ///
    /// A child in `spec` that no longer exists is skipped, with a warning.
    pub fn component(&mut self, spec: ComponentSpec) -> Handle {
        let geometry = spec.geometry.clone();
        let entity = self
            .world
            .world
            .spawn((geometry.clone(), DeclaredGeometry(geometry)))
            .id();
        if !spec.non_interactive {
            self.world.world.entity_mut(entity).insert(Interactable);
        }
        if spec.default_focus {
            self.world.world.entity_mut(entity).insert(DefaultFocus);
        }
        if let Some(devices) = spec.devices {
            self.world
                .world
                .entity_mut(entity)
                .insert(AcceptsDevices(devices));
        }

        if spec.has_interaction_styles() {
            // The style state starts here, rather than on the style system's
            // first tick, so that its resting geometry is the one declared
            // now, even if `animate_to` or `set_declared_geometry` changes it
            // before the first tick. It starts on `Default`, as on a first
            // tick; the next tick applies any style that already applies.
            let declared = spec.geometry.clone();
            self.world.world.entity_mut(entity).insert((
                InteractionDef {
                    hover: spec.hover,
                    pressed: spec.pressed,
                    focused: spec.focused,
                    disabled: spec.disabled,
                },
                proteus_ui::InteractionState {
                    current: proteus_ui::InteractionStateKind::Default,
                    declared,
                },
            ));
        }

        if spec.bake {
            self.world.world.entity_mut(entity).insert(Baked);
        }

        if !spec.visible {
            self.world
                .world
                .entity_mut(entity)
                .insert(proteus_ui::Visibility::HIDDEN);
        }

        if spec.start_disabled {
            self.world
                .world
                .entity_mut(entity)
                .insert(proteus_ui::Disabled);
        }

        if let Some(interaction) = spec.transition_interaction {
            self.world.world.entity_mut(entity).insert(interaction);
        }

        if let Some(opacity) = spec.opacity {
            self.world
                .world
                .entity_mut(entity)
                .insert(proteus_ui::Opacity(opacity));
        }

        if let Some(text) = spec.text {
            self.world.world.entity_mut(entity).insert(text);
        }
        if let Some(image) = spec.image {
            self.world.world.entity_mut(entity).insert(image);
        }
        if let Some(border) = spec.border {
            self.world.world.entity_mut(entity).insert(border);
        }
        if let Some(glow) = spec.glow {
            self.world.world.entity_mut(entity).insert(glow);
        }
        if let Some(drop_shadow) = spec.drop_shadow {
            self.world.world.entity_mut(entity).insert(drop_shadow);
        }

        for child in spec.children {
            // Skip a child that no longer exists, with a warning, rather than
            // failing the whole call. All other `entity_mut` calls are safe because
            // `entity` was created early in this scope.
            match self.world.world.get_entity_mut(child.0) {
                Ok(mut child_entity) => {
                    child_entity.insert(ChildOf(entity));
                }
                Err(_) => log::warn!(
                    "Proteus::component: child entity {:?} is no longer alive — not attached",
                    child.0
                ),
            }
        }

        Handle(entity)
    }

    /// Creates a transition channel, which transitions one component into
    /// another with [`TransitionChannel::set`].
    ///
    /// If `owner` is given, the channel is destroyed along with it.
    pub fn transition_channel(&mut self, owner: Option<Handle>) -> TransitionChannel {
        TransitionChannel(create_channel(&mut self.world.world, owner.map(|h| h.0)))
    }

    /// Wraps the ID of a texture that is already in the atlas.
    ///
    /// To add a texture, use [`Proteus::load_texture`] or
    /// [`Proteus::bake_texture`].
    pub fn texture(&self, id: TextureId) -> TextureHandle {
        TextureHandle(id)
    }

    /// Returns a snapshot of a component's current state, or `None` if it
    /// has been destroyed.
    pub fn get(&self, handle: Handle) -> Option<ComponentData> {
        let world = &self.world.world;
        let geometry = world.get::<QuadState>(handle.0)?.clone();

        let state = world
            .get::<proteus_ui::InteractionState>(handle.0)
            .map(|s| s.current)
            .unwrap_or_default();

        let visible = world
            .get::<EffectiveVisibility>(handle.0)
            .map(|v| v.0)
            .unwrap_or_else(|| {
                world
                    .get::<Visibility>(handle.0)
                    .map(|v| v.visible)
                    .unwrap_or(true)
            });

        let disabled = world.get::<proteus_ui::Disabled>(handle.0).is_some();

        let opacity = world
            .get::<EffectiveOpacity>(handle.0)
            .map(|o| o.0)
            .unwrap_or_else(|| {
                world
                    .get::<proteus_ui::Opacity>(handle.0)
                    .map(|o| o.0)
                    .unwrap_or(1.0)
            });

        let children = world
            .get::<Children>(handle.0)
            .map(|c| c.iter().map(|&e| Handle(e)).collect())
            .unwrap_or_default();

        // A change of interaction style, such as a hover effect, isn't
        // reported as a transition.
        let transition = world
            .get::<ActiveTransition>(handle.0)
            .filter(|active| !active.interaction_style)
            .map(|active| TransitionData::from_active(active, geometry.clone()));

        Some(ComponentData {
            geometry,
            state,
            disabled,
            visible,
            opacity,
            children,
            transition,
        })
    }

    /// Adds RGBA pixels to the atlas and returns a [`TextureHandle`] to them.
    ///
    /// `rgba` holds `width * height * 4` bytes. The pixels are on the GPU when
    /// this returns. For PNG or JPEG data, use [`Proteus::load_texture`].
    ///
    /// The texture is scaled down to `request.max_side` if set. If it is still
    /// larger than an atlas page ([`AtlasConfig::page_size`](proteus_render::AtlasConfig::page_size)),
    /// it is scaled down to fit the page, with a warning.
    ///
    /// Attach the texture to a component with [`Handle::set_texture`] in the
    /// same frame. From the next frame on, a texture that no component uses
    /// may be evicted to make room. Several textures can be added and then
    /// attached: none is evicted in the frame it was added.
    ///
    /// Returns a null handle if the atlas is full (this
    /// is logged) or if no GPU is set up, as in a headless test.
    /// A null handle will draw nothing.
    pub fn bake_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        request: TextureRequest,
    ) -> TextureHandle {
        let null = TextureHandle::from_texture_id(TextureId::default());

        let mut decoded = DecodedImage {
            width,
            height,
            rgba_pixels: rgba,
        };
        if let Some(cap) = request.max_side {
            decoded = resize_to_fit(decoded, cap);
        }

        let world = self.world_mut();
        let Some(queue) = world.get_resource::<GpuContext>().map(|g| g.queue.clone()) else {
            return null;
        };
        let Some(mut pipeline) = world.get_resource_mut::<QuadPipeline>() else {
            return null;
        };
        let max = pipeline.texture_registry.max_texture_side();
        if decoded.width > max || decoded.height > max {
            log::warn!(
                "bake_texture: {}x{} is larger than an atlas page ({max}x{max}); it is scaled \
                 down to fit",
                decoded.width,
                decoded.height,
            );
            decoded = resize_to_fit(decoded, max);
        }
        let Some(texture_id) = pipeline.texture_registry.register_static(
            decoded.width,
            decoded.height,
            request.eternal,
        ) else {
            log::warn!(
                "bake_texture: main_atlas full — could not register {}x{}",
                decoded.width,
                decoded.height,
            );
            return null;
        };
        let placement = pipeline
            .texture_registry
            .main_atlas_region(texture_id)
            .expect("just registered");
        pipeline.write_to_main_atlas(&queue, placement, &decoded.rgba_pixels);
        TextureHandle::from_texture_id(texture_id)
    }

    /// Decodes a PNG or JPEG image and adds it to the atlas.
    ///
    /// Use this when you already have the file's bytes. To load an asset by
    /// name through the host, use `proteus_runtime::Frame::load_texture`.
    ///
    /// Returns `None` if the bytes can't be decoded (the reason is logged).
    /// An image that decodes but doesn't fit returns a null handle
    pub fn load_texture(&mut self, bytes: &[u8], request: TextureRequest) -> Option<TextureHandle> {
        match decode_image(bytes) {
            Ok(decoded) => {
                Some(self.bake_texture(decoded.width, decoded.height, decoded.rgba_pixels, request))
            }
            Err(e) => {
                log::warn!("load_texture: could not decode {} bytes: {e}", bytes.len());
                None
            }
        }
    }

    /// Advances the app by `dt` seconds, then runs callbacks for the events
    /// that occurred.
    ///
    /// Key callbacks run first, before the update, for the keys reported since
    /// the last tick, so that navigation only moves focus with the keys none
    /// of them claimed. The other callbacks run after the update, so anything
    /// they start, such as a transition from an `on_click` handler, will start
    /// on the next tick.
    pub fn tick(&mut self, dt: f32) {
        self.dispatch_keys();
        self.world.update(dt);
        self.dispatch_events();

        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_pressed = false;
        pointer.just_released = false;
        let mut keys = self.world.world.resource_mut::<proteus_ui::KeyInput>();
        keys.clear_events();
        // Its repeats and long presses go to the next tick, with the keys the
        // host reports by then.
        keys.advance(dt);
        self.world
            .world
            .resource_mut::<NavigationRequests>()
            .0
            .clear();
    }

    /// Sends each key event not yet sent to the key callbacks: those of the
    /// focused component, then its ancestors', then the app's, until one claims
    /// it, and to the engine's default for its action if none does.
    fn dispatch_keys(&mut self) {
        while let Some(event) = self.next_key_event() {
            let mut handled = false;
            for component in self.key_chain() {
                if callback::fire_key(self, component.0, &event) {
                    handled = true;
                    break;
                }
            }
            if !handled && !callback::fire_app_key(self, &event) {
                self.apply_key_default(&event);
            }
        }
    }

    /// The next key event the key callbacks haven't had, with its action from
    /// the key map, marked as sent. For a language binding that runs key
    /// callbacks of its own, instead of [`Proteus::tick`].
    pub fn next_key_event(&mut self) -> Option<KeyEvent> {
        let world = &mut self.world.world;
        let i = world.resource::<proteus_ui::KeyInput>().dispatched;
        let action = {
            let input = world.resource::<proteus_ui::KeyInput>();
            let event = input.events.get(i)?;
            world.resource::<KeyMap>().action_for(event)
        };
        let mut input = world.resource_mut::<proteus_ui::KeyInput>();
        input.dispatched += 1;
        let event = &mut input.events[i];
        event.action = action;
        let event = event.clone();
        // The key that pressed a component releases it, whether or not a
        // callback claims the release, so it can't be left pressed.
        world.resource_mut::<KeyPressedEntity>().release_by(&event);
        let navigates =
            action.is_some_and(|a| world.resource::<NavigatingActions>().0.contains(&a));
        if event.state == KeyState::Pressed && navigates {
            self.set_input_mode(InputMode::Navigating);
        }
        Some(event)
    }

    fn set_input_mode(&mut self, mode: InputMode) {
        let mut current = self.world.world.resource_mut::<InputMode>();
        // Only on a change, so that change detection sees one.
        if *current != mode {
            *current = mode;
        }
    }

    /// Which style of input the app is in: pointing, after the pointer moved
    /// or was pressed, or navigating, after a key bound to one of the
    /// navigating actions was pressed. The focused style shows only while
    /// navigating, and the hover style only while pointing. Use it, for
    /// example, to show hints for one style of input or the other.
    pub fn input_mode(&self) -> InputMode {
        *self.world.world.resource::<InputMode>()
    }

    /// Sets the actions whose keys switch the app to navigating; see
    /// [`Proteus::input_mode`]. By default the six that move focus, so the
    /// arrows and Tab switch it, and Enter, Space, Escape and typing don't.
    pub fn set_navigating_actions(&mut self, actions: &[Action]) {
        self.world.world.resource_mut::<NavigatingActions>().0 = actions.to_vec();
    }

    /// The actions whose keys switch the app to navigating.
    pub fn navigating_actions(&self) -> &[Action] {
        &self.world.world.resource::<NavigatingActions>().0
    }

    /// The components a key event goes to, in order: the focused component,
    /// then its ancestors, nearest first, leaving out those that can't take
    /// keys, such as a disabled one. For a language binding that runs key
    /// callbacks of its own.
    pub fn key_chain(&self) -> Vec<Handle> {
        let world = &self.world.world;
        let mut chain = Vec::new();
        let mut next = world.resource::<FocusState>().focused;
        while let Some(entity) = next {
            if !world.entities().contains(entity) {
                break;
            }
            if proteus_ui::focus_blocked(world, entity).is_none() {
                chain.push(Handle(entity));
            }
            next = world.get::<ChildOf>(entity).map(|c| c.parent());
        }
        chain
    }

    /// Does what the engine does with a key event no callback claimed: queues
    /// a navigation action, when pressed or repeated, for navigation to move
    /// focus with; and on a press of [`Action::Activate`], presses and clicks
    /// the focused component, as the pointer does, until the key is released.
    /// For a language binding that runs key callbacks of its own.
    pub fn apply_key_default(&mut self, event: &KeyEvent) {
        let Some(action) = event.action else {
            return;
        };
        let world = &mut self.world.world;
        match event.state {
            KeyState::Pressed | KeyState::Repeated if action.moves_focus() => {
                world.resource_mut::<NavigationRequests>().0.push(action);
            }
            KeyState::Pressed if action == Action::Activate => {
                let focused = world.resource::<FocusState>().focused;
                if let Some(entity) = focused {
                    if proteus_ui::focus_blocked(world, entity).is_none() {
                        world
                            .resource_mut::<KeyPressedEntity>()
                            .press(entity, event.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// Calls `cb` with each key event that no component's key callback
    /// claimed, whether or not a component has focus. Return
    /// [`KeyResult::Handled`] to claim it, so the engine's default for its
    /// action doesn't happen.
    ///
    /// Use it for the app's own shortcuts, and for actions that don't belong
    /// to a component, such as [`Action::Back`](crate::Action::Back).
    pub fn on_key(&mut self, cb: impl FnMut(&mut Proteus, &KeyEvent) -> KeyResult + 'static) {
        self.callbacks.register_app_key(Box::new(cb));
    }

    /// The key map: which keys stand for which actions.
    pub fn key_map(&self) -> &KeyMap {
        self.world.world.resource::<KeyMap>()
    }

    /// Binds `binding` to `action`, replacing what it was bound to.
    pub fn bind(&mut self, binding: KeyBinding, action: Action) {
        self.world
            .world
            .resource_mut::<KeyMap>()
            .bind(binding, action);
    }

    /// Unbinds `binding`, so its key reaches callbacks without an action.
    /// Returns the action it was bound to, if any.
    pub fn unbind(&mut self, binding: &KeyBinding) -> Option<Action> {
        self.world.world.resource_mut::<KeyMap>().unbind(binding)
    }

    /// Puts back the key map's default bindings, removing all the others. See
    /// [`KeyMap::default_bindings`].
    pub fn reset_key_map(&mut self) {
        self.world.world.resource_mut::<KeyMap>().reset();
    }

    /// Sets whether [`Action::Next`] and [`Action::Previous`] wrap round from
    /// the last component to the first, and back. They do by default. The web
    /// host turns it off unless the config's `input.keep_tab` is set, so Tab
    /// can leave the canvas at either end.
    pub fn set_tab_wraps(&mut self, wraps: bool) {
        self.world.world.resource_mut::<TabWraps>().0 = wraps;
    }

    /// Whether [`Action::Next`] and [`Action::Previous`] wrap round.
    pub fn tab_wraps(&self) -> bool {
        self.world.world.resource::<TabWraps>().0
    }

    /// Sets how held keys repeat: by the engine, after a delay and at an
    /// interval of its own, the same on every platform, or as the system
    /// repeats them. The engine's, first after 250 ms then every 100 ms, by
    /// default.
    pub fn set_key_repeat(&mut self, repeat: KeyRepeat) {
        self.world
            .world
            .resource_mut::<proteus_ui::KeyInput>()
            .repeat = repeat;
    }

    /// How held keys repeat.
    pub fn key_repeat(&self) -> KeyRepeat {
        self.world.world.resource::<proteus_ui::KeyInput>().repeat
    }

    /// Sets how long a key is held before it's long-pressed, in milliseconds.
    /// 500 by default.
    pub fn set_long_press_ms(&mut self, ms: u32) {
        self.world
            .world
            .resource_mut::<proteus_ui::KeyInput>()
            .long_press_ms = ms;
    }

    /// How long a key is held before it's long-pressed, in milliseconds.
    pub fn long_press_ms(&self) -> u32 {
        self.world
            .world
            .resource::<proteus_ui::KeyInput>()
            .long_press_ms
    }

    /// Defines an action of the app's own, named `name`, to bind keys to and
    /// match in key callbacks, as `Action::App(search)`. Defining a name again
    /// returns the same action.
    pub fn define_action(&mut self, name: &str) -> AppAction {
        self.world
            .world
            .resource_mut::<KeyMap>()
            .define_action(name)
    }

    fn dispatch_events(&mut self) {
        let (
            clicked,
            hover_entered,
            hover_exited,
            pressed,
            released,
            focused,
            blurred,
            focus_lost,
            dragged,
        ) = {
            let ev = self.world.world.resource::<proteus_ui::InteractionEvents>();
            (
                ev.clicked.clone(),
                ev.hover_entered.clone(),
                ev.hover_exited.clone(),
                ev.pressed.clone(),
                ev.released.clone(),
                ev.focused.clone(),
                ev.blurred.clone(),
                ev.focus_lost.clone(),
                ev.dragged.clone(),
            )
        };

        for e in clicked {
            callback::fire(self, e, callback::EventKind::Click);
        }
        for e in hover_entered {
            callback::fire(self, e, callback::EventKind::HoverEnter);
        }
        for e in hover_exited {
            callback::fire(self, e, callback::EventKind::HoverExit);
        }
        for e in pressed {
            callback::fire(self, e, callback::EventKind::Press);
        }
        for e in released {
            callback::fire(self, e, callback::EventKind::Release);
        }
        for e in focused {
            callback::fire(self, e, callback::EventKind::Focus);
        }
        for e in blurred {
            callback::fire(self, e, callback::EventKind::Blur);
        }
        for (e, reason) in focus_lost {
            callback::fire_focus_lost(
                self,
                FocusLost {
                    component: Handle(e),
                    reason,
                },
            );
        }
        for (e, delta) in dragged {
            callback::fire_drag(self, e, delta);
        }

        // Read rather than drain: the next tick clears it, and code using
        // `world()` may want to see this tick's completions too.
        let completed = self
            .world
            .world
            .resource::<proteus_ui::CompletedTransitions>()
            .entities
            .clone();
        for e in completed {
            callback::fire(self, e, callback::EventKind::TransitionComplete);
        }

        let dropped = self
            .world
            .world
            .resource_mut::<proteus_ui::DroppedRequests>()
            .drain();
        for d in dropped {
            callback::fire_dropped(self, d);
        }
    }

    /// Updates the pointer position. Pass `None` when the pointer leaves the
    /// viewport.
    ///
    /// The position is in world units: the origin is the center of the
    /// viewport and y points up. To convert from window coordinates, use
    /// `x - width / 2` and `height / 2 - y`. Hosts do this conversion for you.
    ///
    /// Ignored while the app doesn't accept the pointer; see
    /// [`Proteus::set_input_devices`].
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        if !self.accepts(InputDevice::Pointer) {
            return;
        }
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        let moved = pos.is_some() && pos != pointer.position;
        pointer.position = pos;
        if moved {
            self.set_input_mode(InputMode::Pointing);
        }
    }

    /// Recomputes which components are visible and how opaque they are,
    /// without running a full tick.
    ///
    /// [`Proteus::tick`] computes these before app code runs, so a component
    /// hidden or faded after `tick` returns would be drawn with the old values
    /// for one frame. Call this after such changes and before rendering.
    /// Hosts call it for you after `App::update`.
    pub fn refresh_cascades(&mut self) {
        self.world.refresh_cascades();
    }

    /// Records that the pointer was pressed. Ignored while the app doesn't
    /// accept the pointer.
    pub fn pointer_pressed(&mut self) {
        if !self.accepts(InputDevice::Pointer) {
            return;
        }
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_pressed = true;
        pointer.is_pressed = true;
        self.set_input_mode(InputMode::Pointing);
    }

    /// Records that the pointer was released. Ignored while the app doesn't
    /// accept the pointer.
    pub fn pointer_released(&mut self) {
        if !self.accepts(InputDevice::Pointer) {
            return;
        }
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_released = true;
        pointer.is_pressed = false;
    }

    /// Records a key being pressed, repeated or released.
    ///
    /// A repeat or a release of a key that isn't held is ignored, so every
    /// release follows a press. Ignored while the app doesn't accept the
    /// keyboard.
    pub fn key_event(&mut self, event: KeyEvent) {
        if !self.accepts(InputDevice::Keyboard) {
            return;
        }
        log::trace!("{event:?}");
        self.world
            .world
            .resource_mut::<proteus_ui::KeyInput>()
            .record(event);
    }

    /// Releases every key still held. Hosts call this when the window or
    /// canvas loses focus, since the keys' own releases won't arrive.
    pub fn release_keys(&mut self) {
        self.world
            .world
            .resource_mut::<proteus_ui::KeyInput>()
            .release_all();
    }

    /// Sets which input devices the app accepts. Every device by default.
    ///
    /// Input from a device the app doesn't accept is ignored, before any
    /// component or callback sees it. Turning the pointer off ends its hover
    /// and releases its press; turning the keyboard off releases the keys
    /// held. A component can narrow the set further, with
    /// [`ComponentSpec::devices`].
    pub fn set_input_devices(&mut self, devices: InputDevices) {
        let before = self.input_devices();
        self.world.world.resource_mut::<AppDevices>().0 = devices;

        let dropped = |device| before.contains(device) && !devices.contains(device);
        if dropped(InputDevice::Pointer) {
            let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
            pointer.position = None;
            if pointer.is_pressed {
                pointer.is_pressed = false;
                pointer.just_released = true;
            }
        }
        if dropped(InputDevice::Keyboard) {
            self.release_keys();
        }
    }

    /// The input devices the app accepts.
    pub fn input_devices(&self) -> InputDevices {
        self.world.world.resource::<AppDevices>().0
    }

    fn accepts(&self, device: InputDevice) -> bool {
        self.input_devices().contains(device)
    }

    /// The component that has focus, if any.
    pub fn focused(&self) -> Option<Handle> {
        self.world
            .world
            .resource::<FocusState>()
            .focused
            .filter(|e| self.world.world.entities().contains(*e))
            .map(Handle)
    }

    /// Clears focus, so no component has it. Its `on_blur` fires on the next
    /// tick, and `on_focus_lost` doesn't, since the app cleared it.
    pub fn clear_focus(&mut self) {
        self.world.world.resource_mut::<FocusState>().set(None);
    }

    /// Calls `cb` each time a component loses focus because it can no longer
    /// keep it: it was destroyed, hidden, disabled, made non-interactive, or
    /// stopped accepting the keyboard. Not when the app clears focus or gives
    /// it to another component.
    ///
    /// Nothing has focus afterwards. This is where the app puts focus where it
    /// belongs, such as on the component that replaced the one lost.
    pub fn on_focus_lost(&mut self, cb: impl FnMut(&mut Proteus, FocusLost) + 'static) {
        self.callbacks.register_focus_lost(Box::new(cb));
    }
}

impl Proteus {
    /// The number of registered callbacks of every kind. For tests: a leaked
    /// callback is otherwise invisible.
    #[cfg(test)]
    pub(crate) fn callback_count(&self) -> usize {
        self.callbacks.len()
    }
}

/// Whether a key callback claimed a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    /// Claimed: the event goes no further, to other components, the app's key
    /// callbacks, or the engine's default for its action.
    Handled,
    /// Not claimed: the event goes on.
    Ignored,
}

/// A component that lost focus because it could no longer keep it; see
/// [`Proteus::on_focus_lost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusLost {
    /// The component that lost focus. It may no longer exist.
    pub component: Handle,
    /// Why it lost focus.
    pub reason: FocusBlocked,
}

impl Default for Proteus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ComponentSpec;

    // Destroying a component must drop its callbacks. A destroyed entity's ID
    // is never reused, so nothing else would remove them, and an app that
    // rebuilds components would grow without bound.
    #[test]
    fn destroying_a_component_forgets_its_callbacks() {
        let mut app = Proteus::new();
        assert_eq!(app.callback_count(), 0);

        let handle = app.component(ComponentSpec::new(QuadState::default()));
        handle.on_click(&mut app, |_| {});
        handle.on_hover_enter(&mut app, |_| {});
        handle.on_drag(&mut app, |_, _| {});
        assert_eq!(app.callback_count(), 3);

        handle.destroy(&mut app).unwrap();
        assert_eq!(
            app.callback_count(),
            0,
            "every handler registered against a destroyed component must go with it"
        );
    }

    // A destroyed parent takes its descendants with it, and their callbacks.
    #[test]
    fn destroying_a_parent_forgets_its_descendants_callbacks() {
        let mut app = Proteus::new();
        let grandchild = app.component(ComponentSpec::new(QuadState::default()));
        let child = app.component(ComponentSpec::new(QuadState::default()).child(grandchild));
        let parent = app.component(ComponentSpec::new(QuadState::default()).child(child));

        for h in [parent, child, grandchild] {
            h.on_click(&mut app, |_| {});
        }
        assert_eq!(app.callback_count(), 3);

        parent.destroy(&mut app).unwrap();
        assert_eq!(
            app.callback_count(),
            0,
            "descendants are despawned with the parent, so their handlers must go too"
        );
        assert!(app.get(grandchild).is_none(), "cascade really happened");
    }

    // A handler that destroys its own component (a dismiss button) must not
    // have its handlers put back after dispatch. They are out of the map while
    // it runs, where `destroy` can't remove them.
    #[test]
    fn a_callback_that_destroys_its_own_component_does_not_resurrect_its_handlers() {
        let mut app = Proteus::new();
        let handle = app.component(ComponentSpec::new(QuadState {
            position: glam::Vec3::new(0.0, 0.0, 0.0),
            size: glam::Vec2::new(100.0, 100.0),
            ..Default::default()
        }));
        handle.on_click(&mut app, move |app| {
            let _ = handle.destroy(app);
        });
        assert_eq!(app.callback_count(), 1);

        app.pointer_moved(Some(glam::Vec2::ZERO));
        app.pointer_pressed();
        app.tick(0.016);

        assert!(app.get(handle).is_none(), "the callback destroyed it");
        assert_eq!(
            app.callback_count(),
            0,
            "the handler must not be put back for an entity that can never fire again"
        );
    }

    // Destroying a channel drops its `on_dropped` handlers, and so does
    // destroying the component that owns it.
    #[test]
    fn destroying_channels_and_their_owners_forgets_dropped_handlers() {
        let mut app = Proteus::new();

        let unowned = app.transition_channel(None);
        unowned.on_dropped(&mut app, |_, _| {});
        assert_eq!(app.callback_count(), 1);
        unowned.destroy(&mut app);
        assert_eq!(app.callback_count(), 0, "explicit channel destroy");

        let owner = app.component(ComponentSpec::new(QuadState::default()));
        let owned = app.transition_channel(Some(owner));
        owned.on_dropped(&mut app, |_, _| {});
        assert_eq!(app.callback_count(), 1);
        owner.destroy(&mut app).unwrap();
        assert_eq!(
            app.callback_count(),
            0,
            "an owned channel's handlers go when its owner does"
        );
    }

    // Like a component's handler destroying its component: an `on_dropped`
    // handler that destroys its own channel must not have its handlers put
    // back, or they would fire again for a request on the stale handle.
    #[test]
    fn an_on_dropped_handler_that_destroys_its_own_channel_does_not_resurrect_its_handlers() {
        let mut app = Proteus::new();
        let to = app.component(ComponentSpec::new(QuadState::default()));
        // Hidden, so a transition from it is dropped.
        let from = app.component(ComponentSpec::new(QuadState::default()).visible(false));
        let channel = app.transition_channel(None);
        let fired = std::rc::Rc::new(std::cell::Cell::new(0));
        let clone = fired.clone();
        channel.on_dropped(&mut app, move |app, _| {
            clone.set(clone.get() + 1);
            channel.destroy(app);
        });

        channel.set(
            &mut app,
            to,
            from,
            crate::TransitionConfig::default(),
            false,
        );
        app.tick(0.016);
        assert_eq!(fired.get(), 1);
        assert_eq!(app.callback_count(), 0, "the handler went with its channel");

        channel.set(
            &mut app,
            to,
            from,
            crate::TransitionConfig::default(),
            false,
        );
        app.tick(0.016);
        assert_eq!(
            fired.get(),
            1,
            "and doesn't hear the stale handle's request"
        );
    }

    // Records each warning logged on the current thread, so a test can check
    // what was logged by the code it ran.
    struct WarningLog;

    thread_local! {
        static WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    impl log::Log for WarningLog {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }
        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                WARNINGS.with(|w| w.borrow_mut().push(record.args().to_string()));
            }
        }
        fn flush(&self) {}
    }

    fn take_warnings() -> Vec<String> {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_logger(&WarningLog).unwrap();
            log::set_max_level(log::LevelFilter::Warn);
        });
        WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
    }

    // `set` on a destroyed channel warns at once. Dispatch would drop it with
    // `ChannelNotFound` a tick later, but `destroy` has already removed the
    // `on_dropped` handlers that would hear it.
    #[test]
    fn set_on_a_destroyed_channel_is_ignored_with_a_warning() {
        take_warnings();
        let mut app = Proteus::new();
        let to = app.component(ComponentSpec::new(QuadState::default()).visible(false));
        let from = app.component(ComponentSpec::new(QuadState::default()));
        let channel = app.transition_channel(None);
        channel.destroy(&mut app);

        channel.set(
            &mut app,
            to,
            from,
            crate::TransitionConfig::default(),
            false,
        );
        let warnings = take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("TransitionChannel::set")),
            "warned at the call: {warnings:?}"
        );

        app.tick(1.0);
        assert!(!app.get(to).unwrap().visible, "and nothing ran");
    }

    // A handler registered while its event is being dispatched runs from the
    // next dispatch on, after the handlers registered before it.
    #[test]
    fn handlers_keep_their_registration_order_across_dispatch() {
        let mut app = Proteus::new();
        let handle = app.component(ComponentSpec::new(QuadState {
            size: glam::Vec2::new(100.0, 100.0),
            ..Default::default()
        }));
        let order = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        let log = order.clone();
        let registered = std::rc::Rc::new(std::cell::Cell::new(false));
        handle.on_click(&mut app, move |app| {
            log.borrow_mut().push("a");
            if !registered.replace(true) {
                let log = log.clone();
                handle.on_click(app, move |_| log.borrow_mut().push("c"));
            }
        });
        let log = order.clone();
        handle.on_click(&mut app, move |_| log.borrow_mut().push("b"));

        for _ in 0..2 {
            app.pointer_moved(Some(glam::Vec2::ZERO));
            app.pointer_pressed();
            app.tick(0.016);
            app.pointer_released();
            app.tick(0.016);
        }

        assert_eq!(*order.borrow(), ["a", "b", "a", "b", "c"]);
    }
}
