//! [`Proteus`] — the top-level app object. Owns the `proteus-ui` world and
//! the callback registry; every `Handle`/`SignalHandle` method takes it
//! explicitly (see `handle.rs`'s top doc for why).

use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::Component;
use glam::Vec2;

use proteus_render::TextureId;
use proteus_ui::{
    create_signal, ActiveTransition, Baked, EffectiveVisibility, Interactable, InteractionDef,
    ProteusWorld, QuadState, Visibility,
};

use crate::callback::{self, CallbackRegistry};
use crate::data::{ComponentData, TransitionData};
use crate::handle::{Handle, SignalHandle, TextureHandle};
use crate::spec::ComponentSpec;

/// Captured by [`Proteus::component`] at creation time — the declared rest
/// geometry `signal().set()` resolves an entity's target to automatically,
/// since nothing else in `proteus-ui` stores a "declared" state separately
/// from an entity's live (possibly mid-transition) `QuadState`. Private to
/// this crate; not part of the public API.
#[derive(Component, Debug, Clone)]
pub(crate) struct DeclaredGeometry(pub QuadState);

/// The top-level Proteus application. One instance per app; wraps a
/// [`ProteusWorld`] (the `proteus-ui` ECS runtime) plus this crate's own
/// callback dispatch.
pub struct Proteus {
    pub(crate) world: ProteusWorld,
    pub(crate) callbacks: CallbackRegistry,
}

impl Proteus {
    pub fn new() -> Self {
        Self {
            world: ProteusWorld::new(),
            callbacks: CallbackRegistry::default(),
        }
    }

    /// Escape hatch to the underlying `bevy_ecs::World` — for `proteus-ui`
    /// primitives this crate doesn't wrap yet (e.g. attaching
    /// `proteus_ui::component::Disabled`, or inserting `GpuContext`/
    /// `QuadPipeline` for baking/texture tests). Prefer the typed API above
    /// where it covers what you need.
    pub fn world_mut(&mut self) -> &mut bevy_ecs::world::World {
        &mut self.world.world
    }

    /// Read-only twin of [`Proteus::world_mut`].
    pub fn world(&self) -> &bevy_ecs::world::World {
        &self.world.world
    }

    /// Declare a new component from `spec`, returning its [`Handle`].
    ///
    /// Attaches `Interactable` by default (cheap; means `.on_click`/etc.
    /// work on any component without a separate opt-in) unless `spec` opted
    /// out via `.non_interactive()` — see that method's doc for why passive
    /// chrome (e.g. a full-window background) needs to. `InteractionDef` is
    /// only attached when `spec` declared at least one style override —
    /// otherwise there is nothing for `interaction_style_system` to
    /// resolve, and skipping it avoids a permanently-no-op mini-transition
    /// firing on every hover/press.
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
            // A dead handle in `children` skips that child rather than
            // panicking the whole `component()` call — same contract as
            // `Handle::add_child`, which this is the declarative form of.
            // (Every other `entity_mut` in this function targets `entity`,
            // which was spawned three lines up and is always alive.)
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

    /// Register a new signal, optionally owned by `owner` — an owned signal
    /// is destroyed automatically when its owner is destroyed.
    pub fn signal(&mut self, owner: Option<Handle>) -> SignalHandle {
        SignalHandle(create_signal(&mut self.world.world, owner.map(|h| h.0)))
    }

    /// Wrap an already-registered texture id for inspection. See
    /// [`TextureHandle`]'s doc for why this doesn't construct/upload
    /// anything.
    pub fn texture(&self, id: TextureId) -> TextureHandle {
        TextureHandle(id)
    }

    /// Read a component's current state. `None` if `handle` no longer refers
    /// to a live entity.
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

        let children = world
            .get::<Children>(handle.0)
            .map(|c| c.iter().map(|&e| Handle(e)).collect())
            .unwrap_or_default();

        let transition = world
            .get::<ActiveTransition>(handle.0)
            .map(|active| TransitionData::from_active(active, geometry.clone()));

        Some(ComponentData {
            geometry,
            state,
            visible,
            children,
            transition,
        })
    }

    /// Advance one frame: runs the `proteus-ui` schedule, then dispatches
    /// this frame's interaction/signal-drop events to registered callbacks.
    ///
    /// **Callbacks run after the schedule, so anything one of them starts
    /// takes effect on the next tick.** A `signal.set` from inside an
    /// `on_click` handler queues a `TransitionRequest` that
    /// `transition_setup_system` has already run past this frame; the morph
    /// begins on the following `tick`. That is one frame — invisible at
    /// 60fps — and it is what makes dispatch re-entrant-safe, since a
    /// handler can mutate the world freely without racing a system that is
    /// mid-iteration. Pinned by
    /// `signal_set_from_inside_an_on_click_handler_starts_the_transition_next_tick`.
    ///
    /// Clears `just_pressed`/`just_released` after processing — callers only
    /// need to call `pointer_pressed`/`pointer_released` once per physical
    /// press/release, not remember to clear them again (a small ergonomic
    /// improvement over the raw `PointerInput` contract, whose own doc
    /// leaves that as the caller's responsibility).
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

        let dropped = self
            .world
            .world
            .resource_mut::<proteus_ui::DroppedSignals>()
            .drain();
        for d in dropped {
            callback::fire_dropped(self, d);
        }
    }

    /// Pointer position in **world-space** — origin at the viewport center,
    /// Y-up — matching `proteus_ui::PointerInput`'s contract exactly, *not*
    /// window/CSS pixels (origin top-left, Y-down). `Proteus` has no
    /// window-size concept of its own, so it can't do that conversion for
    /// you: a caller (native winit shell, wasm shell, or a test) converts
    /// first — `world_x = cursor_x - width/2`, `world_y = height/2 -
    /// cursor_y` — then passes the result here.
    pub fn pointer_moved(&mut self, pos: Option<Vec2>) {
        self.world
            .world
            .resource_mut::<proteus_ui::PointerInput>()
            .position = pos;
    }

    /// Re-runs just the `Visibility`→`EffectiveVisibility` and `Opacity`→
    /// `EffectiveOpacity` cascades — not the full per-frame `tick()` (no
    /// input/transition/bake re-run). Wraps
    /// `proteus_ui::ProteusWorld::refresh_cascades`, which isn't otherwise
    /// reachable: `Proteus` only exposes the raw `bevy_ecs::World` via
    /// `world`/`world_mut`, not the `ProteusWorld` wrapper itself.
    ///
    /// `tick()`'s own cascade pass (part of the normal schedule) reflects
    /// `Visibility`/`Opacity` as they stood *before* this frame's
    /// application logic ran. Application code that mutates `Visibility`
    /// directly after `tick()` returns (e.g. a state-machine `settle()` step
    /// hiding/revealing components) needs a second cascade pass before
    /// rendering, or the mutation renders one frame late — call this after
    /// all such per-frame mutations, immediately before reading instances
    /// for rendering.
    pub fn refresh_cascades(&mut self) {
        self.world.refresh_cascades();
    }

    pub fn pointer_pressed(&mut self) {
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_pressed = true;
        pointer.is_pressed = true;
    }

    pub fn pointer_released(&mut self) {
        let mut pointer = self.world.world.resource_mut::<proteus_ui::PointerInput>();
        pointer.just_released = true;
        pointer.is_pressed = false;
    }
}

impl Proteus {
    /// How many callbacks are currently registered, across every kind.
    /// Test-only: a leaked closure is otherwise invisible from outside.
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

    /// Destroying a component must drop the closures registered against it.
    ///
    /// The registry is keyed by `(Entity, EventKind)`, and `bevy_ecs` bumps an
    /// entity's generation on despawn — so a recycled index never collides with
    /// the dead key, nothing ever overwrote it, and nothing removed it. An app
    /// that destroys and rebuilds components grew without bound; until there's
    /// a way to *hide* a component, destroy-and-rebuild is the only way to swap
    /// a screen, so this is the normal path rather than an unusual one.
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

    /// `bevy_ecs` cascades a despawn to descendants, so a destroyed parent takes
    /// its children's callbacks with it too.
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

    /// A handler that destroys its own component is a real pattern ("this
    /// button dismisses the thing it belongs to"), and it races the
    /// take-call-put-back that makes dispatch re-entrant: the handlers are held
    /// *outside* the map during the call, where `destroy`'s pruning can't see
    /// them, so putting them back unconditionally resurrects them.
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

    /// Destroying a signal drops its `on_dropped` handlers, and destroying an
    /// entity that *owns* signals drops theirs too — `proteus-ui` already
    /// destroys owned signals on despawn, but that only clears its own registry,
    /// not this crate's separate handler map.
    #[test]
    fn destroying_signals_and_their_owners_forgets_dropped_handlers() {
        let mut app = Proteus::new();

        let unowned = app.signal(None);
        unowned.on_dropped(&mut app, |_, _| {});
        assert_eq!(app.callback_count(), 1);
        unowned.destroy(&mut app);
        assert_eq!(app.callback_count(), 0, "explicit signal destroy");

        let owner = app.component(ComponentSpec::new(QuadState::default()));
        let owned = app.signal(Some(owner));
        owned.on_dropped(&mut app, |_, _| {});
        assert_eq!(app.callback_count(), 1);
        owner.destroy(&mut app).unwrap();
        assert_eq!(
            app.callback_count(),
            0,
            "an owned signal's handlers go when its owner does"
        );
    }
}
