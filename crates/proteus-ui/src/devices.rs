//! Which input devices the app, and each component, accept.
//!
//! The app accepts a set of [`InputDevices`], every device by default, held in
//! [`AppDevices`]. Input from a device it doesn't accept is dropped before
//! anything sees it. A component can narrow the set with [`AcceptsDevices`]:
//! it accepts the devices both it and the app accept.
//!
//! When the app accepts both styles of input, the one used last wins: see
//! [`InputMode`].

use std::fmt;
use std::ops::{BitAnd, BitOr};

use bevy_ecs::prelude::*;

use crate::keymap::Action;

/// A kind of input device.
///
/// More devices will be added, such as touch, TV remotes and game
/// controllers, so a `match` on it outside this crate needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputDevice {
    /// A mouse natively, and a mouse, touch or pen in a browser.
    Pointer,
    /// A keyboard.
    Keyboard,
}

impl InputDevice {
    /// Every device, in a fixed order.
    pub const ALL: [InputDevice; 2] = [InputDevice::Pointer, InputDevice::Keyboard];

    fn bit(self) -> u8 {
        match self {
            InputDevice::Pointer => 1,
            InputDevice::Keyboard => 2,
        }
    }
}

/// A set of [`InputDevice`]s.
///
/// Combine sets with `|`, as in `InputDevices::POINTER | InputDevices::KEYBOARD`,
/// and intersect them with `&`. [`InputDevices::ALL`] includes devices added
/// in later versions, so an app that accepts every device keeps doing so.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputDevices(u8);

impl InputDevices {
    /// No device.
    pub const NONE: InputDevices = InputDevices(0);
    /// Only the pointer.
    pub const POINTER: InputDevices = InputDevices(1);
    /// Only the keyboard.
    pub const KEYBOARD: InputDevices = InputDevices(2);
    /// Every device.
    pub const ALL: InputDevices = InputDevices(1 | 2);

    /// Whether `device` is in the set.
    pub fn contains(self, device: InputDevice) -> bool {
        self.0 & device.bit() != 0
    }

    /// The devices in the set, in the order of [`InputDevice::ALL`].
    pub fn iter(self) -> impl Iterator<Item = InputDevice> {
        InputDevice::ALL
            .into_iter()
            .filter(move |d| self.contains(*d))
    }
}

impl Default for InputDevices {
    /// Every device.
    fn default() -> Self {
        Self::ALL
    }
}

impl From<InputDevice> for InputDevices {
    fn from(device: InputDevice) -> Self {
        InputDevices(device.bit())
    }
}

impl FromIterator<InputDevice> for InputDevices {
    fn from_iter<I: IntoIterator<Item = InputDevice>>(devices: I) -> Self {
        devices
            .into_iter()
            .fold(Self::NONE, |set, device| set | device.into())
    }
}

impl BitOr for InputDevices {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        InputDevices(self.0 | other.0)
    }
}

impl BitAnd for InputDevices {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        InputDevices(self.0 & other.0)
    }
}

impl fmt::Debug for InputDevices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// The devices the app accepts. Every device by default.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppDevices(pub InputDevices);

/// The devices a component accepts, narrowing the app's. A component without
/// it accepts every device the app does.
///
/// A component that doesn't accept the pointer lets it through to whatever is
/// behind it, as a non-interactive one does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptsDevices(pub InputDevices);

impl AcceptsDevices {
    /// Whether a component with `accepts`, or without it if `None`, accepts
    /// `device`.
    pub fn allows(accepts: Option<&AcceptsDevices>, device: InputDevice) -> bool {
        accepts.is_none_or(|a| a.0.contains(device))
    }
}

/// Which style of input the app is in: whichever was used last, as on TVs and
/// consoles with a pointer.
///
/// The focused style shows only while navigating, and the hover style only
/// while pointing, so a pointer resting on one component while the keys move
/// to another doesn't show two highlights.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// The pointer was used last: moved or pressed.
    #[default]
    Pointing,
    /// One of the [`NavigatingActions`] was used last.
    Navigating,
}

/// The actions whose keys switch the app to [`InputMode::Navigating`]. By
/// default the six that move focus, so the arrows and Tab switch it, and
/// Enter, Space, Escape and typing don't.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct NavigatingActions(pub Vec<Action>);

impl Default for NavigatingActions {
    fn default() -> Self {
        Self(Action::MOVING_FOCUS.to_vec())
    }
}
