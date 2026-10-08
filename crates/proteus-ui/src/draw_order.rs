//! The order components are drawn in, which the pointer follows too, so that a
//! click goes to the component drawn on top.
//!
//! Top-level entities are drawn by `z`, lowest first, then by [`SpawnOrder`],
//! earliest first. Each entity is followed by its children, ordered the same
//! way by their own `z` and `SpawnOrder`, so a child is always drawn over its
//! parent, and never over a different top-level entity drawn after it.
//!
//! [`DrawKey`] captures this for hit testing: the `(z, SpawnOrder)` of an
//! entity's ancestors from the top level down, then its own. Keys compare in
//! drawing order, since a parent's key is the start of its children's.

use std::cmp::Ordering;

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;

use crate::component::QuadState;
use crate::spawn_order::SpawnOrder;

/// Orders two siblings, or two top-level entities, by `z` and then
/// [`SpawnOrder`]: the one drawn first is `Less`. For a child, `z` is its own
/// `z`, relative to its parent.
pub(crate) fn rank_cmp(a: (f32, SpawnOrder), b: (f32, SpawnOrder)) -> Ordering {
    a.0.partial_cmp(&b.0)
        .unwrap_or(Ordering::Equal)
        .then(a.1.cmp(&b.1))
}

/// An entity's place in drawing order; see the module docs. A greater key is
/// drawn later, on top.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawKey(Vec<(f32, SpawnOrder)>);

impl DrawKey {
    /// The key of `entity`, read through its `ChildOf` ancestors. An entity
    /// without a `SpawnOrder`, such as one in a test world without hooks,
    /// sorts last among equals, as in drawing.
    pub(crate) fn of(
        entity: Entity,
        quad_states: &Query<&QuadState>,
        spawn_orders: &Query<&SpawnOrder>,
        parents: &Query<&ChildOf>,
    ) -> Self {
        let mut ranks = Vec::new();
        let mut current = Some(entity);
        while let Some(e) = current {
            let z = quad_states.get(e).map(|qs| qs.position.z).unwrap_or(0.0);
            let spawn = spawn_orders.get(e).copied().unwrap_or(SpawnOrder(u64::MAX));
            ranks.push((z, spawn));
            current = parents.get(e).ok().map(|c| c.parent());
        }
        ranks.reverse();
        DrawKey(ranks)
    }

    /// Compares two keys in drawing order.
    pub(crate) fn draw_cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.0.iter().zip(&other.0) {
            match rank_cmp(*a, *b) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        // One is an ancestor of the other: the ancestor is drawn first.
        self.0.len().cmp(&other.0.len())
    }
}
