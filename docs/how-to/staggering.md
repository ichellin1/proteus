# Stagger a group

A stagger starts each component's transition a little after the one before it, so a group
arrives as a wave instead of all at once. Give each transition a `delay` that grows with its
place in the group.

Here six tiles rise into place from below, 0.06 seconds apart:

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec3};
# use proteus_sdk::{ComponentSpec, Easing, Handle, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
// Where each tile ends up: six in a row.
let places: Vec<QuadState> = (0..6)
    .map(|i| QuadState {
        position: Vec3::new(-250.0 + i as f32 * 100.0, 0.0, 0.0),
        size: Vec2::new(80.0, 80.0),
        ..Default::default()
    })
    .collect();

// Each tile starts 600 pixels below its place.
let tiles: Vec<Handle> = places
    .iter()
    .map(|place| {
        let mut below = place.clone();
        below.position.y -= 600.0;
        app.component(ComponentSpec::new(below))
    })
    .collect();

for (i, (tile, place)) in tiles.iter().zip(places).enumerate() {
    tile.animate_to(
        &mut app,
        place,
        TransitionConfig {
            duration: 0.4,
            delay: i as f32 * 0.06,
            easing: Easing::EaseOutCubic,
        },
    )?;
}
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const base = app.get(card)!.geometry;
const tiles = [0, 1, 2, 3, 4, 5].map((i) => {
  // Where the tile ends up: six in a row.
  const place = {
    ...base,
    position: { x: -250 + i * 100, y: 0, z: 0 },
    size: { width: 80, height: 80 },
  };
  // It starts 600 pixels below its place.
  const below = { ...place, position: { ...place.position, y: -600 } };
  return { handle: app.component({ geometry: below }), place };
});

tiles.forEach(({ handle, place }, i) => {
  handle.animateTo(place, { duration: 0.4, delay: i * 0.06, easing: "easeOutCubic" });
});
```

During its delay, a tile stays where it starts, so start the tiles somewhere they aren't seen
yet, such as below the window's edge, or hidden behind another component.

## Choosing the delay

- **Keep the step small,** between 0.03 and 0.1 seconds. The whole group takes the duration plus
  the step times one less than the number of components, so a long step with many components
  keeps the last one waiting.
- **For a long list,** stop growing the delay after the first ten or so, or only stagger the
  ones on screen.
- **To leave in reverse order,** count down: the delay for component `i` of `n` is
  `(n - 1 - i) * step`.
- **For a ripple** from a point, such as the tile that was clicked, base each delay on the
  component's distance from that point instead of its place in the list.

## Knowing when the group has arrived

The component with the longest delay finishes last. Register an `on_transition_complete`
callback on it to start whatever comes next; see [Chain transitions](./chaining-transitions.md).

## Splits and merges

The pieces of a split or a merge can be staggered the same way, with a function that gives each
piece its own timing; see [Timing each piece](../guides/splits-and-merges.md#timing-each-piece).
