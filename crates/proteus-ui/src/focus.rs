//! Which components can take focus, focus following a component into what it
//! transforms into, and focus leaving a component that can no longer keep it.
//!
//! A component can take focus when it's interactive, enabled and visible,
//! accepts the keyboard as the app does, and isn't transitioning, unless its
//! `TransitionInteractionConfig::allow_navigation` is set. [`focus_blocked`]
//! says why one can't.
//!
//! When a transition starts on the focused component, focus goes to what it
//! becomes: the `to` component of a channel, a split's target marked with
//! [`DefaultFocus`] or else its first, or a merge's destination.
//!
//! [`focus_validity_system`] clears focus from a component that can no longer
//! keep it, and records where it was in [`FocusState::lost_at`]. A component
//! hidden only while a split or merge into it runs keeps its focus, and so
//! does one that is transitioning.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;

use crate::component::{Disabled, Lifecycle, TransitionInteractionConfig};
use crate::devices::{AcceptsDevices, AppDevices, InputDevice};
use crate::hierarchy::{resolve_world_position_query, EffectiveVisibility};
use crate::input::{FocusState, Interactable, InteractionEvents};
use crate::topology::ActiveGroupTransition;
use crate::{QuadState, Visibility};

/// Marks the component focus starts at: the target of a split that takes
/// focus from the source.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct DefaultFocus;

/// Why a component can't take focus, or lost it.
///
/// More reasons may be added, so a `match` on it outside this crate needs a
/// `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FocusBlocked {
    /// The component was destroyed.
    Destroyed,
    /// It's hidden, or a parent of it is.
    Hidden,
    /// It's disabled.
    Disabled,
    /// It's non-interactive.
    NonInteractive,
    /// It, or the app, doesn't accept the keyboard.
    KeyboardNotAccepted,
    /// It's transitioning, and doesn't accept navigation while it does.
    Transitioning,
}

impl std::fmt::Display for FocusBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Destroyed => "it was destroyed",
            Self::Hidden => "it is hidden",
            Self::Disabled => "it is disabled",
            Self::NonInteractive => "it is non-interactive",
            Self::KeyboardNotAccepted => "it or the app doesn't accept the keyboard",
            Self::Transitioning => "it is transitioning and doesn't accept navigation meanwhile",
        })
    }
}

/// Why `entity` can't take focus now, or `None` if it can.
pub fn focus_blocked(world: &World, entity: Entity) -> Option<FocusBlocked> {
    let Ok(e) = world.get_entity(entity) else {
        return Some(FocusBlocked::Destroyed);
    };
    // Its own visibility changes at once; the cascaded one, which covers its
    // parents, a tick later.
    let visible = e.get::<Visibility>().is_none_or(|v| v.visible)
        && e.get::<EffectiveVisibility>().is_none_or(|v| v.0);
    let transitioning = matches!(e.get::<Lifecycle>(), Some(Lifecycle::Transitioning));
    let allow_navigation = e
        .get::<TransitionInteractionConfig>()
        .is_some_and(|c| c.allow_navigation);
    let keyboard = world
        .resource::<AppDevices>()
        .0
        .contains(InputDevice::Keyboard)
        && AcceptsDevices::allows(e.get::<AcceptsDevices>(), InputDevice::Keyboard);

    if !e.contains::<Interactable>() {
        Some(FocusBlocked::NonInteractive)
    } else if e.contains::<Disabled>() {
        Some(FocusBlocked::Disabled)
    } else if !keyboard {
        Some(FocusBlocked::KeyboardNotAccepted)
    } else if !visible {
        Some(FocusBlocked::Hidden)
    } else if transitioning && !allow_navigation {
        Some(FocusBlocked::Transitioning)
    } else {
        None
    }
}

/// Moves focus between components, recording the change in
/// [`InteractionEvents`] for this tick's callbacks.
#[derive(SystemParam)]
pub struct FocusMover<'w> {
    focus: ResMut<'w, FocusState>,
    events: ResMut<'w, InteractionEvents>,
}

impl FocusMover<'_> {
    /// Moves focus from `from` to `to`, if `from` has it.
    pub fn follow(&mut self, from: Entity, to: Entity) {
        if self.focus.focused == Some(from) && from != to {
            self.focus.focused = Some(to);
            self.events.blurred.push(from);
            self.events.focused.push(to);
        }
    }
}

/// The entities [`focus_validity_system`] reads to decide whether the focused
/// one can keep its focus.
type FocusCheckQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static QuadState,
        Has<Interactable>,
        Has<Disabled>,
        Option<&'static AcceptsDevices>,
        Option<&'static Visibility>,
        Option<&'static EffectiveVisibility>,
    ),
>;

/// Clears focus from a component that can no longer keep it: one destroyed,
/// hidden, disabled, made non-interactive, or no longer accepting the
/// keyboard. Fires its blur, unless it was destroyed, records why in
/// [`InteractionEvents::focus_lost`] and where it was in
/// [`FocusState::lost_at`]. Runs in
/// [`crate::schedule::ProteusSet::Navigation`].
///
/// A transitioning component keeps its focus, and so does one hidden only
/// while a split or merge into it runs: a split's targets and a merge's
/// destination are hidden until every piece arrives.
pub fn focus_validity_system(
    mut focus: ResMut<FocusState>,
    mut events: ResMut<InteractionEvents>,
    app_devices: Res<AppDevices>,
    components: FocusCheckQuery,
    groups: Query<(Entity, &ActiveGroupTransition)>,
    quad_states: Query<&QuadState>,
    parents: Query<&ChildOf>,
) {
    let Some(focused) = focus.focused else {
        return;
    };
    let Ok((qs, interactable, disabled, accepts, vis, eff_vis)) = components.get(focused) else {
        focus.focused = None;
        focus.lost_at = focus.geometry.take();
        events.focus_lost.push((focused, FocusBlocked::Destroyed));
        return;
    };

    // Its own visibility changes at once; the cascaded one, which covers its
    // parents, a tick later.
    let visible = vis.is_none_or(|v| v.visible) && eff_vis.is_none_or(|v| v.0);
    let in_group = || {
        groups
            .iter()
            .any(|(coord, g)| coord == focused || g.reveal_on_complete.contains(&focused))
    };
    let keyboard = app_devices.0.contains(InputDevice::Keyboard)
        && AcceptsDevices::allows(accepts, InputDevice::Keyboard);

    let blocked = if !interactable {
        Some(FocusBlocked::NonInteractive)
    } else if disabled {
        Some(FocusBlocked::Disabled)
    } else if !keyboard {
        Some(FocusBlocked::KeyboardNotAccepted)
    } else if !visible && !in_group() {
        Some(FocusBlocked::Hidden)
    } else {
        None
    };

    let world_qs = resolve_world_position_query(focused, qs, &quad_states, &parents);
    match blocked {
        None => focus.geometry = Some(world_qs),
        Some(reason) => {
            focus.focused = None;
            focus.geometry = None;
            focus.lost_at = Some(world_qs);
            events.blurred.push(focused);
            events.focus_lost.push((focused, reason));
        }
    }
}
