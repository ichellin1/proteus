# Changelog

The changes in each version of Proteus. This file is generated from the commit history; see
RELEASING.md.
## Unreleased

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

### Changes

- **Breaking:** Rename and reshape the public API before release

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

### Build and release

- Check manifest comments, and TypeScript docs with typedoc
- Ignore the local review notes
- **examples:** A folder per language, make targets, and builds in CI
- Make check and the pre-push hook run the docs build, as CI does
- Crates ready for crates.io, and READMEs for every package
