//! Interaction styles: how a component looks while *hovered*, *pressed*, *focused*
//! or *disabled*.
//!
//! ```text
//! HoveredEntity, PressedEntity, FocusState, Disabled   (this tick's state)
//!         │
//!         ▼
//! interaction_style_system   works out which style applies
//!         │  changed? an ActiveTransition to the new style
//!         ▼
//! transition_tick_system     as for any other transition
//! ```
//!
//! A style transition is not a transition of the component for anything else:
//! the entity stays in its current `Lifecycle`, so it keeps taking input and
//! a transition channel can still start a transition on it, and its completion
//! isn't recorded in [`crate::CompletedTransitions`]. Without this, a component
//! with a hover style would ignore the pointer for the length of the hover
//! animation.
//!
//! ## Which style wins
//!
//! When several apply at once: `Disabled`, then `Pressed`, then `Focused`, then
//! `Hover`, then `Default`. `Focused` applies only while the app is navigating,
//! and `Hover` only while it's pointing; see [`InputMode`].
//!
//! ## The declared geometry
//!
//! A style applies on top of the entity's declared geometry, not its current
//! `QuadState`, which may be partway through another style's transition.
//! [`InteractionState::declared`] holds it: the first tick the system sees an
//! entity, it records the entity's `QuadState` there and does nothing else.
//! `proteus-sdk` creates the `InteractionState` itself, with the component's
//! declared geometry, and its `set_declared_geometry` and `animate_to` update
//! it.
//!
//! ## Effects
//!
//! A style can also change a component's glow, border and drop shadow, one
//! property at a time, and they change alongside the geometry. Each time the
//! component leaves its default style, its effects are recorded as the ones it
//! rests with, [`RestingEffects`], which the default style returns to. Only the
//! properties a style changes are animated, so the app can set the others
//! meanwhile; one it sets that a style changes is undone when the style ends.
//!
//! ## Other transitions take priority
//!
//! While an entity is transitioning for another reason,
//! [`interaction_style_system`] leaves it alone, since a style transition would
//! redirect it. Once the entity is idle again, its style catches up.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};

use crate::component::{Disabled, Lifecycle};
use crate::devices::InputMode;
use crate::effects::{Border, DropShadow, Glow};
use crate::input::{FocusState, HoveredEntity, KeyPressedEntity, PressedEntity};
use crate::transition::{ActiveTransition, Easing, FrameTime, TransitionConfig};
use crate::QuadState;

// ---------------------------------------------------------------------------
// InteractionStateKind
// ---------------------------------------------------------------------------

/// Which interaction style applies to a component.
///
/// More states may be added, such as one for keyboard focus, so a `match` on
/// it outside this crate needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
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
/// change; the others come from the declared geometry, and from the effects
/// the component rests with.
///
/// The effect fields change an effect the component has: a glow's only on a
/// component with a [`Glow`], and so on. To have a style add a glow, give the
/// component one at rest, such as a glow of radius 0.
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
    /// The glow's radius, in pixels.
    pub glow_radius: Option<f32>,
    /// The glow's color.
    pub glow_color: Option<Vec4>,
    /// The glow's intensity.
    pub glow_intensity: Option<f32>,
    /// The border's width, in pixels.
    pub border_width: Option<f32>,
    /// The border's color.
    pub border_color: Option<Vec4>,
    /// The drop shadow's offset, in pixels.
    pub shadow_offset: Option<Vec2>,
    /// The drop shadow's color.
    pub shadow_color: Option<Vec4>,
    /// How soft the drop shadow's edge is, in pixels.
    pub shadow_softness: Option<f32>,
    /// How much larger than the component the drop shadow is, in pixels.
    pub shadow_spread: Option<f32>,
}

impl StyleOverride {
    /// Returns `base` with this style's effect fields applied to the effects
    /// it has. An effect `base` doesn't have stays absent.
    pub fn resolve_effects(&self, base: &EffectValues) -> EffectValues {
        EffectValues {
            glow: base.glow.map(|g| Glow {
                radius: self.glow_radius.unwrap_or(g.radius),
                color: self.glow_color.unwrap_or(g.color),
                intensity: self.glow_intensity.unwrap_or(g.intensity),
            }),
            border: base.border.map(|b| Border {
                width: self.border_width.unwrap_or(b.width),
                color: self.border_color.unwrap_or(b.color),
                ..b
            }),
            shadow: base.shadow.map(|d| DropShadow {
                offset: self.shadow_offset.unwrap_or(d.offset),
                color: self.shadow_color.unwrap_or(d.color),
                softness: self.shadow_softness.unwrap_or(d.softness),
                spread: self.shadow_spread.unwrap_or(d.spread),
            }),
        }
    }

    /// `current`, with each effect property `touched` sets taken from
    /// `wanted` instead. The rest keep `current`'s values, so that a property
    /// no style changes stays as the app set it.
    pub fn merge_effects(
        touched: &StyleOverride,
        current: &EffectValues,
        wanted: &EffectValues,
    ) -> EffectValues {
        fn pick<T: Copy>(touched: bool, current: T, wanted: T) -> T {
            if touched {
                wanted
            } else {
                current
            }
        }
        let t = touched;
        EffectValues {
            glow: match (current.glow, wanted.glow) {
                (Some(c), Some(w)) => Some(Glow {
                    radius: pick(t.glow_radius.is_some(), c.radius, w.radius),
                    color: pick(t.glow_color.is_some(), c.color, w.color),
                    intensity: pick(t.glow_intensity.is_some(), c.intensity, w.intensity),
                }),
                (c, _) => c,
            },
            border: match (current.border, wanted.border) {
                (Some(c), Some(w)) => Some(Border {
                    width: pick(t.border_width.is_some(), c.width, w.width),
                    color: pick(t.border_color.is_some(), c.color, w.color),
                    ..c
                }),
                (c, _) => c,
            },
            shadow: match (current.shadow, wanted.shadow) {
                (Some(c), Some(w)) => Some(DropShadow {
                    offset: pick(t.shadow_offset.is_some(), c.offset, w.offset),
                    color: pick(t.shadow_color.is_some(), c.color, w.color),
                    softness: pick(t.shadow_softness.is_some(), c.softness, w.softness),
                    spread: pick(t.shadow_spread.is_some(), c.spread, w.spread),
                }),
                (c, _) => c,
            },
        }
    }

    /// The effects this style changes that `effects` doesn't have, by name:
    /// `"glow"`, `"border"` or `"drop shadow"`. A style's changes to them do
    /// nothing.
    pub fn missing_effects(&self, effects: &EffectValues) -> Vec<&'static str> {
        let mut missing = Vec::new();
        let glow = self.glow_radius.is_some()
            || self.glow_color.is_some()
            || self.glow_intensity.is_some();
        if glow && effects.glow.is_none() {
            missing.push("glow");
        }
        if (self.border_width.is_some() || self.border_color.is_some()) && effects.border.is_none()
        {
            missing.push("border");
        }
        let shadow = self.shadow_offset.is_some()
            || self.shadow_color.is_some()
            || self.shadow_softness.is_some()
            || self.shadow_spread.is_some();
        if shadow && effects.shadow.is_none() {
            missing.push("drop shadow");
        }
        missing
    }

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
// Effects
// ---------------------------------------------------------------------------

/// A component's glow, border and drop shadow, each if it has one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EffectValues {
    /// Its glow.
    pub glow: Option<Glow>,
    /// Its border.
    pub border: Option<Border>,
    /// Its drop shadow.
    pub shadow: Option<DropShadow>,
}

/// The effects a component rests with, which its default style returns to.
/// Recorded by [`interaction_style_system`] each time the component leaves its
/// default style, so changes the app made at rest are kept.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct RestingEffects(pub EffectValues);

/// How a component's changes of interaction style are timed. Without it,
/// 0.15 s, easing out.
#[derive(Component, Debug, Clone, Copy)]
pub struct StyleTiming(pub TransitionConfig);

/// A change of effects under way, from one interaction style to another.
/// [`effect_transition_system`] advances it, writing only the properties that
/// differ between `from` and `to`, so the app can change the others meanwhile.
#[derive(Component, Debug, Clone, Copy)]
pub struct EffectTransition {
    /// Where the effects start.
    pub from: EffectValues,
    /// Where they end.
    pub to: EffectValues,
    /// How the change is timed.
    pub config: TransitionConfig,
    /// Seconds since it started.
    pub elapsed: f32,
}

/// The entities [`effect_transition_system`] advances, with the effects it
/// writes.
type EffectTransitionQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut EffectTransition,
        Option<&'static mut Glow>,
        Option<&'static mut Border>,
        Option<&'static mut DropShadow>,
    ),
>;

/// Advances each [`EffectTransition`] by this tick's time, and removes it once
/// it has ended. Runs in [`crate::schedule::ProteusSet::TransitionTick`].
pub fn effect_transition_system(
    mut commands: Commands,
    time: Res<FrameTime>,
    mut query: EffectTransitionQuery,
) {
    for (entity, mut change, glow, border, shadow) in &mut query {
        change.elapsed += time.delta_secs;
        let config = change.config;
        let run = change.elapsed - config.delay;
        if run < 0.0 {
            continue;
        }
        let linear = if config.duration > 0.0 {
            (run / config.duration).min(1.0)
        } else {
            1.0
        };
        let t = config.easing.apply(linear);
        let EffectTransition { from, to, .. } = *change;

        if let (Some(mut g), Some(a), Some(b)) = (glow, from.glow, to.glow) {
            set_lerp(&mut g.radius, a.radius, b.radius, t);
            set_lerp(&mut g.color, a.color, b.color, t);
            set_lerp(&mut g.intensity, a.intensity, b.intensity, t);
        }
        if let (Some(mut d), Some(a), Some(b)) = (border, from.border, to.border) {
            set_lerp(&mut d.width, a.width, b.width, t);
            set_lerp(&mut d.color, a.color, b.color, t);
        }
        if let (Some(mut d), Some(a), Some(b)) = (shadow, from.shadow, to.shadow) {
            set_lerp(&mut d.offset, a.offset, b.offset, t);
            set_lerp(&mut d.color, a.color, b.color, t);
            set_lerp(&mut d.softness, a.softness, b.softness, t);
            set_lerp(&mut d.spread, a.spread, b.spread, t);
        }
        if linear >= 1.0 {
            commands.entity(entity).remove::<EffectTransition>();
        }
    }
}

/// Sets `field` `t` of the way from `a` to `b`, if they differ: a property a
/// style doesn't change is left to the app.
fn set_lerp<T: Lerp>(field: &mut T, a: T, b: T, t: f32) {
    if a != b {
        *field = T::lerp(a, b, t);
    }
}

trait Lerp: Copy + PartialEq {
    fn lerp(a: Self, b: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
}

impl Lerp for Vec2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a.lerp(b, t)
    }
}

impl Lerp for Vec4 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a.lerp(b, t)
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

impl InteractionDef {
    /// A style setting each effect property any of these styles sets, to
    /// tell which ones they change; its values mean nothing.
    fn effects_touched(&self) -> StyleOverride {
        let styles = [&self.hover, &self.pressed, &self.focused, &self.disabled];
        let any = |f: fn(&StyleOverride) -> bool| {
            styles
                .iter()
                .any(|s| s.as_ref().is_some_and(f))
                .then_some(())
        };
        StyleOverride {
            glow_radius: any(|s| s.glow_radius.is_some()).map(|_| 0.0),
            glow_color: any(|s| s.glow_color.is_some()).map(|_| Vec4::ZERO),
            glow_intensity: any(|s| s.glow_intensity.is_some()).map(|_| 0.0),
            border_width: any(|s| s.border_width.is_some()).map(|_| 0.0),
            border_color: any(|s| s.border_color.is_some()).map(|_| Vec4::ZERO),
            shadow_offset: any(|s| s.shadow_offset.is_some()).map(|_| Vec2::ZERO),
            shadow_color: any(|s| s.shadow_color.is_some()).map(|_| Vec4::ZERO),
            shadow_softness: any(|s| s.shadow_softness.is_some()).map(|_| 0.0),
            shadow_spread: any(|s| s.shadow_spread.is_some()).map(|_| 0.0),
            ..StyleOverride::default()
        }
    }
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

/// The transition to a new interaction style, without a [`StyleTiming`]:
/// styles change smoothly, not instantly.
pub const STYLE_TRANSITION_CONFIG: TransitionConfig = TransitionConfig {
    duration: 0.15,
    delay: 0.0,
    easing: Easing::EaseOutQuad,
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
        (
            Option<&'static Glow>,
            Option<&'static Border>,
            Option<&'static DropShadow>,
            Option<&'static RestingEffects>,
            Option<&'static StyleTiming>,
        ),
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
    key_pressed: Res<KeyPressedEntity>,
    focus: Res<FocusState>,
    mode: Res<InputMode>,
    query: InteractionStyleQuery,
) {
    let navigating = *mode == InputMode::Navigating;
    for (entity, quad_state, def, existing, lifecycle, disabled, effects) in query.iter() {
        let (glow, border, shadow, resting, timing) = effects;
        if matches!(lifecycle, Some(Lifecycle::Transitioning)) {
            continue;
        }

        let declared = existing
            .map(|s| s.declared.clone())
            .unwrap_or_else(|| quad_state.clone());

        let resolved = if disabled {
            InteractionStateKind::Disabled
        } else if pressed.entity == Some(entity)
            || key_pressed
                .pressed
                .as_ref()
                .is_some_and(|(e, _)| *e == entity)
        {
            InteractionStateKind::Pressed
        } else if navigating && focus.focused == Some(entity) {
            InteractionStateKind::Focused
        } else if !navigating && hovered.0 == Some(entity) {
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

        let config = timing.map_or(STYLE_TRANSITION_CONFIG, |t| t.0);
        // Start from the current QuadState, which stays smooth even partway
        // through another style's transition.
        commands
            .entity(entity)
            .insert(ActiveTransition::for_interaction_style(
                quad_state.clone(),
                target,
                config,
            ));

        let style = |kind| match kind {
            InteractionStateKind::Hover => def.hover.as_ref(),
            InteractionStateKind::Pressed => def.pressed.as_ref(),
            InteractionStateKind::Focused => def.focused.as_ref(),
            InteractionStateKind::Disabled => def.disabled.as_ref(),
            _ => None,
        };
        let current = EffectValues {
            glow: glow.copied(),
            border: border.copied(),
            shadow: shadow.copied(),
        };
        // At rest, the effects are the ones to come back to, with any change
        // the app made since.
        let resting = if existing.current == InteractionStateKind::Default {
            commands.entity(entity).insert(RestingEffects(current));
            current
        } else {
            resting.map_or(current, |r| r.0)
        };
        let wanted = style(resolved).map_or(resting, |s| s.resolve_effects(&resting));
        let target_effects =
            StyleOverride::merge_effects(&def.effects_touched(), &current, &wanted);
        if target_effects != current {
            commands.entity(entity).insert(EffectTransition {
                from: current,
                to: target_effects,
                config,
                elapsed: 0.0,
            });
        }
        commands.entity(entity).insert(InteractionState {
            current: resolved,
            declared,
        });
    }
}
