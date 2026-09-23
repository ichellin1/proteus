//! `ComponentData` — the read-only shape [`crate::Proteus::get`] returns.
//!
//! Mirrors PLANNING.md Phase A's `proteus.get(id)` sketch: `geometry` is
//! always "what is this component right now" (declared state when idle, the
//! live interpolated state when transitioning); `transition` is `None` when
//! idle, populated with `base`/`target`/`current`/`progress` otherwise.

use proteus_ui::{ActiveTransition, InteractionStateKind, QuadState};

use crate::handle::Handle;

/// One completed read of an entity's current state, as of the moment
/// [`crate::Proteus::get`] was called — not a live view.
#[derive(Debug, Clone)]
pub struct ComponentData {
    /// Current resolved geometry — the declared state when idle, the live
    /// interpolated state when transitioning (same value as
    /// `transition.current` in that case).
    pub geometry: QuadState,
    /// Current resolved interaction state. `Default` for an entity with no
    /// `InteractionDef` (M12.2), or one `interaction_style_system` hasn't
    /// processed yet.
    pub state: InteractionStateKind,
    /// Cascaded effective visibility when available (M10), falling back to
    /// the entity's own raw `Visibility` — same preference order
    /// `hit_test_system` already uses. Defaults to `true` when neither
    /// component is present.
    pub visible: bool,
    /// Cascaded effective opacity when available (M10), falling back to the
    /// entity's own raw `Opacity`. Defaults to `1.0` when neither component
    /// is present. Independent of [`ComponentData::visible`] — see
    /// `Handle::set_opacity`.
    pub opacity: f32,
    /// Direct children, in `bevy_ecs::hierarchy::Children` order.
    pub children: Vec<Handle>,
    /// `None` when idle; populated for the duration of an active transition.
    pub transition: Option<TransitionData>,
}

/// Snapshot of an in-flight transition, derived from `ActiveTransition`.
#[derive(Debug, Clone)]
pub struct TransitionData {
    /// Geometry at the start of the transition.
    pub base: QuadState,
    /// Geometry the transition is heading toward.
    pub target: QuadState,
    /// Current interpolated geometry — identical to the enclosing
    /// [`ComponentData::geometry`].
    pub current: QuadState,
    /// Raw (pre-easing) progress in `[0, 1]`, i.e. `elapsed / duration`
    /// clamped — the same `t` PLANNING.md's Phase A describes as "useful for
    /// dependent animations, progress indicators, or cancellation logic."
    pub progress: f32,
}

impl TransitionData {
    pub(crate) fn from_active(active: &ActiveTransition, current: QuadState) -> Self {
        let progress = (active.elapsed / active.config.duration).clamp(0.0, 1.0);
        Self {
            base: active.from.clone(),
            target: active.to.clone(),
            current,
            progress,
        }
    }
}
