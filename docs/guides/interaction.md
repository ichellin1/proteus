# Interaction

Components respond to the pointer: a mouse, or on the web a touch or a pen. A component can call
your code when something happens to it, and can change how it looks while it is hovered,
pressed, focused or disabled. Keyboard input isn't supported yet.

## Events

Each `on_*` method registers a callback that is called every time its event happens:

| Rust | TypeScript | When |
|---|---|---|
| `on_click` | `onClick` | The pointer is pressed on the component. |
| `on_press` | `onPress` | The same moment as a click; use it with `on_release` to follow a press. |
| `on_release` | `onRelease` | The pointer is released after a press on the component, even if it has moved off. |
| `on_drag` | `onDrag` | Every frame while the component is pressed, with how far the pointer moved since the last frame. |
| `on_hover_enter` | `onHoverEnter` | The pointer moves onto the component. |
| `on_hover_exit` | `onHoverExit` | The pointer moves off it. |
| `on_focus` | `onFocus` | The component gains focus: it was clicked. |
| `on_blur` | `onBlur` | It loses focus: another component was clicked. Clicking empty space keeps the focus where it is. |
| `on_transition_complete` | `onTransitionComplete` | A transition on the component finishes; see [1→1 transitions](./transitions.md). |

Here the card from [Components and geometry](./components.md) follows the pointer while it's
dragged:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
card.on_drag(&mut app, move |app, delta| {
    let mut moved = app.get(card).unwrap().geometry;
    moved.position.x += delta.x;
    moved.position.y += delta.y;
    let _ = card.set_declared_geometry(app, moved);
});
```

#### TypeScript

```ts
card.onDrag((delta) => {
  const current = app.get(card)!.geometry;
  card.setDeclaredGeometry({
    ...current,
    position: { ...current.position, x: current.position.x + delta.x, y: current.position.y + delta.y },
  });
});
```

`delta` is in the same units as positions, so its y is positive when the pointer moves up.

A component can have any number of callbacks for an event; they are called in the order they
were registered. A callback can change anything, including destroying its own component.

### When callbacks run

The pointer is read once a frame. In Rust, callbacks for the frame's events run during the
frame, before the app's `update` and before anything is drawn, so a change a callback makes is
drawn in that frame. A transition it starts begins on the next frame.

In TypeScript, callbacks run just after the frame is drawn, so their changes are drawn in the
next frame. That's soon enough to look immediate. An exception thrown in a callback is logged to
the browser console with its stack, and the other callbacks still run.

## Which component gets the pointer

The pointer goes to the component drawn on top under it; see
[Draw order](./components.md#draw-order). Three things change which components can get it:

- **Hidden** components don't get the pointer, and neither do their children.
- **Non-interactive** components aren't there for input at all: the pointer passes through to
  whatever is behind them. Use it for things that are never controls, such as labels and
  backgrounds. A component is interactive by default; `non_interactive()` in the spec, or
  `set_interactive(false)`, takes it out.
- **Disabled** components are there but inert: they block the pointer from reaching what is
  behind them, call none of their callbacks, and show their disabled style, as a disabled
  control does on the web. Use it for a control that isn't available yet, such as a submit
  button. `start_disabled()` in the spec, or `set_disabled(true)`, disables it.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let button = app.component(ComponentSpec::new(QuadState::default()).start_disabled());
// Enable the button once the form is filled in.
button.set_disabled(&mut app, false)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
// Enable the button once the form is filled in.
button.setDisabled(false);
```

### During a transition

A component that is transitioning ignores the pointer by default, so a click doesn't land on
something that is still moving. To let it take pointer input while it moves, give it a
transition interaction setting with `allow_pointer`:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionInteractionConfig};
# let mut app = Proteus::new();
let card = app.component(
    ComponentSpec::new(QuadState::default()).transition_interaction(TransitionInteractionConfig {
        allow_pointer: true,
        ..Default::default()
    }),
);
```

#### TypeScript

```ts
card.setTransitionInteraction({ allowPointer: true });
```

## Interaction styles

A component can change how it looks while the pointer is over it, while it's pressed, while it
has focus, and while it's disabled. Each style lists only the fields that change; the rest come
from the component's geometry:

#### Rust

```rust
# use proteus_sdk::glam::Vec4;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, StyleOverride};
# let mut app = Proteus::new();
let button = app.component(
    ComponentSpec::new(QuadState::default())
        .hover(StyleOverride {
            scale: Some(1.05),
            ..Default::default()
        })
        .pressed(StyleOverride {
            scale: Some(0.95),
            ..Default::default()
        })
        .disabled(StyleOverride {
            color: Some(Vec4::new(0.6, 0.6, 0.6, 1.0)),
            ..Default::default()
        }),
);
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const styled = app.component({
  geometry: app.get(button)!.geometry,
  hover: { scale: 1.05 },
  pressed: { scale: 0.95 },
  disabled: { color: colorFrom("#999999") },
});
```

When more than one applies, the first in this list wins: disabled, pressed, focused, hover.

A change of style animates over 0.15 seconds. It isn't a transition: the component keeps getting
the pointer during it, and it doesn't call `on_transition_complete`. A component that is
transitioning keeps its style until the transition ends, then takes the style that applies.

A style applies on top of where the component rests, so after the component moves, its styles
move with it; see [Where a component rests](./components.md#where-a-component-rests).

`app.get` reports the style that applies now, as the snapshot's interaction state: default,
hover, pressed, focused or disabled.
