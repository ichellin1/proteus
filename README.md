# Proteus

> *A new UX paradigm and GPU-native rendering framework built on shapeshifting as a first-class design primitive.*

Proteus is a cross-platform UI framework written in Rust. Its defining idea: **UI components are metamorphic.** They do not navigate to new screens or swap out for different components — they *transform* into them. A button can become a list, a list item can become a video player, and the transition between forms is a first-class, visually continuous experience. Rendering is GPU-native via wgpu — **WebGL2** on the web (via WASM, with automatic **WebGPU** upgrade where available) and **Vulkan / Metal / DirectX 12** on native platforms.

## Read First

→ [VISION.md](./VISION.md) — the philosophy, architecture, and roadmap

## Crate Structure

```
crates/
  proteus-gpu/          # Layer 0: wgpu device abstraction
  proteus-render/       # Layer 1: scene graph, materials, transition pipeline
  proteus-ui/           # Layer 2: metamorphic component model, transition topologies
  proteus-shell-web/    # Layer 3: WebGL2/WebGPU WASM shell, TypeScript bridge
  proteus-shell-native/ # Layer 3: native windowing shell (winit)
```

## Status

🚧 **M0 — Foundation** — vision (Phase A), architecture (Phase B), and dependencies & tooling (Phase C) complete; project plan & roadmap (Phase D) up next. See [PLANNING.md](./PLANNING.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
