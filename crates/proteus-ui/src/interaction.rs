//! Interaction styles: how a component looks while *hovered*, *pressed*, *focused*
//! or *disabled*.
//!
//! ```text
//! HoveredEntity, PressedEntity, FocusState, Disabled   (this tick's state)
//!         │
//!         ▼
//! interaction_style_system   works out which style applies
//!         │  changed? a TransitionRequest to the new style
//!         ▼
//! transition_setup_system    as for any other transition
//! ```
//!
//! ## Which style wins
//!
//! When several apply at once: `Disabled`, then `Pressed`, then `Focused`, then
//! `Hover`, then `Default`.
//!
//! ## The declared geometry
//!
//! A style applies on top of the entity's declared geometry, not its current
//! `QuadState`, which may be partway through another style's transition.
//! [`InteractionState::declared`] holds it: the first tick the system sees an
//! entity, it records the entity's `QuadState` there and does nothing else.
//! `proteus-sdk`'s `set_declared_geometry` updates it.
//!
//! ## Other transitions take priority
//!
//! While an entity is transitioning for another reason,
//! [`interaction_style_system`] leaves it alone, since a style transition would
//! redirect it. Once the entity is idle again, its style catches up.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use crate::component::{Disabled, Lifecycle, TransitionRequest};
use crate::input::{FocusState, HoveredEntity, PressedEntity};
use crate::transition::{ease_out_quad, TransitionConfig};
use crate::QuadState;

// ---------------------------------------------------------------------------
// InteractionStateKind
// ---------------------------------------------------------------------------

/// Which interaction style applies to a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InteractionStateKind {
    /// No style: the declared geometry.
    #[default]
    Default,
    /// The pointer is over it.
    Hover,
    /// It is pressed.
    Pressed,
    /// It has focus.
    Focused,
    /// It is disabled.
    Disabled,
}

// ---------------------------------------------------------------------------
// StyleOverride
// ---------------------------------------------------------------------------

/// How a component looks in one interaction state. Set only the fields that
/// change; the others come from the declared geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleOverride {
    /// Position, in world units.
    pub position: Option<Vec3>,
    /// Width and height.
    pub size: Option<Vec2>,
    /// Rotation, in radians.
    pub rotation: Option<f32>,
    /// Uniform scale.
    pub scale: Option<f32>,
    /// The point `position` refers to, as fractions of the size.
    pub anchor: Option<Vec2>,
    /// Fill color.
    pub color: Option<Vec4>,
    /// Corner radius, in pixels.
    pub corner_radius: Option<f32>,
}

impl StyleOverride {
    /// Returns `base` with this style's fields applied. Fields that are `None`
    /// keep `base`'s value.
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

/// A component's interaction styles. A state without one shows the declared
/// geometry.
#[derive(Component, Debug, Clone, Default)]
pub struct InteractionDef {
    /// The style while the pointer is over it.
    pub hover: Option<StyleOverride>,
    /// The style while it is pressed.
    pub pressed: Option<StyleOverride>,
    /// The style while it has focus.
    pub focused: Option<StyleOverride>,
    /// The style while it is disabled.
    pub disabled: Option<StyleOverride>,
}

// ---------------------------------------------------------------------------
// InteractionState component
// ---------------------------------------------------------------------------

/// An entity's current interaction style and declared geometry. Added by
/// [`interaction_style_system`] the first tick it sees an entity with an
/// [`InteractionDef`].
#[derive(Component, Debug, Clone, PartialEq)]
pub struct InteractionState {
    /// The style that applies now.
    pub current: InteractionStateKind,
    /// The geometry styles apply on top of.
    pub declared: QuadState,
}

// ---------------------------------------------------------------------------
// interaction_style_system
// ---------------------------------------------------------------------------

/// The transition to a new interaction style: styles change smoothly, not
/// instantly.
const STYLE_TRANSITION_CONFIG: TransitionConfig = TransitionConfig {
    duration: 0.15,
    delay: 0.0,
    easing: ease_out_quad,
};

/// The entities [`interaction_style_system`] considers: every entity with an
/// `InteractionDef`.
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

/// Works out which interaction style applies to each entity from this tick's
/// [`HoveredEntity`], [`PressedEntity`], [`FocusState`] and [`Disabled`], and
/// when it changes, starts a transition to it. Runs just after
/// [`crate::input::hit_test_system`].
///
/// Skips a transitioning entity, and for an entity seen for the first time
/// only records its declared geometry; see the module docs.
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
            // First sighting: record Default, even if the entity is already
            // hovered or pressed. Recording that style instead would mark it
            // as applied without transitioning to it, and later ticks would
            // see no change. Next tick finds the difference and transitions.
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
            // `None`: start from the current QuadState, which stays smooth even
            // partway through another style's transition.
            from_state: None,
        });
        commands.entity(entity).insert(InteractionState {
            current: resolved,
            declared,
        });
    }
}
