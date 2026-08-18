//! Per-state interaction styling (M12.2) — the rest of what M7's DoD deferred
//! to M12, beyond the raw events `input.rs` produces.
//!
//! ## Data flow
//!
//! ```text
//! HoveredEntity / PressedEntity / FocusState / Disabled  (this frame's state)
//!         │
//!         ▼
//! interaction_style_system  (resolves precedence, compares to InteractionState.current)
//!         │  state changed? insert TransitionRequest + updated InteractionState
//!         ▼
//! transition_setup_system (transition.rs)  — same machinery a signal-driven morph uses
//! ```
//!
//! ## Precedence
//!
//! When more than one of disabled/pressed/focused/hovered is true at once:
//! `Disabled` > `Pressed` > `Focused` > `Hover` > `Default`. Not specified in
//! PLANNING.md's Phase A — this is a documented default (disabled always wins;
//! pressed is more specific than hover; a focus ring showing through hover is
//! conventional).
//!
//! ## The "declared" state
//!
//! A style override resolves against the entity's true rest geometry, not its
//! live `QuadState` (which may itself be mid-interaction-style). There is no
//! general "declared state" storage yet — that's M12.3's `proteus-sdk` job —
//! so [`InteractionState`] captures it locally: the first frame an
//! `InteractionDef`-carrying entity is seen, its current `QuadState` is
//! snapshotted into `InteractionState.declared` and nothing else happens that
//! frame. Every later resolution targets `override.resolve(&declared)`.
//!
//! ## Never fights a live signal-driven transition
//!
//! If an entity is `Lifecycle::Transitioning` (a big morph in flight),
//! [`interaction_style_system`] skips it entirely that frame. Inserting a
//! competing `TransitionRequest` would hijack `transition_setup_system`'s
//! existing retarget-from-current-state behavior and visibly redirect an
//! in-flight signal-driven morph toward a hover/press style instead. Once the
//! entity returns to `Idle`, the next frame's resolution catches up on any
//! state drift that happened while it was transitioning.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use crate::component::{Disabled, Lifecycle, TransitionRequest};
use crate::input::{FocusState, HoveredEntity, PressedEntity};
use crate::transition::{ease_out_quad, TransitionConfig};
use crate::QuadState;

// ---------------------------------------------------------------------------
// InteractionStateKind
// ---------------------------------------------------------------------------

/// Which sparse style, if any, currently applies to an interactive component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionStateKind {
    #[default]
    Default,
    Hover,
    Pressed,
    Focused,
    Disabled,
}

// ---------------------------------------------------------------------------
// StyleOverride
// ---------------------------------------------------------------------------

/// A sparse `QuadState` override — only declare the fields that change for a
/// given interaction state. Undeclared fields inherit from the entity's
/// declared default (Phase A: "only the properties that change for a given
/// state need to be declared").
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleOverride {
    pub position: Option<Vec3>,
    pub size: Option<Vec2>,
    pub rotation: Option<f32>,
    pub scale: Option<f32>,
    pub anchor: Option<Vec2>,
    pub color: Option<Vec4>,
    pub corner_radius: Option<f32>,
}

impl StyleOverride {
    /// Apply this override on top of `base`, returning a fully resolved
    /// `QuadState`. Fields left `None` pass `base`'s value through unchanged.
    pub fn resolve(&self, base: &QuadState) -> QuadState {
        QuadState {
            position: self.position.unwrap_or(base.position),
            size: self.size.unwrap_or(base.size),
            rotation: self.rotation.unwrap_or(base.rotation),
            scale: self.scale.unwrap_or(base.scale),
            anchor: self.anchor.unwrap_or(base.anchor),
            color: self.color.unwrap_or(base.color),
            corner_radius: self.corner_radius.unwrap_or(base.corner_radius),
        }
    }
}

// ---------------------------------------------------------------------------
// InteractionDef component
// ---------------------------------------------------------------------------

/// Declares the sparse per-state style overrides for an interactive
/// component. Any state left `None` simply resolves to the declared default
/// unchanged.
#[derive(Component, Debug, Clone, Default)]
pub struct InteractionDef {
    pub hover: Option<StyleOverride>,
    pub pressed: Option<StyleOverride>,
    pub focused: Option<StyleOverride>,
    pub disabled: Option<StyleOverride>,
}

// ---------------------------------------------------------------------------
// InteractionState component
// ---------------------------------------------------------------------------

/// Tracks an entity's currently-resolved interaction state and its captured
/// declared (rest) geometry. Inserted automatically by
/// [`interaction_style_system`] the first frame it sees an `InteractionDef`
/// entity — never constructed by callers directly.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct InteractionState {
    pub current: InteractionStateKind,
    pub declared: QuadState,
}

// ---------------------------------------------------------------------------
// interaction_style_system
// ---------------------------------------------------------------------------

/// Every state change is a mini-transition, driven through the same
/// `TransitionRequest`/`ActiveTransition` machinery a full signal-driven morph
/// uses — not an instant snap (Phase A: "every state change is a potential
/// mini-transition, not just a CSS swap").
const STYLE_TRANSITION_CONFIG: TransitionConfig = TransitionConfig {
    duration: 0.15,
    delay: 0.0,
    easing: ease_out_quad,
};

/// Query for [`interaction_style_system`]: every `InteractionDef` entity,
/// with whatever `InteractionState`/`Lifecycle`/`Disabled` it currently has.
type InteractionStyleQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static QuadState,
        &'static InteractionDef,
        Option<&'static InteractionState>,
        Option<&'static Lifecycle>,
        Has<Disabled>,
    ),
>;

/// Resolves each `InteractionDef` entity's current [`InteractionStateKind`]
/// from this frame's [`HoveredEntity`]/[`PressedEntity`]/[`FocusState`]/
/// [`Disabled`], and — on change — triggers a mini-transition to the
/// resolved style. Runs in [`crate::schedule::ProteusSet::InteractionStyle`],
/// right after [`crate::input::hit_test_system`].
///
/// Skips any entity that is `Lifecycle::Transitioning` (see this module's top
/// doc) and, for a never-before-seen entity, only captures
/// [`InteractionState::declared`] without transitioning anywhere — there is
/// nothing to visually move *from* on that first frame.
pub fn interaction_style_system(
    mut commands: Commands,
    hovered: Res<HoveredEntity>,
    pressed: Res<PressedEntity>,
    focus: Res<FocusState>,
    query: InteractionStyleQuery,
) {
    for (entity, quad_state, def, existing, lifecycle, disabled) in query.iter() {
        if matches!(lifecycle, Some(Lifecycle::Transitioning)) {
            continue;
        }

        let declared = existing
            .map(|s| s.declared.clone())
            .unwrap_or_else(|| quad_state.clone());

        let resolved = if disabled {
            InteractionStateKind::Disabled
        } else if pressed.entity == Some(entity) {
            InteractionStateKind::Pressed
        } else if focus.focused == Some(entity) {
            InteractionStateKind::Focused
        } else if hovered.0 == Some(entity) {
            InteractionStateKind::Hover
        } else {
            InteractionStateKind::Default
        };

        let Some(existing) = existing else {
            // First time seeing this entity: establish the baseline as
            // Default, regardless of `resolved` — even if the entity happens
            // to already be hovered/pressed/focused/disabled this very frame.
            // Recording `resolved` directly here would mark it e.g. Hover
            // without ever having inserted the TransitionRequest that gets it
            // there, silently skipping the mini-transition on every future
            // frame too (the resolved-vs-current comparison below would find
            // no change). Next frame's comparison against this Default
            // baseline correctly detects the drift and transitions properly.
            commands.entity(entity).insert(InteractionState {
                current: InteractionStateKind::Default,
                declared,
            });
            continue;
        };

        if existing.current == resolved {
            continue;
        }

        let target = match resolved {
            InteractionStateKind::Default => declared.clone(),
            InteractionStateKind::Hover => def
                .hover
                .as_ref()
                .map(|o| o.resolve(&declared))
                .unwrap_or_else(|| declared.clone()),
            InteractionStateKind::Pressed => def
                .pressed
                .as_ref()
                .map(|o| o.resolve(&declared))
                .unwrap_or_else(|| declared.clone()),
            InteractionStateKind::Focused => def
                .focused
                .as_ref()
                .map(|o| o.resolve(&declared))
                .unwrap_or_else(|| declared.clone()),
            InteractionStateKind::Disabled => def
                .disabled
                .as_ref()
                .map(|o| o.resolve(&declared))
                .unwrap_or_else(|| declared.clone()),
        };

        commands.entity(entity).insert(TransitionRequest {
            to: target,
            config: STYLE_TRANSITION_CONFIG,
            // None — snapshot the entity's current QuadState as the origin,
            // smooth even if it was already mid-way through a previous
            // interaction-style mini-transition.
            from_state: None,
        });
        commands.entity(entity).insert(InteractionState {
            current: resolved,
            declared,
        });
    }
}
