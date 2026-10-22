# A fast path for plain components in the shader

**Status:** in v0.2.0. The early exit, approach 1, saved only 8%, so plain components got a
pipeline of their own, approach 2, which halved the large scene's frame natively; see PLANNING's
step 6 and ARCHITECTURE's [plain pipeline](../../ARCHITECTURE.md#one-shader-and-a-plain-pipeline).
Found by the M14 benchmarks; see [BENCHMARKS.md](../../BENCHMARKS.md).

Proteus's GPU work, not its CPU or its WebAssembly boundary, is what limits large scenes, and
most of it is the fragment shader doing its full work on every pixel of every component, even a
plain shape with no image, border, shadow or glow.

## What was measured

- **Natively** (`benches/native`, Apple M4, Metal, 1280 × 800), the frame interval is about four
  times the CPU time from 5,000 components up, so the GPU sets the frame rate.
- **In Chrome 155** (`benches/browser`, Proteus on WebGPU, pixel ratio 2, 60 Hz), with
  components 20 to 200 pixels across, Proteus fell below 60 Hz at 5,000 components, while two
  renderers written by hand with WebGL2, one draw call per component and one instanced call,
  both held 60 Hz to 10,000. At 50,000, Proteus took 337 ms a frame and they took 50. Their
  shader only draws a rounded rectangle.
- **With components 4 to 8 pixels across** instead, where filling pixels barely matters, all
  three held 60 Hz up to 100,000 components: Proteus's cost per component isn't what limits it.
- **Safari 26.5** (WebGPU, 59 Hz) agreed: with large components, Proteus fell to 35 ms a frame
  at 10,000 while the hand-written renderers held, and 180 ms at 50,000 against 65 to 69.
- **Safari's tab crashed at 100,000 large components**, probably because of Proteus, the slowest
  renderer there (about 430 ms a frame in Chrome): WebKit resets a GPU process whose work runs
  too long. Check that first, and whether the fast path ends it.
- **Frames are uneven** once the GPU is the limit. With large components, Proteus's p99 is about
  twice its median from 5,000 up, in both browsers and from TypeScript and Rust alike: at
  10,000, 41.2 ms with a p99 of 72.4 in Chrome, and 39 with 77 in Safari. The hand-written
  renderers mostly stay within 15%. Check whether the fast path evens them out.

## The diagnosis

With `fs_main`, the shader's fragment stage, made to return a flat color
immediately, the native frame interval fell from 11.4 to 2.8 ms at 10,000 components, and from
33 to 15.7 ms at 25,000: the fragment shader is about three quarters of the GPU time at 10,000.
The rest, at higher counts, is vertex work, each instance carrying a 160-byte `QuadInstance`,
and blending where components overlap.

`fs_main` computes the rounded rectangle's distance field, always samples a texture (a component
without one samples a white block in the main atlas), checks for a border and a crossfade, and
premultiplies. Shadow and glow work is already skipped when they're off, and `vs_main` only grows
the quad for an active shadow.

## Approaches

1. **An early exit in `fs_main` for plain components,** from a flag in the instance: the
   distance field and the color, with no texture sample, border or crossfade. Start here.
2. **A separate pipeline for plain components.** Harder: drawing order must hold across
   pipelines, so it would draw runs of consecutive plain components, a draw call each.
3. **A smaller `QuadInstance`, or fewer vertex attributes,** for the vertex side.
4. **Skip pixels that a later opaque component covers,** with a depth buffer; see below.
   Independent of the others, and helps most where many opaque components overlap.

### Skipping covered pixels with a depth buffer

Every component's every pixel runs `fs_main`, even where a later component covers it completely;
with large components stacked many times, as in the large scene, most of that work is thrown
away. A GPU can reject a pixel before running the fragment shader if a depth test fails.

A sketch: a first pass draws each opaque component's interior, front to back, writing only
depth, with a trivial shader. Then the main pass draws everything in its usual order, testing
depth without writing it, so covered pixels are rejected before `fs_main` runs. Depth comes from
each instance's place in the drawing order, not from `z`, which the shader doesn't read.

Open questions:

- **What counts as opaque.** A component with an opaque color, full opacity and no crossfade,
  whose texture is opaque: the white block is, text never is, and an image would need a flag
  recorded when it's decoded. Only the interior inside the rounded corners and the antialiased
  edge can write depth.
- **`discard`.** `fs_main` discards pixels outside the shape and its shadow, which on some GPUs
  stops early depth testing. The main pass doesn't write depth, which should keep early rejection
  working, but check on Metal, Vulkan, DirectX 12 and WebGL2.
- **Costs.** A depth texture the size of the surface, the first pass's vertex work, and a second
  instance order. Bakes would need it too, or skip it.

## How to measure

`cargo run --release -p proteus-bench -- large 10000,25000` natively, and `make bench-browser`,
the large scene, in a browser. The flat-color `fs_main` above bounds what any change can gain.
Measure the depth-buffer approach on the large scene against the early exit, and on the small
scene to check it costs nothing where components don't overlap.

The shader draws everything, so the reference demo, its Effects and Text examples especially,
needs a visual check on both shells afterwards.
