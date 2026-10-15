# Start many transitions in one call, from TypeScript

**Status:** a V2 candidate. Found by the M14 browser benchmark; see
[BENCHMARKS.md](../../BENCHMARKS.md).

## What was measured

The benchmark's moving scene: N components 10 to 40 pixels across, each given a new target with
`animateTo` every half second. In Chrome 155 on an Apple M4 (WebGPU, 60 Hz), Proteus held a
16.7 ms median frame up to 100,000 moving components, so running the transitions is cheap. But
its 99th percentile was 38 ms at 10,000, 65 at 25,000, 118 at 50,000 and 235 at 100,000.

That's the frame every half second where every component gets its new `animateTo`: one call from
TypeScript into WebAssembly per component, each passing a geometry object that
`serde-wasm-bindgen` converts, about 2.2 µs a call.

The same app written in Rust (`benches/browser/rust`), making the same `animate_to` calls inside
WebAssembly, had a 99th percentile of 21.6 ms at 10,000, 37 at 50,000 and 77 at 100,000: about
0.6 µs a call, the cost of starting a transition itself. So about 1.6 µs of TypeScript's 2.2,
three quarters, is the crossing and the conversion.

Safari 26.5 agreed, with a costlier crossing: TypeScript's 99th percentile was 364 ms at 100,000
against Rust's 83, about 0.7 µs a call in Rust and 2.8 more for the crossing.

For comparison, CSS, animating the same moves with transitions, fell to 20 frames a second at
5,000 components and 10 at 10,000.

## Approach

A call that starts many transitions at once, such as
`app.animateMany(handles, targets, config)`, with the targets in a `Float32Array`, so the cost
is one crossing and a copy, close to Rust's.

## How to measure

`make bench-browser`, opened with `?scenes=moving`, watching the p99 column.
