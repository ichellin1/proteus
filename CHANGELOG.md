# Changelog

The changes in each version of Proteus. This file is generated from the commit history; see
RELEASING.md.

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
