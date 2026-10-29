//! Keyboard input: the keys every host reports, and this tick's key events.
//!
//! Hosts turn their platform's key events into [`KeyEvent`]s, named as the
//! web names them, and record them in [`KeyInput`] before each tick. Every
//! device's keys arrive as the same values, whether from winit, a browser's
//! `KeyboardEvent` or a TV remote.
//!
//! A key is named two ways:
//!
//! - [`Key`], what it means: `Enter`, `ArrowUp`, or the character `"z"`. It
//!   follows the keyboard layout and the modifiers.
//! - [`Code`], where it is: `KeyW` is the key in the W position of a US
//!   keyboard, whatever the layout prints on it.

use bevy_ecs::prelude::*;

use crate::keymap::Action;

/// Declares an enum of web key names whose variants are named exactly as the
/// web names them, with the name of each, and optionally a variant holding the
/// name of any other.
macro_rules! web_names {
    (
        $(#[$attr:meta])*
        pub enum $name:ident {
            $($variant:ident),+ $(,)?
        }
        $(
            $(#[$other_attr:meta])*
            other $other:ident
        )?
    ) => {
        $(#[$attr])*
        pub enum $name {
            $(
                #[doc = concat!("`", stringify!($variant), "`.")]
                $variant,
            )+
            $(
                $(#[$other_attr])*
                $other(String),
            )?
        }

        impl $name {
            /// Its name, as the web names it.
            pub fn web_name(&self) -> &str {
                match self {
                    $(Self::$variant => stringify!($variant),)+
                    $(Self::$other(name) => name,)?
                }
            }

            fn from_known_name(name: &str) -> Option<Self> {
                match name {
                    $(stringify!($variant) => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

web_names! {
    /// A key with a name of its own, rather than a character it types.
    ///
    /// Each is named as the web names it. More may be added, so a `match` on it
    /// outside this crate needs a `_` arm. A named key that isn't here arrives
    /// as [`Key::Other`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[non_exhaustive]
    pub enum NamedKey {
        Unidentified, Dead, Process,
        Alt, AltGraph, CapsLock, Control, Fn, Meta, NumLock, ScrollLock, Shift,
        Enter, Tab,
        ArrowDown, ArrowLeft, ArrowRight, ArrowUp, End, Home, PageDown, PageUp,
        Backspace, Clear, Copy, Cut, Delete, Insert, Paste, Redo, Undo,
        ContextMenu, Escape, Find, Help, Pause, Play, Select, ZoomIn, ZoomOut,
        BrightnessDown, BrightnessUp, Eject, Power, PrintScreen,
        MediaFastForward, MediaPause, MediaPlay, MediaPlayPause, MediaRecord, MediaRewind,
        MediaStop, MediaTrackNext, MediaTrackPrevious,
        AudioVolumeDown, AudioVolumeMute, AudioVolumeUp,
        BrowserBack, BrowserForward, BrowserHome, BrowserRefresh, BrowserSearch,
        GoBack, GoHome,
        ChannelDown, ChannelUp, ColorF0Red, ColorF1Green, ColorF2Yellow, ColorF3Blue,
        Exit, Guide, Info, Settings, Subtitle,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
        F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    }
}

web_names! {
    /// Where a key is on the keyboard, whatever the layout prints on it.
    ///
    /// Each is named as the web names it, after the key in that position on a
    /// US keyboard: `KeyW` is the key between `KeyQ` and `KeyE` on any layout.
    /// Use it for keys chosen by position, such as W, A, S and D for movement.
    /// More may be added, so a `match` on it outside this crate needs a `_`
    /// arm.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    #[non_exhaustive]
    pub enum Code {
        Unidentified,
        Backquote, Backslash, BracketLeft, BracketRight, Comma, Equal, IntlBackslash, Minus,
        Period, Quote, Semicolon, Slash,
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
        KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
        AltLeft, AltRight, Backspace, CapsLock, ContextMenu, ControlLeft, ControlRight, Enter,
        MetaLeft, MetaRight, ShiftLeft, ShiftRight, Space, Tab,
        Delete, End, Help, Home, Insert, PageDown, PageUp,
        ArrowDown, ArrowLeft, ArrowRight, ArrowUp,
        NumLock, Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7,
        Numpad8, Numpad9, NumpadAdd, NumpadDecimal, NumpadDivide, NumpadEnter, NumpadEqual,
        NumpadMultiply, NumpadSubtract,
        Escape, Fn, PrintScreen, ScrollLock, Pause,
        AudioVolumeDown, AudioVolumeMute, AudioVolumeUp, MediaPlayPause, MediaStop,
        MediaTrackNext, MediaTrackPrevious, BrowserBack, BrowserForward, BrowserHome,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
        F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    }
    /// Any other position, by the platform's name for it.
    other Other
}

impl Code {
    /// The code the web names `name`, as in a `KeyboardEvent`'s `code`. A
    /// name without a variant of its own becomes [`Code::Other`], and an empty
    /// one, as some browsers give a key they can't place, is
    /// [`Code::Unidentified`].
    pub fn from_web_name(name: &str) -> Self {
        if name.is_empty() {
            return Self::Unidentified;
        }
        Self::from_known_name(name).unwrap_or_else(|| Self::Other(name.to_owned()))
    }
}

/// What a key means: a named key, or the character it types.
///
/// It follows the keyboard layout and the modifiers: with Shift held, the key
/// that types `"a"` types `"A"`. The space bar is the character `" "`, as on
/// the web.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// A key with a name of its own, such as Enter or an arrow.
    Named(NamedKey),
    /// A key that types a character, such as `"a"`, `"é"` or `" "`.
    Character(String),
    /// Any other named key, by the platform's name for it.
    Other(String),
}

impl Key {
    /// The key the web names `name`, as in a `KeyboardEvent`'s `key`.
    ///
    /// A name of the form the web gives named keys, a capital letter followed
    /// by letters and digits, is a named key: [`Key::Named`] when it has a
    /// variant, [`Key::Other`] when it doesn't. Anything else is the character
    /// it types. An empty name is [`NamedKey::Unidentified`].
    pub fn from_web_name(name: &str) -> Self {
        if name.is_empty() {
            return Self::Named(NamedKey::Unidentified);
        }
        if let Some(named) = NamedKey::from_known_name(name) {
            return Self::Named(named);
        }
        let mut chars = name.chars();
        let looks_named = name.len() > 1
            && chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_alphanumeric());
        if looks_named {
            Self::Other(name.to_owned())
        } else {
            Self::Character(name.to_owned())
        }
    }

    /// Its name, as the web names it: the character itself for
    /// [`Key::Character`].
    pub fn web_name(&self) -> &str {
        match self {
            Self::Named(named) => named.web_name(),
            Self::Character(text) | Self::Other(text) => text,
        }
    }
}

/// The modifier keys held during a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub control: bool,
    /// Alt, or Option on a Mac.
    pub alt: bool,
    /// Meta: the Windows key, or Command on a Mac.
    pub meta: bool,
}

/// What happened to a key.
///
/// More may be added, so a `match` on it outside this crate needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyState {
    /// The key was pressed.
    Pressed,
    /// The key is still held, and repeated; see [`KeyRepeat`].
    Repeated,
    /// The key has been held for the long-press time, once per press; see
    /// [`KeyInput::long_press_ms`].
    LongPressed,
    /// The key was released.
    Released,
}

/// How held keys repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRepeat {
    /// The engine repeats held keys itself, the same on every platform and
    /// device, and ignores the system's repeats.
    Engine {
        /// How long a key is held before it first repeats, in milliseconds.
        delay_ms: u32,
        /// How long between repeats after that, in milliseconds.
        interval_ms: u32,
    },
    /// Keys repeat as the system repeats them, at the speed each user set.
    /// Also for devices that send a press and then repeats, but no release.
    System,
}

impl Default for KeyRepeat {
    /// The engine's repeats: first after 250 ms, then every 100 ms.
    fn default() -> Self {
        Self::Engine {
            delay_ms: 250,
            interval_ms: 100,
        }
    }
}

/// A key being pressed, repeated, long-pressed or released.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// What the key means.
    pub key: Key,
    /// Where the key is.
    pub code: Code,
    /// What happened to it.
    pub state: KeyState,
    /// The modifiers held at the time.
    pub modifiers: Modifiers,
    /// The text the key types, if any. Only a press or a repeat types text.
    pub text: Option<String>,
    /// The action the key stands for in the [`KeyMap`](crate::keymap::KeyMap),
    /// if it's bound. The engine sets it before key callbacks see the event.
    pub action: Option<Action>,
    /// On a release, whether the key was long-pressed before it, so a tap can
    /// be told from a hold. `false` on any other event.
    pub after_long_press: bool,
}

impl KeyEvent {
    /// An event of `key`, at position `code`, with no action yet.
    pub fn new(
        key: Key,
        code: Code,
        state: KeyState,
        modifiers: Modifiers,
        text: Option<String>,
    ) -> Self {
        Self {
            key,
            code,
            state,
            modifiers,
            text,
            action: None,
            after_long_press: false,
        }
    }

    /// Whether `self` and `other` are events of the same key: the same
    /// [`Code`], or, for a key whose position is unknown, the same [`Key`].
    pub fn is_same_key(&self, other: &KeyEvent) -> bool {
        if self.code == Code::Unidentified {
            self.key == other.key
        } else {
            self.code == other.code
        }
    }

    /// This event's key in `state`, with its modifiers and text.
    fn with_state(&self, state: KeyState) -> KeyEvent {
        KeyEvent {
            state,
            action: None,
            after_long_press: false,
            ..self.clone()
        }
    }
}

/// A key still held: its press, and how long it has been held.
#[derive(Debug)]
struct Held {
    press: KeyEvent,
    held_secs: f32,
    // When it repeats next, in seconds held.
    next_repeat_secs: f32,
    long_pressed: bool,
}

/// This tick's key events, written by the host before each tick, the keys
/// still held, and how they repeat and long-press.
///
/// Whoever drives the world clears the events after each tick, with
/// [`KeyInput::clear_events`], then calls [`KeyInput::advance`], whose
/// repeats and long presses go to the next tick; `Proteus::tick` does both.
#[derive(Resource, Debug)]
pub struct KeyInput {
    /// This tick's key events, in the order they happened.
    pub events: Vec<KeyEvent>,
    /// How many of [`KeyInput::events`], from the first, have gone to the
    /// app's key callbacks. `Proteus::tick` sends the rest at its start.
    pub dispatched: usize,
    /// How held keys repeat.
    pub repeat: KeyRepeat,
    /// How long a key is held before it's long-pressed, in milliseconds.
    /// 500 by default.
    pub long_press_ms: u32,
    held: Vec<Held>,
}

impl Default for KeyInput {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            dispatched: 0,
            repeat: KeyRepeat::default(),
            long_press_ms: 500,
            held: Vec::new(),
        }
    }
}

impl KeyInput {
    /// Records `event`.
    ///
    /// A repeat or a release of a key that isn't held, such as one pressed
    /// before the window had focus, is ignored, so every release follows a
    /// press. So is every repeat while the engine repeats keys itself. A
    /// release says whether the key was long-pressed.
    pub fn record(&mut self, mut event: KeyEvent) {
        let held = self.held.iter().position(|h| h.press.is_same_key(&event));
        match event.state {
            KeyState::Pressed => {
                // A press of a key already held means its release was never
                // reported, so this press replaces it.
                if let Some(i) = held {
                    self.held.remove(i);
                }
                let next_repeat_secs = match self.repeat {
                    KeyRepeat::Engine { delay_ms, .. } => delay_ms as f32 / 1000.0,
                    KeyRepeat::System => 0.0,
                };
                self.held.push(Held {
                    press: event.clone(),
                    held_secs: 0.0,
                    next_repeat_secs,
                    long_pressed: false,
                });
            }
            KeyState::Repeated => {
                if held.is_none() || self.repeat != KeyRepeat::System {
                    return;
                }
            }
            KeyState::Released => match held {
                Some(i) => {
                    event.after_long_press = self.held.remove(i).long_pressed;
                }
                None => return,
            },
            _ => return,
        }
        self.events.push(event);
    }

    /// Advances the time keys have been held by `dt` seconds, adding the
    /// repeats and long presses it brings, after the events already recorded.
    ///
    /// Repeats and long presses are accurate to one call. At most one repeat
    /// is added per key per call, so a long `dt`, such as after the app was in
    /// the background, doesn't add a burst of them.
    pub fn advance(&mut self, dt: f32) {
        let long_press_secs = self.long_press_ms as f32 / 1000.0;
        let repeat = self.repeat;
        let mut added = Vec::new();
        for held in &mut self.held {
            held.held_secs += dt;
            if let KeyRepeat::Engine { interval_ms, .. } = repeat {
                if held.held_secs >= held.next_repeat_secs {
                    added.push(held.press.with_state(KeyState::Repeated));
                    let interval = (interval_ms as f32 / 1000.0).max(f32::EPSILON);
                    held.next_repeat_secs =
                        (held.next_repeat_secs + interval).max(held.held_secs + interval / 2.0);
                }
            }
            if !held.long_pressed && held.held_secs >= long_press_secs {
                held.long_pressed = true;
                added.push(held.press.with_state(KeyState::LongPressed));
            }
        }
        self.events.extend(added);
    }

    /// Clears this tick's events, once every system and callback has seen
    /// them.
    pub fn clear_events(&mut self) {
        self.events.clear();
        self.dispatched = 0;
    }

    /// Releases every key still held, as when the window or canvas loses
    /// focus and its releases would never arrive.
    ///
    /// Each release has the key and code of its press, no modifiers and no
    /// text.
    pub fn release_all(&mut self) {
        for held in std::mem::take(&mut self.held) {
            self.events.push(KeyEvent {
                state: KeyState::Released,
                modifiers: Modifiers::default(),
                text: None,
                action: None,
                after_long_press: held.long_pressed,
                ..held.press
            });
        }
    }
}
