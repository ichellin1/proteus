//! The types [`Proteus::get`](crate::Proteus::get) returns when you read a
//! component's state.
//!
//! [`ComponentData`] is a copy of the component's state at the moment `get`
//! was called. If the component is transitioning, its `transition` field holds
//! a [`TransitionData`] describing the transition. Neither updates afterwards;
//! call `get` again to see later changes.

use proteus_ui::{ActiveTransition, InteractionStateKind, QuadState};

use crate::handle::Handle;

/// A snapshot of a component's state, taken when
/// [`Proteus::get`](crate::Proteus::get) was called. It does not update
/// afterwards.
#[derive(Debug, Clone)]
pub struct ComponentData {
    /// The component's current geometry: its declared geometry when idle, or
    /// its in-progress geometry while transitioning (the same as
    /// `transition.current`).
    pub geometry: QuadState,
    /// The interaction style currently applied.
    ///
    /// Stays `Default` for a component that declared no interaction styles,
    /// even while it is disabled, and updates one tick after the change. To
    /// check whether a component is disabled, use [`ComponentData::disabled`].
    pub state: InteractionStateKind,
    /// Whether the component is disabled.
    ///
    /// Unlike [`ComponentData::state`], this changes as soon as
    /// [`Handle::set_disabled`] is called, and is correct for every component,
    /// including one with no interaction styles.
    pub disabled: bool,
    /// Whether the component is visible. A component inside a hidden parent
    /// is not. Updated each tick.
    pub visible: bool,
    /// The opacity the component is drawn with: its own opacity multiplied
    /// by its parents'. Updated each tick.
    pub opacity: f32,
    /// The component's direct children, in order.
    pub children: Vec<Handle>,
    /// The transition in progress, or `None` when idle. A change of
    /// interaction style, such as a hover effect, isn't reported here.
    pub transition: Option<TransitionData>,
}

/// A snapshot of a transition in progress.
#[derive(Debug, Clone)]
pub struct TransitionData {
    /// The geometry the transition started from.
    pub base: QuadState,
    /// The geometry the transition ends at.
    pub target: QuadState,
    /// The current geometry, the same as [`ComponentData::geometry`].
    pub current: QuadState,
    /// Progress through the transition, from `0.0` to `1.0`, before easing is
    /// applied. Useful for keeping other animation in step with the
    /// transition.
    pub progress: f32,
}

impl TransitionData {
    pub(crate) fn from_active(active: &ActiveTransition, current: QuadState) -> Self {
        let progress = active.raw_t();
        Self {
            base: active.from.clone(),
            target: active.to.clone(),
            current,
            progress,
        }
    }
}
