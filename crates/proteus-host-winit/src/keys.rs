//! Turning winit's key events into Proteus's.

use proteus_runtime::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use winit::event::ElementState;
use winit::keyboard::{self, KeyCode, ModifiersState, PhysicalKey};

/// Matches `$value`: first the special arms, then each listed variant of
/// `$from` to the variant of `$to` with the same name, and anything else to
/// `$fallback`.
macro_rules! same_names {
    ($value:expr, $from:ident => $to:ident, [$($special:pat => $result:expr,)*], [$($name:ident),+ $(,)?], $fallback:expr) => {
        match $value {
            $($special => $result,)*
            $($from::$name => Some($to::$name),)+
            _ => $fallback,
        }
    };
}

/// The [`KeyEvent`] for winit's `event`, with the modifiers held at the time.
pub(crate) fn key_event(event: &winit::event::KeyEvent, modifiers: Modifiers) -> KeyEvent {
    let state = key_state(event.state, event.repeat);
    let text = match state {
        KeyState::Released => None,
        _ => event.text.as_ref().map(|text| text.to_string()),
    };
    KeyEvent::new(
        key(&event.logical_key),
        code(event.physical_key),
        state,
        modifiers,
        text,
    )
}

pub(crate) fn key_state(state: ElementState, repeat: bool) -> KeyState {
    match (state, repeat) {
        (ElementState::Pressed, false) => KeyState::Pressed,
        (ElementState::Pressed, true) => KeyState::Repeated,
        (ElementState::Released, _) => KeyState::Released,
    }
}

pub(crate) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        meta: state.super_key(),
    }
}

pub(crate) fn key(key: &keyboard::Key) -> Key {
    match key {
        keyboard::Key::Named(named) => named_key(*named),
        keyboard::Key::Character(text) => Key::Character(text.to_string()),
        keyboard::Key::Unidentified(_) => Key::Named(NamedKey::Unidentified),
        keyboard::Key::Dead(_) => Key::Named(NamedKey::Dead),
    }
}

fn named_key(named: keyboard::NamedKey) -> Key {
    use keyboard::NamedKey as W;
    let mapped = same_names!(named, W => NamedKey, [
        // The web names the space bar by the character it types, and the
        // Windows and Command keys Meta.
        W::Space => return Key::Character(" ".to_owned()),
        W::Super => Some(NamedKey::Meta),
    ], [
        Process, Alt, AltGraph, CapsLock, Control, Fn, Meta, NumLock, ScrollLock, Shift,
        Enter, Tab, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, End, Home, PageDown, PageUp,
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
    ], None);
    match mapped {
        Some(named) => Key::Named(named),
        // winit's names follow the web's, so its name for a key Proteus has
        // no variant for is the web's name too.
        None => Key::Other(format!("{named:?}")),
    }
}

pub(crate) fn code(key: PhysicalKey) -> Code {
    let PhysicalKey::Code(winit_code) = key else {
        return Code::Unidentified;
    };
    use KeyCode as W;
    same_names!(winit_code, W => Code, [
        W::SuperLeft => Some(Code::MetaLeft),
        W::SuperRight => Some(Code::MetaRight),
    ], [
        Backquote, Backslash, BracketLeft, BracketRight, Comma, Equal, IntlBackslash, Minus,
        Period, Quote, Semicolon, Slash,
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
        KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
        AltLeft, AltRight, Backspace, CapsLock, ContextMenu, ControlLeft, ControlRight, Enter,
        ShiftLeft, ShiftRight, Space, Tab,
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
    ], None)
    .unwrap_or_else(|| Code::Other(format!("{winit_code:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{NativeKey, NativeKeyCode};

    #[test]
    fn named_keys_keep_their_names() {
        assert_eq!(
            key(&keyboard::Key::Named(keyboard::NamedKey::ArrowUp)),
            Key::Named(NamedKey::ArrowUp)
        );
        assert_eq!(
            key(&keyboard::Key::Named(keyboard::NamedKey::ColorF0Red)),
            Key::Named(NamedKey::ColorF0Red)
        );
    }

    // The web calls the space bar " " and the Command and Windows keys Meta;
    // winit calls them Space and Super. Apps see the web's names on every host.
    #[test]
    fn space_and_super_take_the_webs_names() {
        assert_eq!(
            key(&keyboard::Key::Named(keyboard::NamedKey::Space)),
            Key::Character(" ".to_owned())
        );
        assert_eq!(
            key(&keyboard::Key::Named(keyboard::NamedKey::Super)),
            Key::Named(NamedKey::Meta)
        );
        assert_eq!(code(PhysicalKey::Code(KeyCode::SuperLeft)), Code::MetaLeft);
        assert_eq!(
            code(PhysicalKey::Code(KeyCode::SuperRight)),
            Code::MetaRight
        );
    }

    #[test]
    fn a_named_key_without_a_variant_keeps_its_name() {
        assert_eq!(
            key(&keyboard::Key::Named(keyboard::NamedKey::TVPower)),
            Key::Other("TVPower".to_owned())
        );
        assert_eq!(
            code(PhysicalKey::Code(KeyCode::Lang1)),
            Code::Other("Lang1".to_owned())
        );
    }

    #[test]
    fn characters_unidentified_and_dead_keys() {
        assert_eq!(
            key(&keyboard::Key::Character("é".into())),
            Key::Character("é".to_owned())
        );
        assert_eq!(
            key(&keyboard::Key::Unidentified(NativeKey::Unidentified)),
            Key::Named(NamedKey::Unidentified)
        );
        assert_eq!(
            key(&keyboard::Key::Dead(Some('´'))),
            Key::Named(NamedKey::Dead)
        );
        assert_eq!(
            code(PhysicalKey::Unidentified(NativeKeyCode::Unidentified)),
            Code::Unidentified
        );
    }

    #[test]
    fn codes_keep_their_names() {
        assert_eq!(code(PhysicalKey::Code(KeyCode::KeyW)), Code::KeyW);
        assert_eq!(code(PhysicalKey::Code(KeyCode::Space)), Code::Space);
        assert_eq!(code(PhysicalKey::Code(KeyCode::F24)), Code::F24);
    }

    #[test]
    fn a_held_key_repeats() {
        assert_eq!(key_state(ElementState::Pressed, false), KeyState::Pressed);
        assert_eq!(key_state(ElementState::Pressed, true), KeyState::Repeated);
        assert_eq!(key_state(ElementState::Released, false), KeyState::Released);
    }

    #[test]
    fn super_is_meta() {
        let state = ModifiersState::SHIFT | ModifiersState::SUPER;
        assert_eq!(
            modifiers(state),
            Modifiers {
                shift: true,
                meta: true,
                ..Modifiers::default()
            }
        );
    }
}
