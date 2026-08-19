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
    /// Always attaches `Interactable` (cheap; means `.on_click`/etc. work on
    /// any component without a separate opt-in). `InteractionDef` is only
    /// attached when `spec` declared at least one style override — otherwise
    /// there is nothing for `interaction_style_system` to resolve, and
    /// skipping it avoids a permanently-no-op mini-transition firing on every
    /// hover/press.
    pub fn component(&mut self, spec: ComponentSpec) -> Handle {
        let geometry = spec.geometry.clone();
        let entity = self
            .world
            .world
            .spawn((geometry.clone(), DeclaredGeometry(geometry), Interactable))
            .id();

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
            self.world.world.entity_mut(child.0).insert(ChildOf(entity));
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

impl Default for Proteus {
    fn default() -> Self {
        Self::new()
    }
}
