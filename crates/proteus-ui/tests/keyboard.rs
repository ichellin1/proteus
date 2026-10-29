// Tests of keyboard input: turning the web's key names into keys, and
// `KeyInput`'s record of a tick's key events and the keys held.

use proteus_ui::{Code, Key, KeyEvent, KeyInput, KeyState, Modifiers, NamedKey};

fn event(key: Key, code: Code, state: KeyState) -> KeyEvent {
    KeyEvent {
        key,
        code,
        state,
        modifiers: Modifiers::default(),
        text: None,
    }
}

fn enter(state: KeyState) -> KeyEvent {
    event(Key::Named(NamedKey::Enter), Code::Enter, state)
}

fn letter(c: &str, code: Code, state: KeyState) -> KeyEvent {
    event(Key::Character(c.to_owned()), code, state)
}

// ---------------------------------------------------------------------------
// web names
// ---------------------------------------------------------------------------

#[test]
fn named_keys_from_web_names() {
    assert_eq!(Key::from_web_name("ArrowUp"), Key::Named(NamedKey::ArrowUp));
    assert_eq!(Key::from_web_name("Escape"), Key::Named(NamedKey::Escape));
    assert_eq!(Key::from_web_name("F12"), Key::Named(NamedKey::F12));
    assert_eq!(
        Key::from_web_name("ColorF0Red"),
        Key::Named(NamedKey::ColorF0Red)
    );
}

#[test]
fn characters_from_web_names() {
    assert_eq!(Key::from_web_name("a"), Key::Character("a".to_owned()));
    assert_eq!(Key::from_web_name("A"), Key::Character("A".to_owned()));
    assert_eq!(Key::from_web_name("é"), Key::Character("é".to_owned()));
    assert_eq!(Key::from_web_name(" "), Key::Character(" ".to_owned()));
    assert_eq!(Key::from_web_name("/"), Key::Character("/".to_owned()));
}

// A named key Proteus has no variant for must still reach the app, by name,
// rather than being taken for a character it types.
#[test]
fn a_named_key_without_a_variant_keeps_its_name() {
    assert_eq!(
        Key::from_web_name("TVPower"),
        Key::Other("TVPower".to_owned())
    );
    assert_eq!(
        Code::from_web_name("Lang1"),
        Code::Other("Lang1".to_owned())
    );
}

#[test]
fn empty_names_are_unidentified() {
    assert_eq!(Key::from_web_name(""), Key::Named(NamedKey::Unidentified));
    assert_eq!(Code::from_web_name(""), Code::Unidentified);
    assert_eq!(
        Key::from_web_name("Unidentified"),
        Key::Named(NamedKey::Unidentified)
    );
}

#[test]
fn codes_from_web_names() {
    assert_eq!(Code::from_web_name("KeyW"), Code::KeyW);
    assert_eq!(Code::from_web_name("Space"), Code::Space);
    assert_eq!(Code::from_web_name("MetaLeft"), Code::MetaLeft);
}

#[test]
fn web_names_round_trip() {
    for name in ["ArrowDown", "a", " ", "TVPower"] {
        assert_eq!(Key::from_web_name(name).web_name(), name);
    }
    for name in ["KeyA", "NumpadEnter", "Lang1"] {
        assert_eq!(Code::from_web_name(name).web_name(), name);
    }
}

// ---------------------------------------------------------------------------
// KeyInput
// ---------------------------------------------------------------------------

#[test]
fn events_keep_the_order_they_happened_in() {
    let mut input = KeyInput::default();
    input.record(letter("a", Code::KeyA, KeyState::Pressed));
    input.record(enter(KeyState::Pressed));
    input.record(letter("a", Code::KeyA, KeyState::Released));
    input.record(enter(KeyState::Repeated));

    let states: Vec<_> = input
        .events
        .iter()
        .map(|e| (e.code.clone(), e.state))
        .collect();
    assert_eq!(
        states,
        vec![
            (Code::KeyA, KeyState::Pressed),
            (Code::Enter, KeyState::Pressed),
            (Code::KeyA, KeyState::Released),
            (Code::Enter, KeyState::Repeated),
        ]
    );
}

// A key pressed before the window had focus reports only its repeats and its
// release. Ignoring them means every release has a press before it.
#[test]
fn repeats_and_releases_of_a_key_not_held_are_ignored() {
    let mut input = KeyInput::default();
    input.record(enter(KeyState::Repeated));
    input.record(enter(KeyState::Released));
    assert!(input.events.is_empty());
}

// With Shift pressed after it, a key that typed "a" is released as "A". The
// release is matched by position, so it still releases the held key.
#[test]
fn a_release_is_matched_to_its_press_by_position() {
    let mut input = KeyInput::default();
    input.record(letter("a", Code::KeyA, KeyState::Pressed));
    input.record(letter("A", Code::KeyA, KeyState::Released));
    input.events.clear();

    input.release_all();
    assert!(input.events.is_empty(), "the key is no longer held");
}

#[test]
fn keys_of_unknown_position_are_matched_by_meaning() {
    let mut input = KeyInput::default();
    let red = || Key::Named(NamedKey::ColorF0Red);
    let green = || Key::Named(NamedKey::ColorF1Green);
    input.record(event(red(), Code::Unidentified, KeyState::Pressed));
    input.record(event(green(), Code::Unidentified, KeyState::Pressed));
    input.record(event(red(), Code::Unidentified, KeyState::Released));
    input.events.clear();

    input.release_all();
    let released: Vec<_> = input.events.iter().map(|e| e.key.clone()).collect();
    assert_eq!(released, vec![green()]);
}

#[test]
fn release_all_releases_each_held_key_once() {
    let mut input = KeyInput::default();
    input.record(KeyEvent {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
        text: Some("A".to_owned()),
        ..letter("A", Code::KeyA, KeyState::Pressed)
    });
    input.record(enter(KeyState::Pressed));
    input.record(enter(KeyState::Repeated));
    input.events.clear();

    input.release_all();
    assert_eq!(
        input.events,
        vec![
            letter("A", Code::KeyA, KeyState::Released),
            enter(KeyState::Released),
        ]
    );

    input.events.clear();
    input.release_all();
    assert!(input.events.is_empty(), "nothing is held any more");
}

// A press of a key already held means its release was lost, as when a
// platform doesn't report it. The key is held once, not twice.
#[test]
fn a_second_press_of_a_held_key_replaces_the_first() {
    let mut input = KeyInput::default();
    input.record(enter(KeyState::Pressed));
    input.record(enter(KeyState::Pressed));
    assert_eq!(input.events.len(), 2);
    input.events.clear();

    input.release_all();
    assert_eq!(input.events, vec![enter(KeyState::Released)]);
}
