# Keyboard and focus

Components respond to the keyboard as well as the pointer. One component at a time has focus:
the arrows, Tab and Shift+Tab move it, Enter and Space choose it, as a click does, and keys go to
it first.

An app works with the pointer, the keyboard, or both, and chooses which; see
[Pointer, keyboard or both](#pointer-keyboard-or-both). This guide covers that choice, the keys
and focus. For the pointer, and for how a component looks while it's hovered, pressed, focused or
disabled, see [Interaction](./interaction.md).

## Pointer, keyboard or both

An app works one of three ways, and chooses with `set_input_devices` (`setInputDevices`):

| Way | Devices | What works | Suits |
|---|---|---|---|
| Both, the default | `InputDevices::ALL` (`["pointer", "keyboard"]`) | The pointer and the keys, whichever was used last showing its highlight; see [Pointing and navigating](#pointing-and-navigating). | Desktop and web apps, and a TV with a pointing remote. |
| Pointer only | `InputDevices::POINTER` (`["pointer"]`) | Hover, clicks and drags. Keys are dropped, so nothing takes focus, and Enter and Escape do nothing. | A touch-screen kiosk. |
| Keyboard only | `InputDevices::KEYBOARD` (`["keyboard"]`) | Focus, the arrows, Tab, Enter and the app's own keys. The pointer is dropped, so nothing is hovered or clicked. | A TV app, with a remote whose buttons are bound to the actions. |

#### Rust

```rust
# use proteus_sdk::{InputDevices, Proteus};
# let mut app = Proteus::new();
// A TV app: the keys, or a remote bound to the same actions, and no pointer.
app.set_input_devices(InputDevices::KEYBOARD);
```

#### TypeScript

```ts
// A TV app: the keys, or a remote bound to the same actions, and no pointer.
app.setInputDevices(["keyboard"]);
```

Input from a device the app doesn't accept is dropped before any component or callback sees it,
so the app's own key callbacks don't get keys in a pointer-only app either. The choice is made in
code rather than in the config, in `setup` or at any time after: an app can switch to the
keyboard alone while a remote is in use, say. Turning a device off ends what it was doing: the
pointer's hover and press end, and keys held are released.

A keyboard-only app is always navigating, so the focused component shows its focused style from
the start, and the first arrow moves focus, as on a TV.

### Pointing and navigating

In an app that accepts both, the last input wins. The app is either pointing, after the pointer
moved or was pressed, or navigating, after a key bound to one of the navigating actions was
pressed: the arrows and Tab, by default. The focused style shows only while navigating, and the
hover style only while pointing, as on TVs and consoles with a pointer, and as CSS's
`:focus-visible` does in a browser. So a pointer resting on one component while the keys move to
another doesn't make both look chosen.

`input_mode()` (`inputMode()`) says which, for showing hints for one or the other, and
`set_navigating_actions` (`setNavigatingActions`) changes which actions switch it.

### A component's devices

A component can accept fewer devices than the app, such as a list only the keyboard moves
through while it's being edited, or a control the keys should pass over. It accepts the devices
both it and the app accept:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, InputDevices, Proteus, QuadState};
# let mut app = Proteus::new();
// A component the keys pass over: only the pointer reaches it.
let handle = app.component(ComponentSpec::new(QuadState::default()).devices(InputDevices::POINTER));
# let _ = handle;
```

#### TypeScript

```ts
// A component the keys pass over: only the pointer reaches it.
button.setDevices(["pointer"]);
```

## Actions and keys

Apps respond to actions rather than to keys. The key map binds keys to them:

| Action | Rust | TypeScript | Keys | What it does |
|---|---|---|---|---|
| Up, Down, Left, Right | `Action::Up`… | `"up"`… | The arrows | Move focus to the nearest component that way. |
| Next, Previous | `Action::Next`, `Action::Previous` | `"next"`, `"previous"` | Tab, Shift+Tab | Move focus in reading order. |
| Activate | `Action::Activate` | `"activate"` | Enter, Space | Choose the focused component, as a click does. |
| Back | `Action::Back` | `"back"` | Escape, Backspace | Nothing, until the app says what back means. |

An app that handles actions works with whichever key is bound to them, and with other devices
when they come, such as a TV remote whose buttons are bound to the same actions. Handle a key
itself only for typing, and for keys no action covers.

An app can bind more keys, rebind them, and define actions of its own. Every key has two names,
as in a browser's `KeyboardEvent`: what it types or does with the keyboard layout in use, its
key, such as `/` or `Enter`; and where it is on the keyboard, whatever the layout prints on it,
its code, such as `KeyW`. A binding names one or the other. Bind by code for keys chosen for
where they are, such as W, A, S and D for moving, so they stay in place on any layout. The
modifiers must match exactly: a binding for Tab doesn't match Shift+Tab.

#### Rust

```rust
# use proteus_sdk::{Action, Key, KeyBinding, NamedKey, Proteus};
# let mut app = Proteus::new();
// Delete goes back too.
app.bind(KeyBinding::named(NamedKey::Delete), Action::Back);
// `/` searches: an action of the app's own.
let search = app.define_action("search");
app.bind(KeyBinding::key(Key::Character("/".into())), Action::App(search));
```

#### TypeScript

```ts
// Delete goes back too.
app.bindKey({ key: "Delete" }, "back");
// `/` searches: an action of the app's own, named as it's bound.
app.bindKey({ key: "/" }, "search");
```

`unbind` (`unbindKey`) takes a binding away, so its key arrives without an action, and
`reset_key_map` (`resetKeyMap`) puts back the default bindings alone.

## Handling keys

A key event goes first to the focused component's key callbacks, then to its parents', nearest
first, then to the app's, until one claims it. If none does, the engine does what its action
does: an arrow or Tab moves focus, and Enter or Space clicks the focused component. Claiming a
key stops all of that.

Each event has the key's two names, its key and its code, its action if it's bound, the
modifiers held, the text it types, and what happened to it: pressed, repeated while held,
long-pressed once it's been held for half a second, or released.

A component handles the keys that are about it. Here a tile opens on a tap of Enter, and becomes
a favourite if it's held, as the [browse example](../../examples/browse) does:

#### Rust

```rust
# use proteus_sdk::{Action, ComponentSpec, KeyResult, KeyState, Proteus, QuadState};
# let mut app = Proteus::new();
# let tile = app.component(ComponentSpec::new(QuadState::default()));
tile.on_key(&mut app, move |_app, event| {
    if event.action != Some(Action::Activate) {
        return KeyResult::Ignored;
    }
    match event.state {
        KeyState::LongPressed => { /* make it a favourite */ }
        KeyState::Released if !event.after_long_press => { /* open it */ }
        _ => {}
    }
    // Claimed, so the press doesn't also click the tile.
    KeyResult::Handled
});
```

#### TypeScript

```ts
tile.onKey((event) => {
  if (event.action !== "activate") return false;
  if (event.state === "longPressed") {
    // make it a favourite
  } else if (event.state === "released" && !event.afterLongPress) {
    // open it
  }
  // Claimed, so the press doesn't also click the tile.
  return true;
});
```

The app handles the keys that aren't about any one component, such as back and its own
shortcuts:

#### Rust

```rust
# use proteus_sdk::{Action, KeyResult, KeyState, Proteus};
# let mut app = Proteus::new();
app.on_key(|_app, event| {
    if event.action == Some(Action::Back) && event.state == KeyState::Pressed {
        // go back a screen
        return KeyResult::Handled;
    }
    KeyResult::Ignored
});
```

#### TypeScript

```ts
app.onKey((event) => {
  if (event.action === "back" && event.state === "pressed") {
    // go back a screen
    return true;
  }
  return false;
});
```

Key callbacks run at the start of the frame after the key, before the app's `update`. A
component's key callbacks don't run while it's disabled, non-interactive, doesn't accept the
keyboard, or is transitioning; the key goes on to its parents and the app.

## Focus

### What can take focus

A component can take focus while it's visible, enabled and interactive, it and the app accept the
keyboard, and it isn't transitioning, unless its
[transition interaction setting](./interaction.md#during-a-transition) allows navigation. A
click gives the clicked component focus, as well as the keys.

When the focused component can no longer keep focus, because it was destroyed, hidden, disabled
or made non-interactive, focus is cleared and its `on_blur` fires; see
[Putting focus where it belongs](#putting-focus-where-it-belongs).

### Moving focus

- **The arrows** go to the nearest component in their direction. Those in line with the focused
  one come first. They don't wrap.
- **Tab and Shift+Tab** follow reading order: rows from top to bottom, each left to right, or
  right to left with `set_reading_direction` (`setReadingDirection`). They wrap round from the
  last component to the first; on the web, they leave the canvas instead, unless the app keeps
  them; see [Hosts](./hosts.md#the-keyboard-on-the-web).
- **A named neighbour** wins over the one navigation would choose, for an order the layout
  doesn't suggest, or to wrap round a carousel: `set_neighbour` (`setNeighbour`).

Only components on screen are candidates, except in a focus group focus is in, so a scrolling
row's next item can take focus and the row slide to show it.

With nothing focused, the next arrow or Tab starts from where focus was lost, or the hovered
component, or the one marked as where focus starts, or the first in reading order.

The first arrow or Tab after the pointer was used only shows where focus is, without moving it,
so the user sees where they are before they go anywhere.

### Focus groups

A component can make the components under it a focus group. Focus moves among the group's
members first, and when it would go past the group's edge, the group lets it leave, stops it, or
wraps it round, for each action. Focus entering the group lands on the nearest member, the one
focused last, or the one marked `default_focus` (`defaultFocus`). Groups nest: the innermost
group's rules apply first.

A row of a TV home page keeps Left and Right, lets Up and Down leave, and comes back to where the
user was; a dialog keeps every arrow, and wraps Tab round:

#### Rust

```rust
# use proteus_sdk::{Action, ComponentSpec, FocusEntry, FocusGroup, Proteus, QuadState};
# let mut app = Proteus::new();
# let tiles: Vec<_> = (0..3).map(|_| app.component(ComponentSpec::new(QuadState::default()))).collect();
let mut row = ComponentSpec::new(QuadState::default())
    .non_interactive()
    .focus_group(
        FocusGroup::new()
            .stop(&[Action::Left, Action::Right])
            .entry(FocusEntry::LastFocused),
    );
for &tile in &tiles {
    row = row.child(tile);
}
let row = app.component(row);

let dialog = FocusGroup::new()
    .stop(&[Action::Up, Action::Down, Action::Left, Action::Right])
    .wrap(&[Action::Next, Action::Previous]);
# let _ = (row, dialog);
```

#### TypeScript

```ts
const row = app.component({
  geometry: app.get(card)!.geometry,
  nonInteractive: true,
  children: items.map((item) => item.id()),
  focusGroup: { stop: ["left", "right"], entry: "lastFocused" },
});

panel.setFocusGroup({
  stop: ["up", "down", "left", "right"],
  wrap: ["next", "previous"],
});
```

A group's members come together in Tab order. `set_focus_group` (`setFocusGroup`) makes a
component a group after it's created, or stops it being one.

### Putting focus where it belongs

An app puts focus where it belongs as its screens change, so the keys never start from a
component that's gone: `focus()` gives a component focus, `clear_focus` (`clearFocus`) takes it
away, and `focused()` says which has it. `on_focus` and `on_blur` fire on a component as it gains
and loses focus, by the keys, a click or the app.

A split whose source has focus gives it to the target marked `default_focus`, or else to its
first. When a component loses focus because it can no longer keep it, the callback the app
registered with `on_focus_lost` (`onFocusLost`) is called with the component and the reason.
That's where an app gives focus to the component that replaced it:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let close = app.component(ComponentSpec::new(QuadState::default()));
app.on_focus_lost(move |app, _lost| {
    // The page that had focus has gone: focus its Close button.
    let _ = close.focus(app);
});
```

#### TypeScript

```ts
app.onFocusLost(() => {
  // The page that had focus has gone: focus its Close button.
  button.focus();
});
```

A component can't take focus while it's hidden, and a transition's targets are hidden until it
ends, so give one focus once the transition ends, which `on_transition_complete` reports: on a
split's source, or a merge's destination. The how-to
[Make an app work with the keyboard](../how-to/keyboard-navigation.md) puts this, and the rest of
this guide, together.

### Enter and Space

Enter and Space press and click the focused component, as the pointer does: its `on_press` and
`on_click` fire, it shows its pressed style while the key is held, and `on_release` fires as the
key is released. So a component that responds to clicks responds to the keys with no more code.
A component that shouldn't be clicked this way handles `Activate` itself, as the tile above does.

## Held keys

A held key repeats, first after 250 ms and then every 100 ms, the same on every platform; set
`key_repeat` in the [input settings](./configuration.md#input), or `set_key_repeat`
(`setKeyRepeat`), to change the timing or to use the system's repeats. A key held half a second
is long-pressed once, and its release says so, so a tap can be told from a hold; set
`long_press_ms`, or `set_long_press_ms` (`setLongPressMs`), to change the time.

Keys still held when the window or canvas loses focus are released. Caps Lock arrives as a press
and a release at each tap, on every platform.

## Examples

The [browse example](../../examples/browse) shows each case once: focus groups that keep, wrap
and let go of the arrows, with each kind of entry, a scrolling row, a tap and a hold, back, focus
put where it belongs from code, and hints that follow the input mode. The reference demo works
with the keys on every screen, and its Keyboard & Focus panel shows the keys at work on a virtual
keyboard.
