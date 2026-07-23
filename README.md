# Proteus

Proteus is a cross-platform UI framework written in Rust. Its defining idea: **UI components are metamorphic.** They do not navigate to new screens or swap out for different components — they *transform* into them. A button can become a list, a list item can become a video player, and the transition between forms is a first-class, visually continuous experience. Rendering is GPU-native via wgpu — **WebGL2** on the web (via WASM, with automatic **WebGPU** upgrade where available) and **Vulkan / Metal / DirectX 12** on native platforms.

## Read First

→ [VISION.md](./VISION.md) — the philosophy and principles
→ [ROADMAP.md](./ROADMAP.md) — milestones and sequencing
→ [PLANNING.md](./PLANNING.md) — full architecture decisions and definitions of done
→ [GETTING_STARTED.md](./GETTING_STARTED.md) — dependencies, demo assets, build & run instructions

## Crate Structure

```
crates/
  proteus-gpu/          # Layer 0: wgpu device abstraction
  proteus-render/       # Layer 1: scene graph, instanced render pipeline, transition pipeline
  proteus-ui/           # Layer 2: metamorphic component model, transition topologies
  proteus-shell-web/    # Layer 3: WebGL2/WebGPU WASM shell, TypeScript bridge
  proteus-shell-native/ # Layer 3: native windowing shell (winit)
```

## Build & Run

The reference demo (a "START" button that morphs into three video tiles, each of which morphs
into a full playback screen) runs on both shells from the same `proteus-ui`/`proteus-render` core.

See **[GETTING_STARTED.md](./GETTING_STARTED.md)** for dependency installation, demo-asset
setup, and full run/test instructions for both shells. Quick version, once dependencies and
assets are in place:

```
cargo run -p proteus-shell-native                                          # native
wasm-pack build crates/proteus-shell-web --target web --out-dir www/pkg    # web (build)
python3 -m http.server 8000 --directory crates/proteus-shell-web/www       # web (serve)
```

The native shell is currently only built and verified on macOS; the web shell runs in any
WebGL2-capable browser.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
