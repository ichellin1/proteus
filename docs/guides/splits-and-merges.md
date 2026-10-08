# Splits and merges

A **split** turns one component into several, a 1→N transition: a button can break apart into
the items of the list it opens. A **merge** is the reverse, N→1: the items come back together
into the button. They are how a screen can transform into another without anything being
swapped in.

## Splitting

`split_to` splits a component into `targets`. The source is hidden as soon as the split starts.
It is cut into pieces, one per target, which move from their place in the source to the targets'
places, and fade from the source's appearance to the targets' on the way. When every piece has
arrived, the targets appear in their place.

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec3};
# use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState, SplitStrategy, TransitionConfig};
# let mut app = Proteus::new();
# let button = app.component(ComponentSpec::new(QuadState::default()));
// Three list items, hidden until the split reveals them.
let items: Vec<Handle> = (0..3)
    .map(|i| {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 100.0 - i as f32 * 100.0, 0.0),
                size: Vec2::new(300.0, 80.0),
                ..Default::default()
            })
            .visible(false),
        )
    })
    .collect();

button.split_to(
    &mut app,
    &items,
    TransitionConfig {
        duration: 0.5,
        ..Default::default()
    },
    SplitStrategy::Column,
)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const items = [0, 1, 2].map((i) =>
  app.component({
    geometry: { ...app.get(button)!.geometry, position: { x: 0, y: 100 - i * 100, z: 0 } },
    visible: false,
  }),
);
button.splitTo(items, { duration: 0.5 }, { kind: "column" });
```

Each target ends at its own declared geometry.

The [gallery example](../../examples/gallery) splits a large photo into a new grid of photos
this way.

## How the source is cut

The strategy decides how the source is cut into pieces, and which piece goes where:

- **Row:** strips side by side, left to right. Strip `i` goes to target `i`.
- **Column:** strips stacked top to bottom. Strip `i` goes to target `i`.
- **Grid:** a grid of `cols` by `rows`, filled row by row from the top-left. Cell `i` goes to
  target `i`. The grid must have a cell for every target; a grid with fewer is an error, and
  nothing starts.
- **Per target** (experimental): no pieces. Each target itself moves from the source's position
  to its own, showing its own content throughout. Use it when the targets shouldn't look like
  parts of the source.

Choose the arrangement that matches the targets: a column for a vertical list, a grid for a grid
of tiles. The pieces then travel the shortest way, and the split reads as the source coming
apart into its parts.

For Row, Column and Grid, the source and its children are drawn into one image when the split
starts, so the pieces show what the source looked like, text and all.

## Merging

`merge_from` merges `sources` into a component. The sources are hidden as soon as the merge
starts, and each moves toward a part of the destination, chosen by the layout: Row, Column or
Grid, as for a split. The destination appears when every source has arrived.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Handle, MergeLayout, Proteus, QuadState, TransitionConfig};
# let mut app = Proteus::new();
# let button = app.component(ComponentSpec::new(QuadState::default()).visible(false));
# let items: Vec<Handle> = (0..3)
#     .map(|_| app.component(ComponentSpec::new(QuadState::default())))
#     .collect();
button.merge_from(
    &mut app,
    &items,
    TransitionConfig {
        duration: 0.5,
        ..Default::default()
    },
    MergeLayout::Column,
)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
button.mergeFrom(items, { duration: 0.5 }, { kind: "column" });
```

A split followed by a merge is a round trip: a button that opens into a list, and closes back
into the button. The [menu example](../../examples/menu) is one.

## Timing each piece

Pieces can move with different timing, such as a stagger, where each starts a little after the
one before. Pass a function that is given each piece's index and the total, and returns that
piece's `TransitionConfig`:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Easing, Handle, Proteus, QuadState, SplitStrategy, TransitionConfig};
# let mut app = Proteus::new();
# let button = app.component(ComponentSpec::new(QuadState::default()));
# let items: Vec<Handle> = (0..3)
#     .map(|_| app.component(ComponentSpec::new(QuadState::default())))
#     .collect();
button.split_to_with_behavior(
    &mut app,
    &items,
    TransitionConfig::default(),
    SplitStrategy::Column,
    |i, _total| TransitionConfig {
        duration: 0.4,
        delay: i as f32 * 0.08,
        easing: Easing::EaseOutCubic,
    },
)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
button.splitTo(items, { duration: 0.4 }, { kind: "column" }, (i) => ({
  duration: 0.4,
  delay: i * 0.08,
  easing: "easeOutCubic",
}));
```

`merge_from_with_behavior` does the same for a merge. In TypeScript, `splitTo` and `mergeFrom`
take the function as an optional last argument.

## Ending somewhere other than the declared geometry

A split's targets normally end at their declared geometry. `split_to_with_states` gives each
target's end geometry explicitly instead. Use it when the source is also one of the targets:
setting its declared geometry first would move it before the split begins.

## When a split or merge completes

Completion is reported once, to `on_transition_complete`:

- for a Row, Column or Grid split, on the source, once every piece has arrived;
- for a split per target, on each target, separately;
- for a merge, on the destination, once every source has arrived.

While a split or merge runs, its targets or destination are hidden, and appear at the end.
