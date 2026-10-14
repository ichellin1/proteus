# Benchmarks

How fast Proteus draws, natively and in a browser, how that was measured, and how to run the
measurements yourself.

## The claim

A UI drawn by hand with WebGL usually makes one draw call per component, so the cost of
getting each component to the GPU grows with the number of components. Proteus draws every
component in one instanced draw call, from one buffer it uploads each frame, so there is no
per-component call across the boundary between WebAssembly and the GPU.

**What the measurements show:**

- **Proteus is fast for animated interfaces.** With every component always moving, Proteus held
  the display's full rate of 60 fps up to 100,000 components, in Chrome and in Safari. CSS,
  animating the same components with transitions, fell to 10 fps at 10,000 elements in Chrome,
  and to 2.5 fps at 5,000 in Safari. Canvas 2D fell behind 60 fps when going from 25,000 to
  50,000 components. The WebGL2 renderers written by hand to draw rectangles kept pace, apart
  from the one that makes a draw call per component, which dropped below 60 fps at 100,000 in
  Safari.
- **The design claim holds.** With components only a few pixels across, so the GPU's work per
  pixel barely matters, Proteus draws at the display's full rate of 60 fps with up
  to 100,000 components, as fast as the screen can show them. With more, every renderer slows
  down, but Proteus slows down less than a renderer that makes one draw call per component. At
  half a million components it takes 43 to 45 ms a frame, against 67 ms for one draw call per
  component in Chrome and 144 ms in Safari: 1.6 and 3.2 times faster.
- **What limits Proteus is per-pixel work.** Its one shader handles borders, shadows, glows,
  images and video, and does more work on every pixel than a shader that only draws a rounded
  rectangle, even for a plain component. With large, still components overlapping many times, it
  falls below 60 fps between 5,000 and 10,000 components in both browsers, where the
  hand-written WebGL2 renderers keep 60 fps up through 10,000; at 25,000, Proteus takes 88 ms a
  frame in Chrome against hand-written WebGL2 renderers at 33 ms. Chrome's Canvas 2D does best
  here, keeping 60 fps with up to 25,000 elements on screen. Its frame times are uneven there,
  too: from 5,000 components up, its p99 is about twice its median, in both browsers and from
  TypeScript and Rust alike, while the hand-written renderers' mostly stay within 15% of theirs.
- **Starting many moves at once costs more from TypeScript.** Running transitions is cheap, but
  each `animateTo` call from TypeScript crosses into WebAssembly. When 100,000 components get
  new moves in one frame, that frame took 236 ms in Chrome and 364 in Safari. The same app
  written in Rust, whose calls don't cross, took 77 and 83. Otherwise the two draw the same.
- **A renderer batched by hand, for rectangles only, is faster at extreme counts.** Proteus
  rebuilds and uploads every component's 160-byte instance each frame, even when nothing
  changed; at a million small components, the hand-batched renderer was 3 times faster in
  Safari and 5 in Chrome.

Each limit is planned work for after V1: a fast path in the shader for plain components, a call
that starts many transitions at once, and rebuilding only what changed each frame.

## How it's measured

### Natively

`benches/native` runs the real engine and renderer, as the native host does, but draws into a
1280 × 800 texture rather than a window, so the display's refresh rate doesn't cap it. It runs
the same three scenes as the browser benchmark: N components with random positions and colors,
the same every run, and sizes that depend on the scene.

- **Small, still:** components 4 to 8 pixels across, with 2-pixel corners.
- **Large, still:** components 20 to 200 pixels across, with 8-pixel corners, overlapping many
  times at high counts.
- **Moving:** components 10 to 40 pixels across, with 4-pixel corners, every one always moving:
  each gets a new random place every half second, and moves there over half a second.

As in an app, the CPU starts the next frame while the GPU draws the last, with up to two frames
queued. After 120 warm-up frames, 300 frames are timed two ways: the **CPU** time, from the start
of the frame until its work is submitted, and the **frame interval**, from the start of one frame
to the next, which settles at the slower of the CPU and the GPU. When the GPU is the slower, the
CPU waits for it while submitting, and that wait counts as CPU time: in the large scene, the CPU
column overstates the CPU's own work, which the small scene shows.

Waiting for the GPU after every frame instead, as a first version did, left the CPU and GPU idle
between frames, and the Mac's power management then kept their clocks low: small scenes measured
slower than large ones, by a different amount each run.

A fourth mode, `phases`, needs no GPU. It times a still frame's work on the components:
`Proteus::tick`, `Proteus::refresh_cascades`, which works out each component's visibility and
opacity, and `collect_instances`, which turns the components into what the GPU draws.

### In a browser

`benches/browser` is a page that draws the same scenes six ways, on a 1280 × 800 area at the
display's pixel density:

- **Proteus (TypeScript),** through the TypeScript SDK.
- **Proteus (Rust),** the same app written in Rust and compiled to WebAssembly
  (`benches/browser/rust`). It draws with the same web host and renderer; its calls, such as
  creating a component or starting a transition, are ordinary Rust calls rather than crossings
  from JavaScript.
- **CSS:** a `div` per component, with rounded corners, moved with CSS transitions on
  `transform`. The way most web interfaces are built.
- **Canvas 2D:** the browser's 2D canvas, a `roundRect` and `fill` per component.
- **Naive WebGL2,** written by hand: one draw call per component, with its rectangle and color
  set as uniforms.
- **Batched WebGL2,** written by hand: every component in one instanced draw call, from a buffer
  it fills and uploads again every frame, as Proteus does.

The hand-written WebGL2 renderers' shader only draws a rounded rectangle, with the same edge
smoothing.

What's measured is the time between animation frames, the frame rate a user sees. A browser
caps it at the display's refresh rate, so a renderer at the cap is keeping up, and what to
compare is where each falls below it. Each count gets up to 2 seconds of warm-up, then up to 300
frames or 15 seconds. There are three scenes, each with the same components and moves for
every renderer:

- **Small, still:** components 4 to 8 pixels across. Even a million cover the canvas only a few
  times, so per-pixel work stays small, and what's left is each renderer's cost per component.
- **Large, still:** components 20 to 200 pixels across, overlapping many times at high counts,
  so filling pixels dominates. It stops at 50,000: past that, frames take long enough that
  Safari resets its GPU process and the tab crashes.
- **Moving:** components 10 to 40 pixels across, every one always moving: each gets a new random
  place every half second, and moves there over half a second, easing in and out. Proteus
  moves them with `animateTo`, CSS with transitions, and the others by drawing them at new
  positions each frame.

CSS appears only in the moving scene: a still page of `div`s costs the browser nothing per
frame once it's drawn, so a still scene wouldn't measure it. A renderer slower than 100 ms a
frame is skipped at its scene's larger counts, and the page saves its progress, so a tab that
crashes can resume, with the crash noted.

## Results

All on a MacBook Air (Apple M4, 10 CPU cores, 24 GB) running macOS 26.5.2, in October 2026. The
Air has no fan, so a long run can slow as it warms; each table is from one run.

### Native

Release build, Metal. Times in milliseconds.

**Small components, still:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.06 | 0.11 | 0.07 | 1.36 |
| 1,000 | 0.17 | 0.22 | 0.19 | 1.51 |
| 5,000 | 0.36 | 0.39 | 0.36 | 1.65 |
| 10,000 | 0.64 | 0.68 | 0.64 | 0.68 |
| 25,000 | 1.91 | 2.03 | 1.91 | 2.03 |
| 50,000 | 3.53 | 3.76 | 3.53 | 3.76 |
| 100,000 | 7.86 | 8.29 | 7.86 | 8.29 |

**Large components, still:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.04 | 0.08 | 0.05 | 1.33 |
| 1,000 | 0.10 | 0.16 | 1.35 | 1.44 |
| 5,000 | 1.68 | 2.14 | 5.39 | 6.67 |
| 10,000 | 3.56 | 3.99 | 10.79 | 11.86 |
| 25,000 | 7.41 | 8.34 | 27.38 | 28.92 |
| 50,000 | 12.83 | 13.81 | 55.88 | 58.73 |
| 100,000 | 22.56 | 23.99 | 116.06 | 120.68 |

**Moving components:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.10 | 0.22 | 0.12 | 1.45 |
| 1,000 | 0.20 | 0.70 | 0.21 | 1.52 |
| 5,000 | 0.38 | 2.89 | 0.39 | 2.89 |
| 10,000 | 0.72 | 5.63 | 0.72 | 5.63 |
| 25,000 | 2.22 | 15.28 | 2.22 | 15.29 |
| 50,000 | 4.97 | 29.79 | 4.97 | 29.79 |
| 100,000 | 9.81 | 60.36 | 9.81 | 60.36 |

With small components, the CPU sets the frame rate: 100,000 components take 7.9 ms a frame,
about 127 frames a second, or 79 ns of CPU per component. Moving them all costs little more:
9.8 ms. The moving scene's p99 is the frame every half second where every component gets a new
transition, about 0.5 µs each.

With large components, the GPU sets the frame rate, at about four times the small scene's CPU
time. Most of the GPU's time is the fragment shader: with it made to return a flat color, the
interval fell from 11.4 to 2.8 ms at 10,000 components, and from 33 to 15.7 ms at 25,000.

**A still frame's CPU work, by phase** (`phases`, medians in milliseconds):

| Components | `tick` | `refresh_cascades` | `collect_instances` |
|---:|---:|---:|---:|
| 10,000 | 0.06 | 0.05 | 0.33 |
| 100,000 | 0.55 | 0.55 | 4.53 |

These benchmarks found that working out visibility and opacity cost 4.5 ms at 100,000
components, run twice a frame. They now write only what changed, which took 6.4 ms off a large,
still frame's CPU time at 100,000, measured before the scenes were split, when that scene's CPU
time included waiting for the GPU. Most of what's left of the CPU's own work is the renderer
uploading every component's instance and recording the draw.

### Chrome

Chrome 155, with Proteus on WebGPU; 60 Hz display, pixel ratio 2. Frame interval medians in
milliseconds, with the p99 in brackets wherever it's more than 15% above the median; 16.7 is
the display's cap.

**Small components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 25,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 50,000 | 16.7 | 16.7 | 33.2 | 16.7 | 16.7 |
| 100,000 | 16.7 | 16.7 | 82.3 (99.0) | 16.7 | 16.7 |
| 250,000 | 21.4 | 21.7 | 199.9 | 33.4 (51.0) | 16.7 |
| 500,000 | 42.5 | 43.3 | skipped | 66.7 (84.3) | 16.7 |
| 1,000,000 | 85.0 | 88.5 | skipped | 150.0 | 16.7 (34.1) |

Canvas 2D was skipped once it was slower than 100 ms a frame.

**Large components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 1,000 | 16.7 | 16.6 | 16.7 | 16.7 | 16.7 |
| 5,000 | 17.8 (36.9) | 17.6 (37.7) | 16.7 | 16.7 | 16.7 |
| 10,000 | 41.2 (72.4) | 41.0 (73.6) | 16.7 | 16.7 | 16.7 |
| 25,000 | 88.6 (178.4) | 88.2 (181.6) | 16.7 | 33.3 | 33.3 |
| 50,000 | 183.6 (363.3) | 185.3 (366.8) | 33.3 | 50.0 (68.2) | 50.1 (68.0) |

**Moving components** (10 to 40 pixels, every one always moving). CSS was skipped once it was
slower than 100 ms a frame:

| Components | Proteus (TypeScript) | Proteus (Rust) | CSS | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 1,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 5,000 | 16.7 (30.8) | 16.7 (20.6) | 34.2 (133.4) | 16.7 | 16.7 | 16.7 |
| 10,000 | 16.7 (37.8) | 16.7 (21.6) | 100.0 (366.6) | 16.7 | 16.7 | 16.7 |
| 25,000 | 16.7 (64.9) | 16.7 (23.1) | skipped | 16.7 | 16.7 | 16.7 |
| 50,000 | 16.7 (117.3) | 16.7 (36.9) | skipped | 33.1 | 16.7 | 16.7 |
| 100,000 | 16.6 (236.3) | 16.6 (76.7) | skipped | 83.3 | 16.7 | 16.7 |

### Safari

Safari 26.5.2, with Proteus on WebGPU; 59 Hz display, pixel ratio 2. Frame interval medians in
milliseconds, with the p99 in brackets wherever it's more than 15% above the median; 17 is the
display's cap.

**Small components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 10,000 | 17 | 17 | 17 | 17 | 17 |
| 25,000 | 17 | 17 | 23 | 17 | 17 |
| 50,000 | 17 | 17 | 47 | 17 (20) | 17 |
| 100,000 | 17 | 17 | 100 | 29 (36) | 17 |
| 250,000 | 22 | 23 | 263 | 77 | 18 (25) |
| 500,000 | 45 | 46 | skipped | 144 | 17 (22) |
| 1,000,000 | 88 | 89 | skipped | skipped | 28 |

Canvas 2D and the naive WebGL2 renderer were skipped once they were slower than 100 ms a frame.

**Large components.** Proteus was skipped once it was slower than 100 ms a frame:

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 | 17 | 17 | 17 | 17 | 17 |
| 1,000 | 17 | 17 | 17 | 17 | 17 |
| 5,000 | 17 (40) | 17 (39) | 17 | 17 (22) | 17 |
| 10,000 | 39 (77) | 39 (75) | 17 | 17 | 17 |
| 25,000 | 125 (206) | 117 (407) | 41 (68) | 37 | 31 |
| 50,000 | skipped | skipped | 91 | 73 | 62 |

**Moving components.** CSS was skipped once it was slower than 100 ms a frame, and Canvas 2D at
100,000:

| Components | Proteus (TypeScript) | Proteus (Rust) | CSS | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 17 | 17 | 17 | 17 | 17 | 17 |
| 1,000 | 17 | 17 | 17 (38) | 17 | 17 | 17 |
| 5,000 | 17 (40) | 17 (20) | 404 (1,181) | 17 | 17 | 17 |
| 10,000 | 17 (50) | 17 (21) | skipped | 17 | 17 | 17 |
| 25,000 | 17 (95) | 17 (23) | skipped | 23 (28) | 17 (23) | 17 (27) |
| 50,000 | 17 (169) | 17 (34) | skipped | 49 (62) | 17 (30) | 17 (46) |
| 100,000 | 17 (364) | 17 (83) | skipped | 111 (134) | 32 (61) | 19 (55) |

## Running them

From the repository's root. The native benchmark needs only Rust; the browser benchmark needs
the tools for the TypeScript SDK, listed in [CONTRIBUTING.md](./CONTRIBUTING.md#dependencies).

**Native,** in a release build: a debug build is many times slower, and its numbers mean nothing.
It prints Markdown tables, headed with the GPU and platform. All three scenes take about five
minutes:

```bash
cargo run --release -p proteus-bench
```

It takes two arguments, after `--`, both optional:

```text
cargo run --release -p proteus-bench -- [what] [counts]
```

| Argument | Values | Default |
|---|---|---|
| `what` | `all`, every scene; `small`, `large` or `moving`, one scene; or `phases`, a still frame's CPU work by phase, with no GPU | `all` |
| `counts` | The component counts to measure, separated by commas, with no spaces: `1000,10000` | `100,1000,5000,10000,25000,50000,100000` |

To give `counts`, give `what` first. For example:

```bash
cargo run --release -p proteus-bench -- small 1000,10000
cargo run --release -p proteus-bench -- phases
```

To keep the results in a file:

```bash
cargo run --release -p proteus-bench > results.md
```

**In a browser:**

```bash
make bench-browser
```

builds the SDK if it has changed, and the Rust app, then builds the page and serves it. Open
the address it prints, usually <http://localhost:4173>, click **Run**, and keep the tab in front
until it's done: a hidden tab stops drawing. It prints the browser, GPU, pixel ratio, refresh
rate and Proteus's adapter, then the tables, ready to copy.

The page takes two settings, in its address, both optional:

| Setting | Values | Default |
|---|---|---|
| `scenes` | `small`, `large` and `moving`, separated by commas | all three |
| `counts` | The component counts to measure, separated by commas | `100,1000,5000,10000,25000,50000,100000` |

For example, `http://localhost:4173/?scenes=small&counts=100000,250000,500000,1000000`. The large
scene stops at 50,000 whatever `counts` asks for.

The page saves its progress for each address. If the tab crashes, reload the same address and
click **Resume**: the measurement that crashed is marked, and the run carries on. **Start over**
clears the saved results for that address.

When recording new results, note the machine, the browser and its version, and the display's
refresh rate, which the page reports.
