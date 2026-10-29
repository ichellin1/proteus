//! The key map: which keys stand for which actions.
//!
//! Apps and navigation respond to [`Action`]s, such as [`Action::Activate`],
//! rather than to keys. The [`KeyMap`] binds keys to them: Enter and Space to
//! `Activate`, the arrows to `Up`, `Down`, `Left` and `Right`, and so on. Other
//! devices, such as TV remotes, add bindings to the same actions, so an app
//! that handles actions works with each of them.
//!
//! A key the map doesn't bind still reaches the app, as a key event without an
//! action, for typing and for keys no action covers.

use bevy_ecs::prelude::*;

use crate::keyboard::{Code, Key, KeyEvent, Modifiers, NamedKey};

/// An action an app defined itself, with [`KeyMap::define_action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppAction(u32);

/// Something a key stands for, which apps and navigation respond to.
///
/// More built-in actions may be added, so a `match` on it outside this crate
/// needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Action {
    /// Move focus up. The up arrow.
    Up,
    /// Move focus down. The down arrow.
    Down,
    /// Move focus left. The left arrow.
    Left,
    /// Move focus right. The right arrow.
    Right,
    /// Move focus to the next component. Tab.
    Next,
    /// Move focus to the previous component. Shift+Tab.
    Previous,
    /// Choose the focused component, as a click does. Enter and Space.
    Activate,
    /// Go back. Escape and Backspace. The app decides what it means.
    Back,
    /// An action the app defined.
    App(AppAction),
}

impl Action {
    /// The built-in actions.
    pub const BUILT_IN: [Action; 8] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Next,
        Action::Previous,
        Action::Activate,
        Action::Back,
    ];

    /// Whether it moves focus: `Up`, `Down`, `Left`, `Right`, `Next` or
    /// `Previous`.
    pub fn is_navigation(self) -> bool {
        matches!(
            self,
            Action::Up
                | Action::Down
                | Action::Left
                | Action::Right
                | Action::Next
                | Action::Previous
        )
    }
}

/// A key, by what it means or by where it is, that a [`KeyBinding`] matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BoundKey {
    /// The key that means this, whatever its position.
    Key(Key),
    /// The key in this position, whatever the layout prints on it.
    Code(Code),
}

/// A key and the modifiers held with it, bound to an action in the
/// [`KeyMap`].
///
/// The modifiers must match exactly: a binding for Tab doesn't match
/// Shift+Tab.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    /// The key.
    pub key: BoundKey,
    /// The modifiers held with it.
    pub modifiers: Modifiers,
}

impl KeyBinding {
    /// A binding for the key that means `key`, with no modifiers.
    pub fn key(key: Key) -> Self {
        Self {
            key: BoundKey::Key(key),
            modifiers: Modifiers::default(),
        }
    }

    /// A binding for the named key `key`, with no modifiers.
    pub fn named(key: NamedKey) -> Self {
        Self::key(Key::Named(key))
    }

    /// A binding for the key in position `code`, with no modifiers.
    pub fn code(code: Code) -> Self {
        Self {
            key: BoundKey::Code(code),
            modifiers: Modifiers::default(),
        }
    }

    /// This binding with `modifiers` held.
    pub fn with(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Whether `event` is of this binding's key, with exactly its modifiers.
    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.modifiers == event.modifiers
            && match &self.key {
                BoundKey::Key(key) => *key == event.key,
                BoundKey::Code(code) => *code == event.code,
            }
    }
}

/// Which keys stand for which actions, and the actions the app defined.
///
/// It starts with the default bindings; see [`KeyMap::default_bindings`]. An
/// app can bind more keys, rebind or unbind them, and define actions of its
/// own, at any time.
#[derive(Resource, Debug, Clone)]
pub struct KeyMap {
    // Later bindings win over earlier ones that match the same event.
    bindings: Vec<(KeyBinding, Action)>,
    // The names of the app's actions; an `AppAction` is an index into it.
    action_names: Vec<String>,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self {
            bindings: Self::default_bindings(),
            action_names: Vec::new(),
        }
    }
}

impl KeyMap {
    /// The bindings a key map starts with: the arrows to `Up`, `Down`, `Left`
    /// and `Right`; Tab to `Next` and Shift+Tab to `Previous`; Enter and Space
    /// to `Activate`; and Escape and Backspace to `Back`.
    pub fn default_bindings() -> Vec<(KeyBinding, Action)> {
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        vec![
            (KeyBinding::named(NamedKey::ArrowUp), Action::Up),
            (KeyBinding::named(NamedKey::ArrowDown), Action::Down),
            (KeyBinding::named(NamedKey::ArrowLeft), Action::Left),
            (KeyBinding::named(NamedKey::ArrowRight), Action::Right),
            (KeyBinding::named(NamedKey::Tab), Action::Next),
            (
                KeyBinding::named(NamedKey::Tab).with(shift),
                Action::Previous,
            ),
            (KeyBinding::named(NamedKey::Enter), Action::Activate),
            (
                KeyBinding::key(Key::Character(" ".to_owned())),
                Action::Activate,
            ),
            (KeyBinding::named(NamedKey::Escape), Action::Back),
            (KeyBinding::named(NamedKey::Backspace), Action::Back),
        ]
    }

    /// Binds `binding` to `action`, replacing what it was bound to.
    pub fn bind(&mut self, binding: KeyBinding, action: Action) {
        self.unbind(&binding);
        self.bindings.push((binding, action));
    }

    /// Unbinds `binding`, and returns the action it was bound to, if any.
    pub fn unbind(&mut self, binding: &KeyBinding) -> Option<Action> {
        let i = self.bindings.iter().position(|(b, _)| b == binding)?;
        Some(self.bindings.remove(i).1)
    }

    /// Puts back the default bindings, removing all the others. The app's actions
    /// stay defined.
    pub fn reset(&mut self) {
        self.bindings = Self::default_bindings();
    }

    /// The bindings, in the order they were made.
    pub fn bindings(&self) -> impl Iterator<Item = (&KeyBinding, Action)> {
        self.bindings.iter().map(|(b, a)| (b, *a))
    }

    /// The action `event` stands for, if its key is bound. When several
    /// bindings match, as one by meaning and one by position can, the one made
    /// last wins.
    pub fn action_for(&self, event: &KeyEvent) -> Option<Action> {
        self.bindings
            .iter()
            .rev()
            .find(|(b, _)| b.matches(event))
            .map(|(_, a)| *a)
    }

    /// Defines an action of the app's own, named `name`, to bind keys to and
    /// match in key callbacks. Defining a name again returns the same action.
    pub fn define_action(&mut self, name: &str) -> AppAction {
        let i = match self.action_names.iter().position(|n| n == name) {
            Some(i) => i,
            None => {
                self.action_names.push(name.to_owned());
                self.action_names.len() - 1
            }
        };
        AppAction(i as u32)
    }

    /// The app's action named `name`, if it defined one.
    pub fn app_action(&self, name: &str) -> Option<AppAction> {
        self.action_names
            .iter()
            .position(|n| n == name)
            .map(|i| AppAction(i as u32))
    }

    /// The name `action` was defined with, if this map defined it.
    pub fn action_name(&self, action: AppAction) -> Option<&str> {
        self.action_names.get(action.0 as usize).map(String::as_str)
    }
}

/// The navigation actions no key callback claimed this tick, in order, for
/// navigation to move focus with.
#[derive(Resource, Debug, Default)]
pub struct NavigationRequests(pub Vec<Action>);
