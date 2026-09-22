//! Callback storage and dispatch — private to this crate.
//!
//! `Handle`/`SignalHandle`'s `.on_*` methods register closures here;
//! [`Proteus::tick`](crate::Proteus::tick) dispatches them each frame by
//! reading that frame's `InteractionEvents`/`DroppedSignals` (both already
//! computed by the `proteus-ui` schedule `tick()` just ran).
//!
//! ## Persistence and re-entrancy
//!
//! Handlers are persistent — registering with `.on_click` fires on every
//! future click, not just the next one (matching the JS-API convention Phase
//! A sketches, `button.onClick(fn)`). Dispatch uses a take-call-put-back
//! pattern per key: the registered `Vec` is removed from the map before
//! calling any of it, so a callback that itself registers a new handler
//! (mutating the very map being iterated) doesn't conflict with an active
//! borrow. The original handlers are re-inserted afterward, merged with
//! anything newly registered during the call.

use std::collections::HashMap;

use bevy_ecs::prelude::Entity;
use glam::Vec2;

use proteus_ui::{SignalId, TransitionDropped};

use crate::Proteus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EventKind {
    Click,
    HoverEnter,
    HoverExit,
    Press,
    Release,
    Focus,
    Blur,
}

type PlainCallback = Box<dyn FnMut(&mut Proteus)>;
type DragCallback = Box<dyn FnMut(&mut Proteus, Vec2)>;
type DroppedCallback = Box<dyn FnMut(&mut Proteus, TransitionDropped)>;

#[derive(Default)]
pub(crate) struct CallbackRegistry {
    handlers: HashMap<(Entity, EventKind), Vec<PlainCallback>>,
    drag_handlers: HashMap<Entity, Vec<DragCallback>>,
    dropped_handlers: HashMap<SignalId, Vec<DroppedCallback>>,
}

impl CallbackRegistry {
    pub(crate) fn register(&mut self, entity: Entity, kind: EventKind, cb: PlainCallback) {
        self.handlers.entry((entity, kind)).or_default().push(cb);
    }

    /// Drop every handler registered against `entity`.
    ///
    /// Called when an entity is destroyed. Without this the closures stayed in
    /// the map forever: keys are `(Entity, EventKind)` and `bevy_ecs` bumps an
    /// entity's generation on despawn, so a recycled index never collides with
    /// the dead key and nothing ever overwrote it either. An app that destroys
    /// and rebuilds components — which, until there's a way to hide one, is the
    /// *only* way to swap a screen — grows without bound.
    pub(crate) fn forget_entity(&mut self, entity: Entity) {
        self.handlers.retain(|(e, _), _| *e != entity);
        self.drag_handlers.remove(&entity);
    }

    /// Drop every `on_dropped` handler registered against `signal`.
    pub(crate) fn forget_signal(&mut self, signal: SignalId) {
        self.dropped_handlers.remove(&signal);
    }

    /// Total registered handlers of every kind — the number that must come
    /// back down when components are destroyed. Test-facing observability;
    /// there is no other way to see the leak this guards against, since a
    /// leaked closure is invisible from outside.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.handlers.values().map(Vec::len).sum::<usize>()
            + self.drag_handlers.values().map(Vec::len).sum::<usize>()
            + self.dropped_handlers.values().map(Vec::len).sum::<usize>()
    }

    pub(crate) fn register_drag(&mut self, entity: Entity, cb: DragCallback) {
        self.drag_handlers.entry(entity).or_default().push(cb);
    }

    pub(crate) fn register_dropped(&mut self, signal: SignalId, cb: DroppedCallback) {
        self.dropped_handlers.entry(signal).or_default().push(cb);
    }
}

/// Fire every handler registered for `(entity, kind)` on `app`, then restore
/// them (plus anything newly registered mid-dispatch) for next time.
pub(crate) fn fire(app: &mut Proteus, entity: Entity, kind: EventKind) {
    let Some(mut cbs) = app.callbacks.handlers.remove(&(entity, kind)) else {
        return;
    };
    for cb in &mut cbs {
        cb(app);
    }
    // Not put back if the callback destroyed its own entity — a real pattern
    // ("this button dismisses the thing it belongs to"). The take-call-put-back
    // above holds the handlers *outside* the map for the duration of the call,
    // so `Handle::destroy`'s own pruning can't see them, and putting them back
    // unconditionally would resurrect handlers for an entity that can never
    // fire again.
    if is_alive(app, entity) {
        app.callbacks
            .handlers
            .entry((entity, kind))
            .or_default()
            .extend(cbs);
    }
}

/// Whether `entity` still exists — see [`fire`]'s put-back guard.
fn is_alive(app: &Proteus, entity: Entity) -> bool {
    app.world.world.entities().contains(entity)
}

/// Same shape as [`fire`], for the one event that carries a payload.
pub(crate) fn fire_drag(app: &mut Proteus, entity: Entity, delta: Vec2) {
    let Some(mut cbs) = app.callbacks.drag_handlers.remove(&entity) else {
        return;
    };
    for cb in &mut cbs {
        cb(app, delta);
    }
    if is_alive(app, entity) {
        app.callbacks
            .drag_handlers
            .entry(entity)
            .or_default()
            .extend(cbs);
    }
}

/// Same shape as [`fire`], for [`crate::SignalHandle::on_dropped`].
pub(crate) fn fire_dropped(app: &mut Proteus, dropped: TransitionDropped) {
    let signal = dropped.signal;
    let Some(mut cbs) = app.callbacks.dropped_handlers.remove(&signal) else {
        return;
    };
    for cb in &mut cbs {
        cb(app, dropped.clone());
    }
    app.callbacks
        .dropped_handlers
        .entry(signal)
        .or_default()
        .extend(cbs);
}
