# Only redo per-frame work that changed

**Status:** a V2 candidate. Found by the M14 benchmarks; see [BENCHMARKS.md](../../BENCHMARKS.md).
It only matters at hundreds of thousands of components, so it comes after the
[shader fast path](./shader-fast-path.md).

## What was measured

With components 4 to 8 pixels across, so that filling pixels barely matters:

| Frame time (ms) | 250,000 | 500,000 | 1,000,000 |
|---|---:|---:|---:|
| Proteus, Chrome 155 | 21 | 42 | 87 |
| One draw call per component, Chrome | 33 | 67 | 150 |
| Proteus, Safari 26.5 | 22 | 44 | 87 |
| One draw call per component, Safari | 74 | 139 | 274 |
| Hand-batched, Safari | 17 | 21 | 38 |

A renderer written by hand with a single instanced call held 60 Hz throughout in Chrome. Both
browsers ran on an Apple M4 with WebGPU.

Proteus beats drawing each component separately, which is its design's claim, by about 1.7 times
in Chrome and 3 in Safari. But a hand-batched renderer is two to five times faster at a million,
because Proteus redoes all its per-component work every frame, even when nothing changed:

- `collect_instances` walks every component in drawing order and sorts them;
- it builds a 160-byte `QuadInstance` for each;
- and the renderer uploads all of them: 160 MB a frame at a million, against the hand-batched
  renderer's 28 bytes a component.

Natively, that's most of a still frame's CPU time
(`cargo run --release -p proteus-bench -- phases`).

## Approaches

- Rebuild and upload only the instances of components that changed.
- Keep the sorted drawing order until a `z` or the hierarchy changes.
- A smaller instance record, with rarely used fields such as shadow and glow in a separate
  buffer.

## How to measure

`make bench-browser`, opened with `?scenes=small&counts=100000,250000,500000,1000000`, and
`cargo run --release -p proteus-bench -- phases` natively.
