// Tests of the key map: its default bindings, exact modifiers, rebinding, and
// the app's own actions.

use proteus_ui::{Action, Code, Key, KeyBinding, KeyEvent, KeyMap, KeyState, Modifiers, NamedKey};

fn press(key: Key, code: Code, modifiers: Modifiers) -> KeyEvent {
    KeyEvent::new(key, code, KeyState::Pressed, modifiers, None)
}

fn named(key: NamedKey, code: Code) -> KeyEvent {
    press(Key::Named(key), code, Modifiers::default())
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

#[test]
fn default_bindings() {
    let map = KeyMap::default();
    let cases = [
        (named(NamedKey::ArrowUp, Code::ArrowUp), Action::Up),
        (named(NamedKey::ArrowRight, Code::ArrowRight), Action::Right),
        (named(NamedKey::Tab, Code::Tab), Action::Next),
        (named(NamedKey::Enter, Code::Enter), Action::Activate),
        (named(NamedKey::Escape, Code::Escape), Action::Back),
        (named(NamedKey::Backspace, Code::Backspace), Action::Back),
        (
            press(
                Key::Character(" ".to_owned()),
                Code::Space,
                Modifiers::default(),
            ),
            Action::Activate,
        ),
        (
            press(Key::Named(NamedKey::Tab), Code::Tab, shift()),
            Action::Previous,
        ),
    ];
    for (event, action) in cases {
        assert_eq!(map.action_for(&event), Some(action), "{:?}", event.key);
    }
}

// Shift+Enter isn't Enter: a binding's modifiers must match exactly, so an app
// can give Shift+Enter its own meaning.
#[test]
fn modifiers_must_match_exactly() {
    let map = KeyMap::default();
    let shift_enter = press(Key::Named(NamedKey::Enter), Code::Enter, shift());
    assert_eq!(map.action_for(&shift_enter), None);
}

#[test]
fn an_unbound_key_has_no_action() {
    let map = KeyMap::default();
    let a = press(
        Key::Character("a".to_owned()),
        Code::KeyA,
        Modifiers::default(),
    );
    assert_eq!(map.action_for(&a), None);
}

#[test]
fn binding_a_key_again_replaces_its_action() {
    let mut map = KeyMap::default();
    map.bind(KeyBinding::named(NamedKey::Enter), Action::Back);
    assert_eq!(
        map.action_for(&named(NamedKey::Enter, Code::Enter)),
        Some(Action::Back)
    );
    assert_eq!(
        map.bindings()
            .filter(|(b, _)| **b == KeyBinding::named(NamedKey::Enter))
            .count(),
        1
    );
}

#[test]
fn unbinding_and_resetting() {
    let mut map = KeyMap::default();
    let escape = named(NamedKey::Escape, Code::Escape);
    assert_eq!(
        map.unbind(&KeyBinding::named(NamedKey::Escape)),
        Some(Action::Back)
    );
    assert_eq!(map.action_for(&escape), None);

    map.reset();
    assert_eq!(map.action_for(&escape), Some(Action::Back));
}

// A binding by position and one by meaning can both match the same key; the
// one made last is the one the app meant.
#[test]
fn the_binding_made_last_wins() {
    let mut map = KeyMap::default();
    let search = Action::App(map.define_action("search"));
    map.bind(KeyBinding::code(Code::Enter), search);
    assert_eq!(
        map.action_for(&named(NamedKey::Enter, Code::Enter)),
        Some(search)
    );
}

#[test]
fn app_actions_are_found_by_name() {
    let mut map = KeyMap::default();
    let search = map.define_action("search");
    assert_eq!(map.define_action("search"), search);
    assert_ne!(map.define_action("menu"), search);
    assert_eq!(map.app_action("search"), Some(search));
    assert_eq!(map.action_name(search), Some("search"));
    assert_eq!(map.app_action("unknown"), None);
}
