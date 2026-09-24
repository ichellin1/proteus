# Proteus

Proteus is a cross-platform UI framework written in Rust. Its defining idea: **UI components are metamorphic.** They do not load into new screens or swap out for different components — they *transform* into them. A button can become a list, a list item can become a video player, and the transition between forms is a first-class, visually continuous experience. Rendering is GPU-native via wgpu — **WebGL2** on the web (via WASM, with automatic **WebGPU** upgrade where available) and **Vulkan / Metal / DirectX 12** on native platforms.

## Read First

→ [VISION.md](./VISION.md) — the philosophy and principles
→ [ROADMAP.md](./ROADMAP.md) — milestones and sequencing
→ [PLANNING.md](./PLANNING.md) — full architecture decisions and definitions of done
→ [GETTING_STARTED.md](./GETTING_STARTED.md) — dependencies, demo assets, build & run instructions
→ [RELEASING.md](./RELEASING.md) — release strategy and deploy steps for the web reference demo

## Crate Structure

```
crates/
  proteus-gpu/          # Layer 0: surface + device/queue/swap-chain setup, shared by both hosts
  proteus-render/       # Layer 1: instanced render pipeline, atlases, offscreen bake pipeline
  proteus-ui/           # Layer 2: metamorphic component model, transition topologies
  proteus-sdk/          # Layer 2.5: generic app-authoring API (component/signal/texture) — headless
  proteus-sdk-web/      # Layer 2.5 (web): wasm-bindgen bridge + npm-publishable TypeScript SDK (ts/)
  proteus-runtime/      # Layer 2.75: Renderer + Engine + the App / HostServices contracts
  proteus-host-winit/   # Layer 3: native host — winit window, frame loop, file-backed assets
  proteus-host-web/     # Layer 3: wasm host — canvas, rAF loop, fetched assets, HLS video
  proteus-demo/         # the reference demo, written once as an App
  proteus-shell-native/ # Layer 4: a `main()` that hands the demo to proteus-host-winit
  proteus-shell-web/    # Layer 4: a wasm entry point that hands it to proteus-host-web
examples/
  gallery/              # the TypeScript front door — the SDK driven from TS, no Rust
```

**Writing an app** means implementing `proteus_runtime::App` (`setup` once, `update` per frame)
and handing it to a host's `run()`. The host owns the window or canvas, the GPU surface, the
frame loop and input; `Engine` owns `Proteus` and calls into the app. Nothing forks a shell, and
the same app runs on both platforms — `proteus-demo` is exactly this, and the two `proteus-shell-*`
crates below it are thin entry points. From TypeScript, `mount()` plays the host's role instead;
see `examples/gallery`.

## Reference Demo

The **[reference demo](https://ichellin1.github.io/proteus/)** shows Proteus in action across
three sections: real video playback, a photo gallery backed by live image fetches, and a set of
framework examples (effects, text, transforms, stress tests).

## Build & Run Reference Demo

One `proteus-demo` crate drives both platforms; each shell just picks the host.

See **[GETTING_STARTED.md](./GETTING_STARTED.md)** for dependency installation, demo-asset
setup, and full run/test instructions for both shells. Quick version, once dependencies and
assets are in place:

```
cargo run -p proteus-shell-native   # native
make serve-web                      # web (builds, then serves on :8080)
```

The native shell is currently only built and verified on macOS; the web shell runs in any
WebGL2-capable browser.

To build the TypeScript SDK and its example instead:

```
make build-sdk-web                  # wasm + tsc, into crates/proteus-sdk-web/ts/dist
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
