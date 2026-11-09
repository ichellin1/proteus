# Make an app work with the keyboard

An app accepts the pointer and the keyboard by default, and one built for the pointer already
mostly works with the keys: the arrows move focus to the
nearest component, Tab steps through in reading order, and Enter or Space clicks the focused
component, so its `on_click` runs as it does for the pointer. What's left is deciding where focus
goes, so the keys never start from somewhere that makes no sense. These steps do that, in the
order an app usually needs them. [Keyboard and focus](../guides/keyboard-and-focus.md) explains
each part in full.

## Start with focus somewhere

With nothing focused, the first arrow focuses the hovered component, or the first in reading
order. Give the screen's main control focus as the app starts instead, so the first key goes
where the user expects:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
let play = app.component(ComponentSpec::new(QuadState::default()));
play.focus(&mut app)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
button.focus();
```

In an app that also takes the pointer, focus shows only once a key is used, so a pointer user
never sees it. In a browser, the canvas takes keys only while it has focus; a full-screen app
sets `input.autofocus` so it has focus from the start. See [Hosts](../guides/hosts.md#the-keyboard-on-the-web).

## Group what belongs together

Without groups, the arrows go to whatever is nearest on screen. Group the components that belong
together, so focus moves among them first, and decide what each arrow does at the group's edge.
A row of tiles keeps Left and Right, lets Up and Down leave, and comes back to the tile the user
left; a page keeps every arrow, and wraps Tab round:

#### Rust

```rust
# use proteus_sdk::{Action, ComponentSpec, FocusEntry, FocusGroup, QuadState};
let row = FocusGroup::new()
    .stop(&[Action::Left, Action::Right])
    .entry(FocusEntry::LastFocused);
let page = FocusGroup::new()
    .stop(&[Action::Up, Action::Down, Action::Left, Action::Right])
    .wrap(&[Action::Next, Action::Previous]);
// The group is the parent of the components in it.
let spec = ComponentSpec::new(QuadState::default())
    .non_interactive()
    .focus_group(row);
# let _ = (spec, page);
```

#### TypeScript

```ts
list.setFocusGroup({ stop: ["left", "right"], entry: "lastFocused" });
panel.setFocusGroup({ stop: ["up", "down", "left", "right"], wrap: ["next", "previous"] });
```

## Put focus where it belongs as screens change

When a screen changes, the focused component is usually hidden by the transition, and focus is
lost. Clear focus as the transition starts, and give it to the new screen's main control as the
transition ends: a hidden component can't take focus, and the new screen's components show only
then. Coming back, give focus to the component the user left from, so they're where they were.

`on_transition_complete` says when a transition ends: on a split's source, on a merge's
destination, and on a component that moved. Here a tile merges into a page, whose Close button
takes focus as the page shows:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, MergeLayout, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let tile = app.component(ComponentSpec::new(QuadState::default()));
# let page = app.component(ComponentSpec::new(QuadState::default()).visible(false));
# let close = app.component(ComponentSpec::new(QuadState::default()));
# let _ = page.add_child(&mut app, close);
tile.on_click(&mut app, move |app| {
    app.clear_focus();
    let _ = page.merge_from(app, &[tile], TransitionConfig::default(), MergeLayout::Row);
});
page.on_transition_complete(&mut app, move |app| {
    let _ = close.focus(app);
});
```

#### TypeScript

```ts
tile.onClick(() => {
  app.clearFocus();
  panel.mergeFrom([tile], { duration: 0.4 }, { kind: "row" });
});
panel.onTransitionComplete(() => button.focus());
```

## Handle Back

Escape and Backspace are bound to `Back`, which does nothing until the app says what it means.
Handle it in the app's key callback, where it isn't tied to whichever component has focus, and
go back a screen:

#### Rust

```rust
# use proteus_sdk::{Action, KeyResult, KeyState, Proteus};
# let mut app = Proteus::new();
app.on_key(|_app, event| {
    if event.action == Some(Action::Back) && event.state == KeyState::Pressed {
        // Start the transition back to the previous screen here.
        return KeyResult::Handled;
    }
    KeyResult::Ignored
});
```

#### TypeScript

```ts
app.onKey((event) => {
  if (event.action === "back" && event.state === "pressed") {
    // Start the transition back to the previous screen here.
    return true;
  }
  return false;
});
```

## Wait for a screen change to end

A key can arrive while a screen is still changing: pressing Escape twice quickly sends the second
while the first's transition is still running. So can a click on a component that doesn't move,
such as a back button; a moving component ignores the pointer, but that one isn't moving. Acting
on either then starts another change from components that are still moving. Keep whether a
change is under way, and ignore input that changes screens until it ends:

#### Rust

```rust
# use std::cell::Cell;
# use std::rc::Rc;
# use proteus_sdk::{Action, ComponentSpec, KeyResult, KeyState, Proteus, QuadState};
# let mut app = Proteus::new();
# let page = app.component(ComponentSpec::new(QuadState::default()));
let changing = Rc::new(Cell::new(false));

let busy = changing.clone();
app.on_key(move |_app, event| {
    if event.action == Some(Action::Back) && event.state == KeyState::Pressed {
        if !busy.replace(true) {
            // Start the transition back here.
        }
        return KeyResult::Handled;
    }
    KeyResult::Ignored
});

// Every screen change ends with a transition that reports it.
let busy = changing.clone();
page.on_transition_complete(&mut app, move |_app| busy.set(false));
```

#### TypeScript

```ts
let changing = false;
app.onKey((event) => {
  if (event.action === "back" && event.state === "pressed") {
    if (!changing) {
      changing = true;
      // Start the transition back here.
    }
    return true;
  }
  return false;
});
panel.onTransitionComplete(() => {
  changing = false;
});
```

## Show one highlight at a time

Give each control a focused style, so the user can see where focus is, and a hover style for the
pointer. Only one shows at a time: the focused style while the keys are in use, and the hover
style while the pointer is. A glow or a border shows focus well from across a room; give the
control the effect at rest, such as a glow of radius 0, for the style to grow:

#### Rust

```rust
# use proteus_sdk::glam::Vec4;
# use proteus_sdk::{ComponentSpec, Glow, QuadState, StyleOverride};
let violet = Vec4::new(0.45, 0.35, 0.8, 1.0);
let spec = ComponentSpec::new(QuadState::default())
    .glow(Glow::new(0.0, violet))
    .focused(StyleOverride {
        glow_radius: Some(16.0),
        ..Default::default()
    })
    .hover(StyleOverride {
        scale: Some(1.03),
        ..Default::default()
    });
# let _ = spec;
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const control = app.component({
  geometry: app.get(button)!.geometry,
  glow: { radius: 0, color: colorFrom("#735acc"), intensity: 1 },
  focused: { glowRadius: 16 },
  hover: { scale: 1.03 },
});
```

Hints on screen, such as "Enter to choose", can follow `input_mode()` (`inputMode()`), so they
name the keys or the pointer, whichever is in use.

## Scroll to the focused component

A row longer than the screen still works with the keys: a component of a group focus is in can
take focus off screen. Slide the row as each of its components takes focus, so the component is
where the row started, on screen:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let row = app.component(ComponentSpec::new(QuadState::default()));
# let tile = app.component(ComponentSpec::new(QuadState::default()));
// Where the row starts.
let home = app.get(row).unwrap().geometry;
tile.on_focus(&mut app, move |app| {
    let Some(at) = app.get(tile) else {
        return;
    };
    let mut to = home.clone();
    // The tile's position is relative to the row, its parent.
    to.position.x = home.position.x - at.geometry.position.x;
    let _ = row.animate_to(app, to, TransitionConfig::default());
});
```

#### TypeScript

```ts
// Where the row starts.
const home = app.get(list)!.geometry;
tile.onFocus(() => {
  const at = app.get(tile);
  if (!at) return;
  // The tile's position is relative to the row, its parent.
  list.animateTo(
    { ...home, position: { ...home.position, x: home.position.x - at.geometry.position.x } },
    { duration: 0.4 },
  );
});
```

## Variations

- **A TV app, with the keys or a remote alone:** accept only the keyboard with
  `set_input_devices` (`setInputDevices`), and in a browser set `input.keep_tab` so Tab wraps
  round within the app rather than leaving the canvas. The steps above apply as they are; there's
  just no pointer, and no hover style to show. See
  [Pointer, keyboard or both](../guides/keyboard-and-focus.md#pointer-keyboard-or-both).
- **A pointer-only app, such as a kiosk:** accept only the pointer, and none of this is needed:
  keys are dropped, and nothing takes focus.
- **A tap or a hold:** to do one thing on a tap of Enter and another on a hold, handle `Activate`
  in the component's own key callback; see
  [Handling keys](../guides/keyboard-and-focus.md#handling-keys).

The [browse example](../../examples/browse) does all of this, in Rust and TypeScript.
