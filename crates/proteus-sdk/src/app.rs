//! [`Proteus`], holds an app's components, channels and callbacks.

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::Component;
use glam::Vec2;

use proteus_ui::{
    create_channel, ActiveTransition, Baked, EffectiveOpacity, EffectiveVisibility, Interactable,
    InteractionDef, ProteusWorld, QuadState, Visibility,
};

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
}

impl Proteus {
    /// Returns a new instance of `Proteus`.
    pub fn new() -> Self {
        Self {
            world: ProteusWorld::new(),
            callbacks: CallbackRegistry::default(),
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

        if spec.has_interaction_styles() {
            self.world.world.entity_mut(entity).insert(InteractionDef {
                hover: spec.hover,
                pressed: spec.pressed,
                focused: spec.focused,
                disabled: spec.disabled,
            });
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

    /// Adds RGBA number of pixels to the atlas and returns a [`TextureHandle`] to them.
    ///
    /// `rgba` holds `width * height * 4` bytes. The pixels are on the GPU when
    /// this returns. For PNG or JPEG data, use [`Proteus::load_texture`].
    ///
    /// Make sure to attach the texture to a component with [`Handle::set_texture`] right
    /// away. Until a component uses it, it may be evicted to make room.
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
    /// Callbacks run after the update, so anything they start, such as a
    /// transition from an `on_click` handler, will start on the next tick.
    pub fn tick(&mut self, dt: f32) {
        self.world.update(dt);
        self.dispatch_events();

        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_pressed = false;
        pointer.just_released = false;
    }

    fn dispatch_events(&mut self) {
        let (clicked, hover_entered, hover_exited, pressed, released, focused, blurred, dragged) = {
            let ev = self.world.world.resource::<proteus_ui::InteractionEvents>();
            (
                ev.clicked.clone(),
                ev.hover_entered.clone(),
                ev.hover_exited.clone(),
                ev.pressed.clone(),
                ev.released.clone(),
                ev.focused.clone(),
                ev.blurred.clone(),
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
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.world
            .world
            .resource_mut::<proteus_ui::PointerInput>()
            .position = pos;
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

    /// Records that the pointer was pressed.
    pub fn pointer_pressed(&mut self) {
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_pressed = true;
        pointer.is_pressed = true;
    }

    /// Records that the pointer was released.
    pub fn pointer_released(&mut self) {
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_released = true;
        pointer.is_pressed = false;
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
