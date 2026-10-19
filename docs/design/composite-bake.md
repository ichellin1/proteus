# A composite-bake transition strategy

**Status:** a candidate, to take or drop deliberately. Designed in V1's planning as
"Strategy 1 — Bake", and never built.

Before a split or merge starts, flatten the N side into a single texture, run one ordinary 1→1
transition between two quads, then discard the composite and show the real components at their
final positions.

## Why

A performance hypothesis, untested: on a constrained or low-end device, flattening a compound
subtree, such as a grid whose every tile carries images, text and buttons, into one quad and
transitioning that should beat moving every item separately.

## What exists that is close, but isn't this

- **`SplitStrategy::Row`, `Column` and `Grid`** flatten the source and its whole subtree into
  one bake, but then give a slice of it to each of N virtual components: still N moving pieces.
- **`ComponentSpec::bake`** flattens a component into one quad permanently, destroying its
  children, rather than for a transition's length.
- **`SplitStrategy::PerTarget`** runs N independent 1→1 transitions and bakes nothing.

## How to decide

Measure the per-item cost this is meant to avoid on a low-end device, with the M14 benchmarks
extended to a scene of compound components; see [BENCHMARKS.md](../../BENCHMARKS.md).
