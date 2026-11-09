# Changelog

The changes in each version of Proteus. This file is generated from the commit history; see
RELEASING.md.

## 0.3.0 (2026-11-09)

### Upgrading from 0.2

- **TypeScript apps built against this repository's SDK,** as the examples are, need it built
  again: run `make build-sdk-web` before `npm install`. The SDK from npm needs nothing.
- **`InputConfig` written out in full** no longer compiles in Rust: `autofocus`, `keep_tab`,
  `key_repeat`, `long_press_ms` and `navigating_actions` are new. Fill in the rest with
  `..Default::default()`. It's no longer `Copy`, since it holds a list of actions. Its
  `focus_input_delay_ms`, `click_moves_focus`, `drag_threshold_px`,
  `transition_interaction_allow_pointer` and `transition_interaction_allow_navigation` are
  removed: none of them did anything. A transition's input is set per component, with
  `transition_interaction`.
- **`StyleOverride` written out in full** no longer compiles in Rust: it has fields for glow,
  border and drop shadow. Fill in the rest with `..StyleOverride::default()`.
- **The web canvas can take keyboard focus:** the host gives it `tabindex="0"` unless the page
  set one, so a click focuses it, and a browser may outline it while it has focus. While it has
  focus, the browser doesn't act on the keys the key map binds, so Space and the arrows no
  longer scroll the page. Tab and Shift+Tab move focus within the app, and leave the canvas
  past its last component or before its first; set `input.keep_tab` to keep them in, wrapping
  round, for a full-screen app.
- **Focus leaves a component that can no longer keep it:** when the focused component is
  destroyed, hidden, disabled or made non-interactive, focus is cleared, its `on_blur` fires, and
  the app's `on_focus_lost` callback is called with the component and the reason. In 0.2 it kept
  focus, and showed its focused style again when re-enabled.
- **Enter and Space click the focused component:** a pointer click gives a component focus, so
  in an app that doesn't handle keys, Enter or Space clicks the last component clicked again.
  A component that shouldn't be clicked this way handles `Action::Activate` itself, or doesn't
  accept the keyboard.
- **The focused style shows only after a navigating key, and the hover style only while
  pointing:** a click still gives focus, but its focused style waits until the arrows or Tab are
  pressed, and the hover style hides until the pointer moves again. In 0.2 a click showed the
  focused style at once.

### Features

- **Breaking:** **input:** Keys reach the engine from both hosts, with autofocus for the web canvas
- **input:** The app and each component choose which input devices they accept
- **input:** Focus that components take, follow into what they transform into, and lose when they cannot keep it
- **input:** A key map binds keys to actions, and key callbacks run from the focused component up to the app
- **input:** The engine repeats held keys and reports long presses, with the timing in the app config
- **input:** Enter and Space press and click the focused component, as the pointer does
- **input:** The web host keeps the browser from acting on bound keys, and Tab can leave the canvas unless the app keeps it
- **input:** The last input wins, showing the focused style only while navigating and hover only while pointing
- **input:** The arrows, Tab and Shift+Tab move focus, with named neighbours and reading direction
- **input:** Focus groups keep, wrap or let go of each action at their edge, and choose where focus enters
- **input:** Tab leaves the web canvas only past the first or last component
- **ui:** Interaction styles change glow, border and drop shadow, and each component times its own style changes
- **examples:** Browse, a TV-style screen used with the keys or the pointer
- **demo:** The home page is one grid of features, with Keyboard & Focus, Layout and About panels
- **demo:** The demo works with the keys on every screen, with interaction styles in place of its own hover code
- **input:** An app that does not accept the pointer navigates from the start

### Fixes

- **ui:** Splits, merges and channels place components with parents where they are on screen
- **input:** Caps Lock arrives as a press and a release at each tap, on every host
- **ui:** Interaction styles no longer leave a component at a stale geometry or glow

### Changes

- **Breaking:** **config:** Remove the five InputConfig settings that did nothing

### Documentation

- A Keyboard and focus guide, a how-to for keyboard navigation, and the other guides and ARCHITECTURE updated for the keyboard

### Build and release

- Run the Linux tests on Ubuntu 24.04, as Mesa 26 crashes the GPU tests on 26.04

## 0.2.0 (2026-10-28)

### Upgrading from 0.1

Most apps need no changes: the API apps use is the same in both SDKs.

- **Config structs written out in full** no longer compile in Rust: `frame.skip_idle_frames`,
  `render.plain_pipeline`, `render.skip_covered_pixels` and `resources.max_bake_scale` are new,
  each on by default. Fill in the rest with `..Default::default()`. TypeScript's config is
  unchanged, since every setting is optional there.
- **Custom hosts that build instances:** `proteus_render::QuadInstance` has a radius per corner,
  `corner_radii`, in place of `corner_radius`. Set it with `set_corner_radius` or
  `set_corner_radii`.
- **A still screen isn't drawn:** `update` still runs every frame, but when nothing changed the
  GPU draws nothing. Set `frame.skip_idle_frames` to `false` to draw every frame regardless of
  changes, as 0.1 did.
- **Text and baked components are baked at the display's resolution,** up to twice the logical
  size, so they take up to four times the atlas space on high-density displays. An app that
  filled its atlas in 0.1 can lower `resources.max_bake_scale`, or raise its memory. Component
  level overrides will be added in a future release.
- **A corner radius past half the shorter side draws as half,** as in CSS, so a short, wide
  component with a large radius is a pill. In 0.1 the shape shrank instead.

### Features

- **Breaking:** **runtime:** Skip drawing frames that didn't change
- **Breaking:** **render:** Bake text and components at the display's resolution, up to twice the logical size
- **demo:** Round the dark theme's buttons, tiles and panels twice as much as the light theme's
- **sdk:** Rust's top_left_to_world and world_to_top_left, as in TypeScript
- **sdk:** Declare_geometry and declared_geometry, to change where a component rests without moving it now

### Fixes

- **render:** Draw a radius past half the shorter side as half, as CSS does
- **ui:** Give the pieces of a split or merge one corner curve that changes size, with no crossfade between two

### Performance

- **Breaking:** **render:** Redo and upload only the components that changed, so a still frame of 100,000 takes 1.3 ms of CPU, not 7.9, and a million draw 3 times faster in the browser
- **Breaking:** **render:** Draw plain components with a pipeline of their own, halving large scenes' frame time: 108.7 to 51.6 ms at 100,000 natively, 95 to 48 ms at 25,000 in Chrome
- **Breaking:** **render:** Skip pixels a later opaque component covers, 10 times faster in stacked scenes: 50.9 to 5.1 ms at 100,000 large components natively

### Changes

- **render:** Split the shader into named parts

### Documentation

- **benchmarks:** Add results from a Chromebook
- **benchmarks:** V0.2.0's results, with the large scene up to 100,000 components

### Build and release

- **pages:** Publish the browser benchmark beside the demo
- **bench:** A full-screen video scene in the native benchmark
- **release:** Publish to npm verbosely, with a token only when the secret is set

## 0.1.1 (2026-10-20)

### Fixes

- **gpu:** Make the WebGL2 fallback work

### Documentation

- Badges in the README, and finishing a release by hand
- Plan by release instead of milestones
- A draft release order, and v0.2.0's planning
- Skip drawing idle frames in v0.2.0

### Build and release

- **cliff:** Point skipped commits at the rewritten history

## 0.1.0 (2026-10-15)

The first release of Proteus.

- **Components:** shapes with a position, size, rotation, scale, color and corner radius, with
  borders, drop shadows and glows; children placed relative to their parent; and baking, which
  draws a component and its children as one image.
- **Transitions:** a component moving to new geometry, one component turning into another
  through a transition channel (1→1), a split into several (1→N) and a merge of several into
  one (N→1), with built-in easing curves and any cubic Bézier.
- **Interaction:** pointer events, and hover, pressed, focused and disabled styles, with hit
  testing that follows rotation and drawing order.
- **Content:** a line of text in the app's font, images and textures in a texture atlas that
  makes room for new ones as needed, and video from the app's own player (experimental).
- **Hosts:** a native host, for macOS, Windows and Linux, and a web host, with WebGPU or
  WebGL2. Apps are written in Rust, or in TypeScript with the `proteus-sdk` npm package.
- **Documentation:** getting-started guides, concept guides and how-tos, in Rust and
  TypeScript; an API reference; and examples in both languages.

The changes below were made while preparing this release.

### Features

- Custom easing with cubic-Bézier curves
- **Breaking:** Bring your own video player
- **examples:** The gallery in Rust, and a loader while the grid downloads
- **examples:** The video example in Rust
- **examples:** The menu example
- **examples:** The stepper example, in Rust and TypeScript
- **examples:** The menu example in Rust

### Fixes

- Input and event dispatch
- Rendering and textures
- Config, loading and robustness
- **Breaking:** Resting geometry, draw order and the TypeScript API
- **ci:** A colon in a step name broke the workflow
- **sdk-web:** Undefined optional fields no longer throw
- **sdk:** Transitions into a disabled component end in its disabled style
- **examples:** Doc links that broke the docs build
- **render:** Draw a component whatever its z

### Performance

- **ui:** Write visibility and opacity only when they change

### Changes

- **Breaking:** Rename and reshape the public API before release
- **runtime:** Remove the unused eviction setting

### Documentation

- **contributing:** Add the writing standard and a comment checker
- **sdk:** Rewrite proteus-sdk's API docs to the writing standard
- **contributing:** Add plain-sentence and "every other" rules
- Rewrite code comments and API docs to the writing standard
- Set up the guides, snippet checks and API reference
- **guide:** Getting started in Rust and TypeScript
- **guide:** Components, 1→1 transitions, and splits and merges
- **guide:** Interaction, content, configuration, and hosts
- **how-to:** Chaining, staggering, images, easing, platform features, and tricks
- Check links between pages
- Complete CONTRIBUTING, and RELEASING for the library and the demo
- What's tested on each platform, and a native-and-web parity checklist
- Benchmark results
- An architecture document
- Rewrite the README
- The logo and links on the npm and crates.io pages

### Build and release

- Check manifest comments, and TypeScript docs with typedoc
- Ignore the local review notes
- **examples:** A folder per language, make targets, and builds in CI
- Make check and the pre-push hook run the docs build, as CI does
- Crates ready for crates.io, and READMEs for every package
- Generate the changelog with git-cliff
- A script that sets and checks the version everywhere
- Release workflow for a version tag, and a workflow lint
- One copy of the demo's assets, and remove unused media and design files
- A blank line before each version in the changelog
- Test on macOS, Linux and Windows, check Rust 1.89, and name skipped GPU tests
