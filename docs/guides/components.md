# Components and geometry

Everything Proteus draws is a component: a rectangle with a position, a size and a color, which
can have rounded corners, carry text or an image, and have a border, a glow or a drop shadow.
Components can contain other components. A button, a card, a label and a list item are all
examples of components.

The examples on this page build one component, a card, and change it step by step.

## Creating a component

A component is created from a `ComponentSpec`, which describes it. `app.component` creates it and returns
a handle.

#### Rust

```rust
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{Border, ComponentSpec, Proteus, QuadState, Text};

let mut app = Proteus::new();
let card = app.component(
    ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, 100.0, 0.0),
        size: Vec2::new(320.0, 200.0),
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        corner_radius: 12.0,
        ..Default::default()
    })
    .text(Text::new("Hello", 20.0).with_color(Vec4::new(0.1, 0.1, 0.1, 1.0)))
    .border(Border::new(2.0, Vec4::new(0.45, 0.35, 0.8, 1.0))),
);
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const card = app.component({
  geometry: {
    position: { x: 0, y: 100, z: 0 },
    size: { width: 320, height: 200 },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: colorFrom("white"),
    cornerRadius: 12,
  },
  text: { content: "Hello", sizePx: 20, color: colorFrom("#1a1a1a") },
  border: { width: 2, color: colorFrom("#7359cc"), offset: -1 },
});
```

A **handle** is a small ID that refers to the component, which you use to change it later. In
Rust it implements the `Copy` trait, so you can keep it anywhere and capture it in callbacks.

A handle is safe to use after its component has been destroyed. A method that changes the
component then fails: in Rust it returns `Err(HandleError::EntityNotFound)`, and in TypeScript it
throws an `Error` with the message "proteus: the component this handle refers to no longer
exists". A method that only reads, such as `app.get`, returns nothing: `None` in Rust,
`undefined` in TypeScript.

## What a spec can include

Only the geometry is required. Everything else is optional:

| Rust method | TypeScript field | What it does |
|---|---|---|
| `ComponentSpec::new(geometry)` | `geometry` | Where the component is and how it's shaped; see [Geometry](#geometry). |
| `text` | `text` | A line of text drawn on the component. |
| `image` | `image` | A PNG or JPEG image, drawn as the component's fill. |
| `border` | `border` | A border inside the component's edge. |
| `glow` | `glow` | A soft halo around the component. |
| `drop_shadow` | `dropShadow` | A shadow behind the component. If it also has a glow, the shadow is drawn. |
| `child` | `children` | Components inside this one; see [Children](#children). |
| `visible` | `visible` | Whether it starts visible. A hidden component is neither drawn nor clickable. |
| `opacity` | `opacity` | How transparent it is, and everything in it, from `0` to `1`. |
| `hover`, `pressed`, `focused`, `disabled` | the same names | How it looks while the pointer is over it, while pressed, focused, or disabled. |
| `non_interactive` | `nonInteractive` | Takes the component out of input, for things that are never controls, such as labels. |
| `start_disabled` | `startDisabled` | Starts it disabled: drawn, blocking input, but responding to none. |
| `transition_interaction` | `transitionInteraction` | Whether it takes input while it transitions. |
| `bake` | `bake` | Draws it and its children into one image, once, for detailed content that never changes. |

Text, images, input and interaction styles each have a guide of their own.

## Geometry

A component's geometry, `QuadState` in Rust and `Geometry` in TypeScript, has seven fields:

| Field | Meaning |
|---|---|
| `position` | Where the component is, in pixels: see [Position](#position). `z` orders drawing: see [Draw order](#draw-order). |
| `size` | Width and height, in pixels. |
| `rotation` | Rotation, in radians. A positive value turns it counter-clockwise: see [Rotation](#rotation). |
| `scale` | A multiplier on the size: `1` is the component's natural size, `2` twice as large. |
| `anchor` | The point of the component that `position` places, as fractions of its size from its top-left corner. `(0.5, 0.5)`, the default, is its center. See [Anchor](#anchor). |
| `color` | The fill color, RGBA from `0` to `1`. With an image, it tints the image; white shows the image unchanged. |
| `corner_radius` | Corner radius, in pixels; `0` for square corners. |

### Position

**A top-level component** is placed relative to the center of the window or canvas: `(0, 0)` is
the center, whatever the window's size, x grows to the right and y grows upward. Positions are in
logical pixels, so a component is the same size on a high-density display. The card above, at
`(0, 100)`, sits 100 pixels above the center.

**A child** is placed relative to its parent: its position is an offset from the parent's
position, so `(0, 0)` puts the child at the parent's anchor, its center by default. The offset
turns and scales with the parent.

In TypeScript, `topLeftToWorld` converts from the top-left coordinates the web usually uses:

#### TypeScript

```ts
import { topLeftToWorld } from "proteus-sdk";

// 20 pixels from the canvas's left edge and 40 from its top.
const canvas = document.querySelector("canvas")!;
const position = topLeftToWorld(20, 40, canvas.clientWidth, canvas.clientHeight);
```

### Draw order

Where components overlap, the order they are drawn in decides which is on top:

- **Top-level components** are drawn in order of `z`, lowest first. Between equal `z`s, the one
  created first is drawn first, so the one created later is on top.
- **A component's children** are drawn right after it, so a child is always over its parent.
  Among themselves, siblings follow the same rule: by their own `z`, then by when they were
  created.
- **A child stays with its parent:** it is never drawn over a top-level component that is drawn
  after its parent, however large its `z`.

The pointer follows the same order, so a click goes to the component you see on top.

### Rotation

`rotation` is in radians, and a positive value turns the component counter-clockwise. That
follows from y growing upward, as in mathematics and other y-up systems. In systems where y
grows downward, such as CSS, a positive rotation turns clockwise instead. A component turns
around its anchor.

### Anchor

The anchor is the point of the component that matters for placing and transforming it:

- `position` places the anchor: with `(0, 0)`, the component's top-left corner is at its
  position;
- the component **rotates and scales around** its anchor;
- a **child's** `(0, 0)` is the parent's anchor, since a child is placed relative to the
  parent's position;
- **text** is placed by the anchor too: centered in the component by default, and at its
  top-left corner with `(0, 0)`.

A transition animates the anchor along with the rest of the geometry.

## Where a component rests

A component's **declared geometry** is where it rests. It starts as the geometry it was created
with. A transition into the component ends there, and its interaction styles, such as a hover
effect, apply on top of it.

Moving a component changes where it rests:

- `set_declared_geometry` moves it there at once;
- `animate_to` moves it there smoothly, with a transition.

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec3};
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState {
#     position: Vec3::new(0.0, 100.0, 0.0),
#     size: Vec2::new(320.0, 200.0),
#     ..Default::default()
# }));
let mut moved = app.get(card).unwrap().geometry;
moved.position.x = 200.0;

// Move there at once:
card.set_declared_geometry(&mut app, moved.clone())?;
// Or move there over 0.3 seconds:
card.animate_to(
    &mut app,
    moved,
    TransitionConfig {
        duration: 0.3,
        ..Default::default()
    },
)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const current = app.get(card)!.geometry;
const moved = { ...current, position: { ...current.position, x: 200 } };

// Move there at once:
card.setDeclaredGeometry(moved);
// Or move there over 0.3 seconds:
card.animateTo(moved, { duration: 0.3 });
```

## Children

A component can contain others, its children. A child moves, turns, scales, fades and hides with
its parent, so a label inside a card is usually a child of the card. Its position is relative to
the parent's: see [Position](#position).

Children can be given when a component is created, with `child`, or added later with
`add_child`. Here a caption is added to the card, 70 pixels below the card's center:

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec3, Vec4};
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
let caption = app.component(
    ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, -70.0, 0.0),
        size: Vec2::new(280.0, 24.0),
        color: Vec4::ZERO, // transparent: only the text shows
        ..Default::default()
    })
    .text(Text::new("A short caption", 16.0))
    .non_interactive(),
);
card.add_child(&mut app, caption)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const caption = app.component({
  geometry: {
    position: { x: 0, y: -70, z: 0 },
    size: { width: 280, height: 24 },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: colorFrom("transparent"),
    cornerRadius: 0,
  },
  text: { content: "A short caption", sizePx: 16 },
  nonInteractive: true,
});
card.addChild(caption);
```

`remove_child` takes a child out again. The child isn't destroyed: it becomes a top-level
component, and moves on screen, since its position is now relative to the window instead of the
card. To remove it altogether, destroy it instead.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let caption = app.component(ComponentSpec::new(QuadState::default()));
# let card = app.component(ComponentSpec::new(QuadState::default()).child(caption));
card.remove_child(&mut app, caption)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.removeChild(caption);
```

## Visibility and opacity

A **hidden** component isn't drawn and doesn't receive input, and neither are its children. A
component can start hidden, and be revealed by a transition into it.

**Opacity** fades a component and everything in it, from `0`, invisible, to `1`. It multiplies
down to the children: a child at `0.6` in a parent at `0.6` is drawn at `0.36`. It only affects
drawing, so a component at opacity `0` still receives clicks; hide it to take it out of input
as well.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
card.set_opacity(&mut app, 0.5)?;
card.set_visible(&mut app, false)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.setOpacity(0.5);
card.setVisible(false);
```

## Reading a component

`app.get` returns a snapshot of a component: its geometry as it is now, partway through a
transition if one is running, whether it is visible, the opacity it is drawn with, its children,
and its current transition and interaction state. It doesn't update afterwards.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
if let Some(data) = app.get(card) {
    println!("at {:?}, visible: {}", data.geometry.position, data.visible);
}
```

#### TypeScript

```ts
const data = app.get(card);
if (data) {
  console.log(data.geometry.position, data.visible);
}
```

## Destroying a component

`destroy` removes a component for good, with its children, their callbacks, and the transition
channels they own. Its handle then refers to nothing.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
card.destroy(&mut app)?;
assert!(app.get(card).is_none());
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.destroy();
console.log(app.get(card)); // undefined
```

A component you'll show again is better hidden than destroyed: a hidden component keeps
everything, and a transition can bring it back.
