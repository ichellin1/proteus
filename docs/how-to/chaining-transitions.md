# Chain transitions

To run transitions one after another, such as a card that slides across and then grows, start
each one when the one before it completes.

Keep the steps in a queue, and take the next from it each time the component's transition
completes:

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec3};
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
# use std::collections::VecDeque;
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
let start = app.get(card).unwrap().geometry;
let across = QuadState {
    position: Vec3::new(300.0, 0.0, 0.0),
    ..start.clone()
};
let grown = QuadState {
    size: Vec2::new(400.0, 300.0),
    ..across.clone()
};
let config = TransitionConfig {
    duration: 0.3,
    ..Default::default()
};

// The steps after the first.
let mut steps = VecDeque::from([grown]);
card.on_transition_complete(&mut app, move |app| {
    if let Some(next) = steps.pop_front() {
        let _ = card.animate_to(app, next, config);
    }
});

// The first step.
card.animate_to(&mut app, across, config)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const start = app.get(card)!.geometry;
const across = { ...start, position: { x: 300, y: 0, z: 0 } };
const grown = { ...across, size: { width: 400, height: 300 } };

// The steps after the first.
const steps = [grown];
card.onTransitionComplete(() => {
  const next = steps.shift();
  if (next) {
    card.animateTo(next, { duration: 0.3 });
  }
});

// The first step.
card.animateTo(across, { duration: 0.3 });
```

Each step starts on the frame after the one before it ends, so the chain runs without a visible
pause. Each step can have its own duration and easing.

## Why not use delays

Giving the second transition a `delay` as long as the first doesn't work on one component:
starting a transition on a component that is already moving replaces the transition it was
running. Delays suit transitions on *different* components; see
[Stagger a group](./staggering.md).

## Variations

- **Repeat forever:** put each step back at the end of the queue as you take it, so a component
  moves back and forth until you stop it.
- **Stop partway:** clear the queue. The running step finishes, and nothing follows it.
- **Chain across components:** register the callback on the component whose transition comes
  first, and start the next component's transition from it. For example, when a panel has
  finished opening from a button, its contents can move into place.
- **After a channel's transition,** the completion is reported on `to`, the component that was
  transitioned into.
