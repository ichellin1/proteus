# Proteus

> *A new UX paradigm and GPU-native rendering framework built on shapeshifting as a first-class design primitive.*

Proteus is a cross-platform UI framework written in Rust. It targets **WebGPU** on the web (via WASM) and **Vulkan / Metal / DirectX 12** on native platforms. Its defining idea: user interfaces should continuously adapt their structure, visual form, and interactive surface — not through static themes or breakpoints, but through a live context model driven by who the user is, what they're trying to do, and where they are.

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

🚧 **Phase 0 — Foundation** — repository scaffolding and vision in progress.

## License

TBD
