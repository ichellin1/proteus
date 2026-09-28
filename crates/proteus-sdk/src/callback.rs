//! Callback storage and dispatch.
//!
//! The `on_*` methods on [`Handle`](crate::Handle) and
//! [`SignalHandle`](crate::SignalHandle) register closures here, and
//! [`Proteus::tick`](crate::Proteus::tick) calls them after each update with
//! the events that update produced.
//!
//! Calling a handler needs `&mut Proteus`, which also owns the handler map,
//! so the map can't be borrowed while a handler runs. Dispatch therefore
//! removes a key's handlers from the map, calls them, and then puts them
//! back. Handlers registered during the call are kept alongside them.

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
    TransitionComplete,
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

    /// Drops every handler registered for `entity`.
    ///
    /// Called when the entity is destroyed. A destroyed entity's ID is never
    /// reused, so nothing else would ever remove its handlers.
    pub(crate) fn forget_entity(&mut self, entity: Entity) {
        self.handlers.retain(|(e, _), _| *e != entity);
        self.drag_handlers.remove(&entity);
    }

    /// Drops every `on_dropped` handler registered for `signal`.
    pub(crate) fn forget_signal(&mut self, signal: SignalId) {
        self.dropped_handlers.remove(&signal);
    }

    /// The number of registered handlers of every kind. For tests: a leaked
    /// handler is otherwise invisible.
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

/// Calls every handler registered for `(entity, kind)`, then returns them to
/// the map, keeping any handlers registered during the call. If a handler
/// destroyed `entity`, its handlers are dropped instead.
pub(crate) fn fire(app: &mut Proteus, entity: Entity, kind: EventKind) {
    let Some(mut cbs) = app.callbacks.handlers.remove(&(entity, kind)) else {
        return;
    };
    for cb in &mut cbs {
        cb(app);
    }
    // Put the handlers back only if the entity still exists. A handler can
    // destroy its own component, such as a dismiss button. `Handle::destroy`
    // removes the component's handlers from the map, but not these, which
    // are held here during the call. Putting them back would leave them
    // registered for a component that no longer exists.
    if is_alive(app, entity) {
        app.callbacks
            .handlers
            .entry((entity, kind))
            .or_default()
            .extend(cbs);
    }
}

/// Check if the `entity` still exists.
fn is_alive(app: &Proteus, entity: Entity) -> bool {
    app.world.world.entities().contains(entity)
}

/// [`fire`] for drag handlers, which also receive `delta`: how far the
/// pointer moved since the previous tick, in world units.
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

/// [`fire`] for [`SignalHandle::on_dropped`](crate::SignalHandle::on_dropped)
/// handlers. These are registered per signal, and each receives the
/// [`TransitionDropped`] describing the request that couldn't run and why.
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
