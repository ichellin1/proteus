# Proteus — Planning

The decisions and definitions of done for the work underway. [ROADMAP.md](./ROADMAP.md) gives
the order of the releases and the candidates for what comes next.

## How this document works

- **It records decisions and definitions of done.** A decision says what was decided and when,
  in a paragraph at most. A definition of done is a checklist, ticked as the work lands.
- **Reasoning longer than a paragraph goes in a design note,** in
  [docs/design/](./docs/design/README.md), linked from here: what was found or measured, the
  approaches weighed, and how to tell whether a change worked.
- **The design v0.1.0 settled is described in [ARCHITECTURE.md](./ARCHITECTURE.md),** with its
  key decisions. Changing one of them is a decision, recorded here.
- **The planning for v0.1.0, milestones M0 to M14, is archived** in
  [docs/archive/](./docs/archive/), as history. Nothing here depends on it.

## Release planning

Decides the releases: which of ROADMAP's candidates each takes, and in what order. Until it's
done, the candidates are unordered.

- **Decided (2026-10-15): releases, not milestones.** Now that Proteus is released, work is
  planned by version, each release with a theme and features of its own. Milestone numbers end
  at M14. "V2" in older documents means everything after v0.1.0, not a release.
- **Decided (2026-10-15): a release's scope is fixed.** It ships when its features are done,
  not on a date. Small fixes ride along in the next feature release, or, if found after a
  release ships, go out in a patch release.
- **Decided (2026-10-15): candidates are weighed in this order:** fixing known limits and known
  issues; what outside developers need to build real apps; effects, such as blur; then other
  platforms.
- **Decided (2026-10-15): the origin stays at the viewport's center, with y up.** Instead of
  moving it, Rust gets equivalents of TypeScript's `topLeftToWorld` and `worldToTopLeft`, and
  the guides make the origin and the conversion clear in both languages. Nothing breaks.
- **Decided (2026-10-15): dogfooding is off the roadmap.** A personal website built with Proteus
  happens outside it, and what it finds comes back to ROADMAP as candidates.
- **Decided (2026-10-19): work independent of a release is tracked in ROADMAP too,** in a
  section of its own. Rewriting the history to shrink the repository is the first such task,
  done early.
- **Decided (2026-10-19): a draft order of releases,** v0.2.0 to v0.8.0, is in ROADMAP. Each
  release is planned in detail when it comes up, and its scope can change then. Only the next
  release has a section here.
- **Decided (2026-10-20): v0.2.0 skips drawing frames in which nothing changed.** Letting the
  frame loop sleep as well is risky for little gain, so it stays a candidate, to try as a proof
  of concept after the planned releases.
- [x] ROADMAP and PLANNING plan by release rather than milestone.
- [x] Every candidate in ROADMAP is placed in a release, kept as a candidate for later, or
  dropped with a reason.
- [x] The releases are in ROADMAP in order.
- [ ] The two high-priority candidates, the shader fast path and rounded corners on baked
  slices, are decided first.

## v0.2.0 — Rendering limits

Sharper, faster and steadier rendering: the known limits in how Proteus draws. The scope is
from ROADMAP's draft order; the steps are in the order they're built.

- **Decided (2026-10-20): the order.** Skipping idle frames comes early, since it answers the
  reports of fans turning on with Proteus on low-end devices. The instance's 16 vertex attribute
  slots are all in use, and per-frame work, the fast path and the corners all want to change
  it, so the corners approach is decided before per-frame work changes the instance. Bakes move
  to the display's resolution before the corners are fixed, so the fix is tuned against the
  final bake sizes. The high-priority candidates are decided first, as release planning asked,
  but built where the order puts them.
- **Decided (2026-10-20): benchmarks don't run in CI.** Timings on shared runners are noisy,
  and comparing them reliably is more machinery than it's worth for now. Regressions are caught
  by running the benchmarks by hand at each step and comparing with the results in BENCHMARKS,
  which are updated whenever a step makes Proteus faster. Running them in CI stays a candidate
  in ROADMAP.

### Step 1 — Baseline

A baseline before any performance work, so every later step is measured against it: the
results in BENCHMARKS, and the reference demo on a still screen.

- [x] CPU and GPU use with the reference demo sitting still are measured, on a low-end device or
  with Chrome's CPU throttling, as the baseline for step 2. On a Lenovo Chromebook with Intel i3 processor, in
  ChromeOS's Diagnostics, with the demo on WebGL2: 8 to 11% CPU with the demo still, against 3%
  without it. It never overheated.
- [x] The browser benchmark is published beside the demo, at `/bench/` on GitHub Pages and in
  each pull request's staging preview, so people with other machines can run it and send the
  results. Its saved progress is kept apart for each address, since production and staging
  share an origin. BENCHMARKS gives the address and what to send.

### Step 2 — Skip idle frames

- [x] A frame in which nothing changed isn't drawn: no running transition, input, change to a
  component, pending bake or new video frame. A coarse check is enough; it doesn't wait for
  step 4's tracking.
- [x] A test for each of those shows the frame is drawn when it changes.
- [x] Step 1's still-screen measurement is taken again, and shows the difference. On the same
  Chromebook, with the demo still: 4 to 5% CPU, down from 8 to 11%, against 3% without the
  demo, and the CPU at 30 °C rather than 32. What's left is `update`, the tick and collecting the instances, which still run every
  frame; step 4 reduces the collecting.

### Step 3 — Decide the corners approach

- [x] One of the approaches in the [design note](./docs/design/shared-bake-corners.md) is
  decided, before step 4 changes the instance: a mask per shared bake, a radius per corner, or
  mipmaps for bakes. What it needs from the instance, if anything, is recorded here.
- **Decided (2026-10-22): a radius per corner.** A slice at one of the source's corners gets
  that corner's radius and its other corners none, and the shader draws the curve at whatever
  size the slice is drawn, as it already does for a whole component. A mask would be minified
  like the bake, and alias the same way; mipmaps cost a third more atlas memory, a mip pass
  after each bake, and padding against bleeding between atlas regions, so they stay a
  candidate for minified content in general. **The instance needs** four corner radii in place
  of `corner_radius`, packed as `Float16x4`, four more bytes; it fits without a new attribute
  slot by pairing `border_offset`, alone in slot 12, with `opacity`, which frees slot 6 for the
  radii. Step 4 owns the final layout.
- **Decided (2026-10-22): the source is baked with square corners.** `checks/corners` shows a
  second problem besides the aliasing: a corner slice crossfades between two corner shapes,
  the curve baked into the source's image and the target's own, which the shader draws. With a
  radius per corner, a piece has one curve for its whole transition, going from its corner of
  the source to the target's, and the shader clips both images with it. A curve baked into the
  image would show through once the shader's is tighter, so the bake has none. Still open, for
  step 8: a source with a border, whose border baked square would be square inside at the
  corners.

### Step 4 — Per-frame work

- [x] Each frame collects, sorts and uploads only the components that changed, and keeps the
  drawing order until a `z` or the hierarchy changes.
  [Design note](./docs/design/per-frame-work.md)
- [x] If rarely used fields, such as shadow and glow, move out of `QuadInstance`, the attribute
  slots it frees are left for steps 6 and 8. They stay: uploading only what changed makes the
  instance's size matter less, and step 3's radii fit without a new slot. The instance now has
  four corner radii as `Float16x4` in slot 6, and `opacity` beside `border_offset` in slot 12:
  164 bytes. The shader shapes each corner by its own radius; every component still gives all
  four the same one, until step 8.
- [x] Step 2's check for a changed frame uses this tracking.
- [x] Measured against step 1's baseline, and BENCHMARKS updated. Natively, a still frame of
  100,000 small components takes 1.3 ms of CPU, against 7.9 before; the moving scene is
  unchanged. In the browsers, half a million small components now draw at the display's full
  rate, and a million take 24 to 30 ms a frame, against 85 to 89 before: as fast as the
  hand-batched renderer in Safari, and 1.7 times slower in Chrome, against 3 and 5 before.

### Step 5 — Split `quad.wgsl`

- [x] `quad.wgsl` is split into named parts with its output unchanged. Six parts in
  `crates/proteus-render/src/shaders/quad/`, joined into one shader at compile time: the
  bindings, the vertex stage, the shape, the fill, the effects, and `fs_main`, which now reads
  as those steps in turn. A scene with every feature rendered identically, byte for byte,
  before and after.
- [x] The test that checks the shader's atlas-page layout against Rust's survives the shader
  being reformatted: it parses the shader with naga and compares the constants' values.

### Step 6 — The shader fast path

[Design note](./docs/design/shader-fast-path.md)

- [x] Plain components, with no image, border, shadow or glow, exit `fs_main` early, without the
  texture sample, border or crossfade. Measured against step 1's baseline. The early exit alone
  saved only 8%: measured natively, the full shader's size, not its work, is most of the cost,
  since the GPU sets aside room for its most demanding path whichever path a pixel takes. So
  plain components get a pipeline of their own; see the decision below. The values passed from
  the vertex stage are `flat` where they're the same at every corner, which is all but the
  pixel's position.
- **Decided (2026-10-22): a second pipeline for plain components,** a solid color with no
  texture, border, shadow, glow or crossfade, drawn in runs in drawing order, a draw call each.
  A run gets its own call only if it covers at least a 64 × 64 area or has 32 components, so a
  button's background between labels doesn't cost one; past 256 runs, everything is drawn in
  one call. It draws exactly the same pixels, checked by a test, and
  `RenderConfig::plain_pipeline` turns it off. Natively, the large scene's frame went from
  108.7 to 51.6 ms at 100,000, from 25.7 to 11.8 at 25,000 and from 10.3 to 5.3 at 10,000; the
  small and moving scenes didn't change.
- [x] A component that only shows video, with no border, shadow or glow, takes a fast path too,
  sampling the video texture and nothing else. Measured with the demo's video full screen; the
  saving is expected to be small next to the cost of getting frames to the GPU, which v0.5.0
  addresses. It takes the full shader's shorter path, for any component with no border,
  shadow, glow or crossfade. Measured with the native benchmark's new `video` scene, a new
  1280 × 800 frame uploaded every frame, it makes no difference: one full-screen video takes
  0.45 to 0.50 ms a frame before and after, and even 50 stacked, 4.7 to 4.9 ms, within the
  runs' own variation. A video's cost is uploading the frame and filling the pixels.
- [x] Pixels a later opaque component covers aren't shaded, with a depth buffer, measured on the
  large scene against the early exit and on the small scene to check it costs nothing there.
  Natively, the large scene's frame went from 50.9 to 5.1 ms at 100,000, from 12.3 to 1.9 at
  25,000 and from 5.2 to 0.32 at 10,000; the small and moving scenes didn't change.
- **Decided (2026-10-22): the depth pre-pass draws a shrunk rectangle, not the rounded
  shape.** A pre-pass that worked out each pixel's distance from the rounded edge, as the design
  note sketched, would do that work for every overdrawn pixel, back to front, which is the
  cost it's meant to save. So it draws each opaque component's rectangle inset by its largest
  radius and 2 pixels, where every pixel is certainly fully opaque, and does no work per pixel.
  Depth is each instance's place in drawing order, from `instance_index`, which wgpu counts from
  the start of the buffer on WebGL2 too. "Opaque" is a plain component with full alpha and
  opacity; opaque images could join it later, with a flag recorded when they're decoded.
  `RenderConfig::skip_covered_pixels` turns it off.
- [x] BENCHMARKS updated, checking whether frame times even out and whether Safari's tab still
  crashes at 100,000 large components. Both: in both browsers, the large scene's p99 is now
  within a few milliseconds of its median, and Safari holds the display's full rate at 100,000
  large components without crashing, so the browser benchmark's large scene now goes to
  100,000. Proteus went from the slowest renderer there to the fastest: 19.9 ms a frame at
  100,000 in Chrome and 17 in Safari, against 100 to 136 for the WebGL2 renderers written by
  hand.
- [x] The reference demo, its Effects and Text examples especially, is checked on both shells.

### Step 7 — Bake at the display's resolution

- [x] Text and baked components are baked at the display's resolution and drawn at their
  logical size, and baked again when the scale factor changes.
  [Design note](./docs/design/bake-resolution.md) Text, baked components and the bakes of
  splits and merges all bake at the display's scale factor, and text bakes again when it
  changes. A baked component doesn't: baking destroys its children and its own color and
  effects, so there is nothing to bake it again from.
- **Decided (2026-10-27): bakes are capped at a scale of 2,** by
  `ResourceConfig::max_bake_scale`. It makes them pixel-exact on the common high-density
  displays, Retina Macs and browsers at a pixel ratio of 2, while a bake at scale `s` takes `s²`
  the atlas space: 4× at 2, 9× at 3. 3× phones get 2× bakes, at 44% of the memory 3 would
  take. A bake too large for its atlas at the scale takes the largest scale that fits. The
  configuration guide explains the costs and when to change it.
- **Decided (2026-10-27): a baked component keeps the scale it was first baked at.** Baking it
  again on a scale change would need `.bake()` to keep its subtree, hidden, rather than destroy
  it; that is a candidate in ROADMAP.

### Step 8 — Rounded corners on baked slices

- [ ] With step 3's approach, a slice's corners stay round while it is drawn smaller than its
  bake, and change size as one curve, with no crossfade between two corner shapes. Checked with
  `cargo run --release -p check-corners`, on a fill, an image and a border.
  [Design note](./docs/design/shared-bake-corners.md)

### Any time — Top-left coordinates in Rust

Independent of the other steps.

- [ ] Rust has equivalents of TypeScript's `topLeftToWorld` and `worldToTopLeft`, and the
  guides make the origin and the conversion clear in both languages.

### Step 9 — Close out

- [ ] ARCHITECTURE's known limits no longer list what this release fixes.
- [ ] Released through the process in [RELEASING.md](./RELEASING.md).

## Standing decisions

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
