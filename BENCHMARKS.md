# Benchmarks

How fast Proteus draws, natively and in a browser, how that was measured, and how to run the
measurements yourself.

## The claim

A UI drawn by hand with WebGL usually makes one draw call per component, so the cost of
getting each component to the GPU grows with the number of components. Proteus draws every
component in one instanced draw call, from one buffer, of which it uploads only what changed
each frame, so there is no per-component call across the boundary between WebAssembly and the
GPU.

**What the measurements show:**

- **Proteus is fast for animated interfaces.** With every component always moving, Proteus held
  the display's full rate of 60 fps up to 100,000 components, in Chrome and in Safari. CSS,
  animating the same components with transitions, fell to 10 fps at 10,000 elements in Chrome,
  and to about 4 fps at 5,000 in Safari. Canvas 2D fell behind 60 fps from 25,000 components
  in Safari and from 100,000 in Chrome. The WebGL2 renderers written by hand to draw rectangles
  kept pace, apart from the one that makes a draw call per component, which dropped below 60
  fps at 100,000 in Safari.
- **The design claim holds.** With components only a few pixels across, so the GPU's work per
  pixel barely matters, Proteus draws at the display's full rate of 60 fps with up to half a
  million components, in both browsers. At a million, it takes 21 to 25 ms a frame, against 150
  ms for one draw call per component in Chrome, while in Safari that renderer was over 100 ms
  already at 500,000.
- **It keeps up with a renderer batched by hand.** The WebGL2 renderer that batches every
  rectangle into one draw call by hand, uploading all of them every frame, is the fastest
  possible design for rectangles alone. At a million small components, Proteus took 21 to 22 ms
  a frame in Safari against its 27, and 23 to 25 ms in Chrome against its 16.7. Before v0.2.0,
  when Proteus collected and uploaded every component every frame, it was 3 times slower in
  Safari and 5 in Chrome.
- **With large components overlapping, it's the fastest of them all.** With components 20 to
  200 pixels across, stacked many times over, Proteus holds the display's full rate with up to
  100,000 of them in Safari, and takes 19.9 ms a frame at 100,000 in Chrome, where the WebGL2
  renderers written by hand take 100 to 136 ms and Canvas 2D 83 to 144. Its frame times are
  even, too: the p99 stays within a few milliseconds of the median. Before v0.2.0 this was
  Proteus's weak spot: it fell below 60 fps between 5,000 and 10,000 components, took 88 ms a
  frame at 25,000 in Chrome, and crashed Safari's tab at 100,000. Plain components now take a
  smaller shader, and the GPU skips the pixels a later opaque component covers. The scene's
  components are all solid and opaque, which is where skipping covered pixels helps most;
  translucent, textured or bordered components stacked this deep still cost their pixels.
- **Starting many moves at once costs more from TypeScript.** Running transitions is cheap, but
  each `animateTo` call from TypeScript crosses into WebAssembly. When 100,000 components get
  new moves in one frame, that frame took 236 ms in Chrome and 342 in Safari. The same app
  written in Rust, whose calls don't cross, took 74 and 70. Otherwise the two draw the same.

What's left of these limits is planned: a call that starts many transitions at once, in
v0.6.0.

## Performance history

Each change that made Proteus faster, with what it was measured on, before and after. The
measurements are on the MacBook Air in [Results](#results) unless they say otherwise, and each
change is also in [CHANGELOG.md](./CHANGELOG.md), under the release it shipped in.

| Release | Change | Measured | Before | After |
|---|---|---|---:|---:|
| v0.1.0 | Visibility and opacity worked out only for what changed | Native, large still scene, CPU per frame at 100,000 | — | 6.4 ms less |
| v0.2.0 | Frames that didn't change aren't drawn | A two-year-old Chromebook's CPU, with the demo still (3% without it) | 8–11% | 4–5% |
| v0.2.0 | Only the components that changed are redone and uploaded | Native, small still scene, CPU per frame at 100,000 | 7.9 ms | 1.3 ms |
| | | Safari, small scene, frame at 1,000,000 | 88 ms | 24 ms |
| | | Chrome, small scene, frame at 1,000,000 | 85 ms | 27 ms |
| | | Both browsers, most small components at the display's full rate | 100,000 | 500,000 |
| v0.2.0 | Plain components drawn by a pipeline of their own | Native, large still scene, frame at 100,000 | 108.7 ms | 51.6 ms |
| | | Native, large still scene, frame at 10,000 | 10.3 ms | 5.3 ms |
| | | Chrome, large still scene, frame at 25,000 | 95 ms | 48 ms |
| | | Safari, large still scene, frame at 25,000 | 96 ms | 51 ms |
| | | Both browsers, most large components at the display's full rate | 5,000 | 10,000 (Safari), almost (Chrome, 19 ms) |
| v0.2.0 | Pixels a later opaque component covers aren't shaded | Native, large still scene, frame at 100,000 | 50.9 ms | 5.1 ms |
| | | Native, large still scene, frame at 25,000 | 12.3 ms | 1.9 ms |
| | | Chrome, large still scene, frame at 50,000 | 98–100 ms | 16.7 ms |
| | | Safari, large still scene, frame at 50,000 | 102–104 ms | 17 ms |
| | | Safari, large still scene, at 100,000 | The tab crashed | 17 ms |

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

A fourth scene, `video`, runs only when asked for: components the size of the window showing a
video, stacked, with a new 1280 × 800 frame uploaded every frame, as a playing video is. One is
a full-screen video; more stack the per-pixel work.

Another mode, `phases`, needs no GPU. It times a still frame's work on the components:
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
smoothing. Proteus is set to draw every frame, as the others do (`skipIdleFrames: false`):
otherwise it would draw a still scene once and then skip every frame after it, since nothing
changed.

What's measured is the time between animation frames, the frame rate a user sees. A browser
caps it at the display's refresh rate, so a renderer at the cap is keeping up, and what to
compare is where each falls below it. Each count gets up to 2 seconds of warm-up, then up to 300
frames or 15 seconds. There are three scenes, each with the same components and moves for
every renderer:

- **Small, still:** components 4 to 8 pixels across. Even a million cover the canvas only a few
  times, so per-pixel work stays small, and what's left is each renderer's cost per component.
- **Large, still:** components 20 to 200 pixels across, overlapping many times at high counts,
  so filling pixels dominates. It stops at 100,000: past that, a renderer's frames can take
  long enough that Safari resets its GPU process and the tab crashes, as Proteus's did at
  100,000 before v0.2.0.
- **Moving:** components 10 to 40 pixels across, every one always moving: each gets a new random
  place every half second, and moves there over half a second, easing in and out. Proteus
  moves them with `animateTo`, CSS with transitions, and the others by drawing them at new
  positions each frame.

CSS appears only in the moving scene: a still page of `div`s costs the browser nothing per
frame once it's drawn, so a still scene wouldn't measure it. A renderer slower than 100 ms a
frame is skipped at its scene's larger counts, and the page saves its progress, so a tab that
crashes can resume, with the crash noted.

## Results

Natively, in Chrome and in Safari, on a MacBook Air (Apple M4, 10 CPU cores, 24 GB) running
macOS 26.5.2, on 22 October 2026, during v0.2.0, with its instance cache, plain pipeline and
skipping of covered pixels. The Air has no fan, so a long run can slow as it warms; each table
is from one run, apart from the browsers' small scene above 100,000 components and large scene
at 100,000, each run on its own. Then on a low-end machine: in Chrome on a two-year-old
Chromebook, with WebGL2, before v0.2.0's changes.

### Native

Release build, Metal. Times in milliseconds.

**Small components, still:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.04 | 0.20 | 0.05 | 3.83 |
| 1,000 | 0.08 | 0.23 | 0.10 | 3.86 |
| 5,000 | 0.18 | 0.37 | 0.24 | 2.80 |
| 10,000 | 0.32 | 0.57 | 0.38 | 3.04 |
| 25,000 | 0.35 | 0.39 | 0.35 | 1.62 |
| 50,000 | 0.68 | 0.72 | 0.68 | 1.95 |
| 100,000 | 1.32 | 1.40 | 1.32 | 2.58 |

**Large components, still:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.02 | 0.07 | 0.05 | 1.30 |
| 1,000 | 0.06 | 0.09 | 0.07 | 1.35 |
| 5,000 | 0.17 | 0.27 | 0.19 | 1.48 |
| 10,000 | 0.33 | 0.61 | 0.49 | 1.85 |
| 25,000 | 0.77 | 0.93 | 2.01 | 3.28 |
| 50,000 | 1.49 | 1.78 | 2.75 | 4.04 |
| 100,000 | 2.46 | 2.60 | 5.10 | 7.39 |

**Moving components:**

| Components | CPU median | CPU p99 | Frame interval median | Frame interval p99 |
|---:|---:|---:|---:|---:|
| 100 | 0.07 | 0.21 | 0.08 | 1.40 |
| 1,000 | 0.15 | 0.45 | 0.16 | 1.46 |
| 5,000 | 0.47 | 2.80 | 0.47 | 2.80 |
| 10,000 | 0.89 | 5.81 | 0.89 | 5.81 |
| 25,000 | 2.23 | 14.91 | 2.23 | 14.91 |
| 50,000 | 4.73 | 30.36 | 4.73 | 30.36 |
| 100,000 | 9.75 | 60.68 | 9.75 | 60.68 |

With small components, the CPU sets the frame rate. A still frame redoes nothing it did before,
and only draws: 100,000 components take 1.3 ms a frame, about 13 ns of CPU each, against 7.9 ms
before v0.2.0, when every component's instance was collected and uploaded every frame. Moving
them all costs more, 9.8 ms, since every instance is redone and uploaded every frame. Its p99
is the frame every half second where every component gets a new transition, about 0.5 µs
each.

With large components, the GPU set the frame rate before v0.2.0: 112 ms a frame at 100,000,
most of it the fragment shader, run for every pixel of every component, however many times it
was covered. Plain components now take a smaller shader, and pixels a later opaque component
covers aren't shaded at all: 5.1 ms a frame, 22 times faster, of which 2.5 is the CPU's own
work.

**A still frame's CPU work, by phase** (`phases`, medians in milliseconds). The last two
columns are what the renderer does instead of `collect_instances` since v0.2.0: it keeps the
instances in an `InstanceCache` and redoes only the components that changed, here on a still
frame, and on a frame where 1% of the components moved:

| Components | `tick` | `refresh_cascades` | `collect_instances` | Cache, still | Cache, 1% moving |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 0.06 | 0.05 | 0.38 | 0.02 | 0.03 |
| 100,000 | 0.55 | 0.55 | 3.92 | 0.19 | 0.25 |

These benchmarks found that working out visibility and opacity cost 4.5 ms at 100,000
components, run twice a frame. They now write only what changed, which took 6.4 ms off a large,
still frame's CPU time at 100,000, measured before the scenes were split, when that scene's CPU
time included waiting for the GPU. They also found that every frame collected every component's
instance from scratch, 4.5 ms at 100,000 even with nothing moving; the cache takes 0.19 ms there,
20 times less, and uploads only what changed. With every component moving, it does them all in
one walk, about as fast as before: 9.2 ms a frame at 100,000 in the moving scene, against 9.4.

### Chrome

Chrome 156, with Proteus on WebGPU; 60 Hz display, pixel ratio 2. Frame interval medians in
milliseconds, with the p99 in brackets wherever it's more than 15% above the median; 16.7 is
the display's cap.

**Small components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 25,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 50,000 | 16.7 | 16.6 | 32.7 | 16.7 | 16.7 |
| 100,000 | 16.6 | 16.7 | 66.7 (83.4) | 16.7 | 16.7 |
| 250,000 | 16.7 | 16.7 | 183.3 (216.7) | 33.4 (50.9) | 16.7 |
| 500,000 | 16.7 | 16.7 | skipped | 67.6 (84.4) | 16.7 |
| 1,000,000 | 23.3 | 24.7 | skipped | 150.0 | 16.7 (34.2) |

Canvas 2D was skipped once it was slower than 100 ms a frame.

**Large components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 10,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 25,000 | 16.7 | 16.6 | 16.7 | 33.3 | 33.3 |
| 50,000 | 16.7 | 16.7 | 16.8 (33.4) | 50.0 (66.8) | 50.0 (67.5) |
| 100,000 | 19.9 | 19.9 | 83.2 | 100.1 (117.7) | 100.1 (117.0) |

**Moving components** (10 to 40 pixels, every one always moving). CSS was skipped once it was
slower than 100 ms a frame:

| Components | Proteus (TypeScript) | Proteus (Rust) | CSS | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 1,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 5,000 | 16.7 (24.1) | 16.7 (20.4) | 50.0 (134.3) | 16.7 | 16.7 | 16.7 |
| 10,000 | 16.7 (36.0) | 16.7 (21.1) | 100.0 (383.3) | 16.7 | 16.7 | 16.7 |
| 25,000 | 16.7 (63.2) | 16.7 (26.6) | skipped | 16.7 | 16.7 | 16.7 |
| 50,000 | 16.7 (117.5) | 16.7 (36.4) | skipped | 17.4 (33.4) | 16.7 | 16.7 |
| 100,000 | 17.0 (236.1) | 16.9 (73.5) | skipped | 66.7 (83.5) | 16.7 | 16.7 |

### Safari

Safari 26.5.2, with Proteus on WebGPU; 59 Hz display, pixel ratio 2. Frame interval medians in
milliseconds, with the p99 in brackets wherever it's more than 15% above the median; 17 is the
display's cap. Safari reports time in whole milliseconds.

**Small components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 10,000 | 17 | 17 | 17 | 17 | 17 |
| 25,000 | 17 | 17 | 23 | 17 | 17 |
| 50,000 | 17 | 17 | 49 | 17 (20) | 17 |
| 100,000 | 17 | 17 | 103 | 29 (35) | 17 |
| 250,000 | 17 | 17 | 269 | 74 | 17 (24) |
| 500,000 | 17 | 17 | skipped | 138 | 17 (24) |
| 1,000,000 | 21 | 22 | skipped | skipped | 27 |

Canvas 2D and the naive WebGL2 renderer were skipped once they were slower than 100 ms a frame.

**Large components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 | 17 | 17 | 17 | 17 | 17 |
| 1,000 | 17 | 17 | 17 | 17 | 17 |
| 5,000 | 17 | 17 | 17 | 21 (28) | 17 (22) |
| 10,000 | 17 | 17 | 17 | 17 (22) | 17 (21) |
| 25,000 | 17 | 17 | 37 | 35 | 30 |
| 50,000 | 17 | 17 (20) | 73 (87) | 69 | 58 |
| 100,000 | 17 (21) | 17 (21) | 144 | 136 | 115 |

**Moving components.** CSS was skipped once it was slower than 100 ms a frame:

| Components | Proteus (TypeScript) | Proteus (Rust) | CSS | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 17 | 17 | 17 | 17 | 17 | 17 |
| 1,000 | 17 | 17 | 17 (36) | 17 | 17 | 17 |
| 5,000 | 17 (39) | 17 (20) | 231 (410) | 17 | 17 | 17 |
| 10,000 | 17 (50) | 17 (21) | skipped | 17 | 17 | 17 |
| 25,000 | 17 (92) | 17 (23) | skipped | 24 (28) | 17 (21) | 16 (27) |
| 50,000 | 17 (168) | 17 (37) | skipped | 50 (65) | 17 (29) | 17 (44) |
| 100,000 | 16 (342) | 17 (70) | skipped | 109 (133) | 32 (59) | 20 (54) |

### A Chromebook

Measured before v0.2.0's changes. Chrome 154 on ChromeOS, on a two-year-old Chromebook with Intel's integrated graphics
(Alder Lake GT2). Its browser has no WebGPU adapter, so Proteus draws with WebGL2, as on most
low-end devices. 60 Hz display, pixel ratio 1.25. Frame interval medians in milliseconds, with
the p99 in brackets wherever it's more than 15% above the median; 16.7 is the display's cap.

**Small components:**

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 to 5,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 10,000 | 16.7 | 16.7 | 16.7 (33.4) | 16.7 | 16.7 |
| 25,000 | 16.7 | 16.7 | 49.9 (116.6) | 16.7 | 16.7 |
| 50,000 | 16.7 | 16.7 | 116.6 (166.7) | 16.7 | 16.7 |
| 100,000 | 16.6 (22.2) | 17.0 (20.3) | skipped | 33.3 (50.0) | 16.7 |

Canvas 2D was skipped once it was slower than 100 ms a frame.

**Large components.** Proteus was skipped once it was slower than 100 ms a frame:

| Components | Proteus (TypeScript) | Proteus (Rust) | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|
| 100 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 1,000 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 5,000 | 33.2 | 32.9 (40.4) | 16.7 | 16.7 | 16.7 |
| 10,000 | 50.3 (67.9) | 50.2 (68.5) | 33.3 (83.3) | 16.7 (33.4) | 16.7 (33.4) |
| 25,000 | 134.1 | 133.5 | 83.3 (200.0) | 50.0 (83.4) | 50.0 (66.7) |
| 50,000 | skipped | skipped | 200.0 (250.1) | 116.6 (150.0) | 100.0 (116.8) |

**Moving components.** CSS was skipped once it was slower than 100 ms a frame, and Canvas 2D at
100,000:

| Components | Proteus (TypeScript) | Proteus (Rust) | CSS | Canvas 2D | Naive WebGL2 | Batched WebGL2 |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 | 16.7 |
| 1,000 | 16.7 | 16.7 | 16.7 (33.3) | 16.7 | 16.7 | 16.7 |
| 5,000 | 16.7 (20.8) | 16.7 (20.4) | 83.4 (250.0) | 16.7 | 16.7 | 16.7 |
| 10,000 | 16.7 (29.2) | 16.7 (22.4) | 283.4 (533.5) | 33.3 (50.0) | 16.7 | 16.7 |
| 25,000 | 16.7 (70.5) | 16.7 (23.8) | skipped | 66.6 (150.0) | 16.7 | 16.7 |
| 50,000 | 16.8 (163.3) | 17.0 (61.0) | skipped | 150.0 (183.4) | 16.7 (33.4) | 16.7 (33.4) |
| 100,000 | 23.1 (385.8) | 32.0 (135.2) | skipped | skipped | 49.9 | 33.3 (50.1) |

What holds on the Mac holds here. Proteus keeps the display's full rate with 100,000 small
components, and with 50,000 always moving, where CSS fell to 12 fps at 5,000. Per-pixel work
limits it sooner: with large components it falls below 60 fps at 5,000, and at 25,000 takes
134 ms a frame, against 50 for the hand-written WebGL2 renderers.

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
| `what` | `all`, every scene; `small`, `large` or `moving`, one scene; `video`, full-screen video, at 1, 10 and 50 unless `counts` says; or `phases`, a still frame's CPU work by phase, with no GPU | `all` |
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

**In a browser:** the page is published at <https://ichellin1.github.io/proteus/bench/>, so it
runs on any machine with nothing to build. Each pull request's staging preview has its own, at
`bench/` under the preview's address. To build and serve it yourself instead:

```bash
make bench-browser
```

builds the SDK if it has changed, and the Rust app, then builds the page and serves it, usually
at <http://localhost:4173>.

Click **Run**, and keep the tab in front until it's done: a hidden tab stops drawing. It prints
the browser, GPU, pixel ratio, refresh rate and Proteus's adapter, then the tables, ready to
copy. To send results from your machine, send everything it printed, and say what the machine
is.

The page takes two settings, in its address, both optional:

| Setting | Values | Default |
|---|---|---|
| `scenes` | `small`, `large` and `moving`, separated by commas | all three |
| `counts` | The component counts to measure, separated by commas | `100,1000,5000,10000,25000,50000,100000` |

For example, `http://localhost:4173/?scenes=small&counts=100000,250000,500000,1000000`. The large
scene stops at 100,000 whatever `counts` asks for.

The page saves its progress for each address. If the tab crashes, reload the same address and
click **Resume**: the measurement that crashed is marked, and the run carries on. **Start over**
clears the saved results for that address.

When recording new results, note the machine, the browser and its version, and the display's
refresh rate, which the page reports.
