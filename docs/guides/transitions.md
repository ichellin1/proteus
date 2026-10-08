# 1→1 transitions

A transition changes a component's geometry smoothly over time: its position, size, rotation,
scale, color and corner radius all move together, from where they are to where they're going.
There are two kinds of 1→1 transition: a component moving to new geometry, and one component
turning into another.

## Moving a component

`animate_to` transitions a component from its current geometry to new geometry:

#### Rust

```rust
# use proteus_sdk::glam::Vec3;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
let to = QuadState {
    position: Vec3::new(300.0, 0.0, 0.0),
    ..QuadState::default()
};
card.animate_to(
    &mut app,
    to,
    TransitionConfig {
        duration: 0.3,
        ..Default::default()
    },
)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const to = { ...app.get(tile)!.geometry, position: { x: 300, y: 0, z: 0 } };
tile.animateTo(to, { duration: 0.3 });
```

Calling it again during a transition starts a new one from wherever the component is, so it
suits a component that moves often, such as one following the pointer. The new geometry becomes
where the component rests; see
[Components and geometry](./components.md#where-a-component-rests).

## Turning one component into another

A **transition channel** turns one component into another. `set(to, from)` hides `from` at once,
shows `to`, and moves `to` from `from`'s geometry to its own declared geometry. On screen, `from`
becomes `to`: a button can grow into the panel it opens.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let button = app.component(ComponentSpec::new(QuadState::default()));
# let panel = app.component(ComponentSpec::new(QuadState::default()).visible(false));
let channel = app.transition_channel(None);
let config = TransitionConfig {
    duration: 0.4,
    ..Default::default()
};
button.on_click(&mut app, move |app| channel.set(app, panel, button, config, false));
panel.on_click(&mut app, move |app| channel.set(app, button, panel, config, false));
```

#### TypeScript

```ts
const opener = app.transitionChannel();
button.onClick(() => opener.set(panel, button, { duration: 0.4 }));
panel.onClick(() => opener.set(button, panel, { duration: 0.4 }));
```

Neither component refers to the other: the channel holds the relationship, and the code that
calls `set` decides where each transition goes. One channel can drive any number of transitions,
between any components; a channel per relationship, such as one per screen, keeps the code
easy to follow.

`to` usually starts hidden, with `visible(false)`, and appears as the transition begins.

A channel lasts until you destroy it. Pass an owner to `transition_channel` to destroy the
channel along with that component:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let screen = app.component(ComponentSpec::new(QuadState::default()));
let channel = app.transition_channel(Some(screen));
```

#### TypeScript

```ts
const owned = app.transitionChannel(panel);
```

## Timing and easing

Every transition takes a `TransitionConfig`:

- **`duration`**, in seconds. `0` is instant: the transition completes on the next frame. A
  negative duration is treated as `0`, with a warning.
- **`delay`**, in seconds, before the transition starts moving. Useful for staggering several
  transitions; see [Splits and merges](./splits-and-merges.md).
- **`easing`**, how the transition speeds up and slows down. The default starts slowly, speeds
  up, and slows to a stop.

The built-in easings are linear, ease in, ease out, ease in-out, and a stronger ease out. Any
other curve can be given as a cubic Bézier, with the same four numbers as CSS's
`cubic-bezier()`, so a curve can be copied from a stylesheet or a design tool. A curve can
overshoot: this one goes past its target and settles back:

#### Rust

```rust
# use proteus_sdk::{Easing, TransitionConfig};
let config = TransitionConfig {
    duration: 0.5,
    delay: 0.0,
    easing: Easing::CubicBezier {
        x1: 0.34,
        y1: 1.56,
        x2: 0.64,
        y2: 1.0,
    },
};
```

#### TypeScript

```ts
const config = { duration: 0.5, easing: { cubicBezier: [0.34, 1.56, 0.64, 1] } } as const;
```

In Rust, `Easing::Custom` takes any function from linear progress to eased progress.

## When a transition starts and ends

A transition starts on the next frame. Its completion is reported to the component's
`on_transition_complete` callback, which is how to start something once a transition has
finished, such as another transition:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let panel = app.component(ComponentSpec::new(QuadState::default()));
panel.on_transition_complete(&mut app, |_app| {
    // The panel has arrived.
});
```

#### TypeScript

```ts
panel.onTransitionComplete(() => {
  // The panel has arrived.
});
```

For a channel's transition, the completion is reported on `to`. Changes of interaction style,
such as a hover effect, don't count as transitions.

## When a transition can't run

A channel's request can't always run. It is then dropped, and the channel's `on_dropped`
callback is told why:

| Reason | When |
|---|---|
| Already transitioning | `to` is in the middle of a transition. |
| Entity not visible | `from` is hidden, so there is nothing to transition from. |
| Entity not found | `to` or `from` has been destroyed. |
| Channel not found | The channel has been destroyed. |

A component that is already transitioning ignores a new request unless the request is
**interruptible**: pass `true` as `set`'s last argument, and the new transition starts from
wherever `to` is, so it changes direction mid-flight.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState};
# let mut app = Proteus::new();
# let channel = app.transition_channel(None);
channel.on_dropped(&mut app, |_app, dropped| {
    println!("transition dropped: {:?}", dropped.reason);
});
```

#### TypeScript

```ts
channel.onDropped((dropped) => {
  console.log("transition dropped:", dropped.reason);
});
```
