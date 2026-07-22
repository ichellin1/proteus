# Proteus

> *A new UX paradigm and GPU-native rendering framework built on shapeshifting as a first-class design primitive.*

Proteus is a cross-platform UI framework written in Rust. Its defining idea: **UI components are metamorphic.** They do not navigate to new screens or swap out for different components — they *transform* into them. A button can become a list, a list item can become a video player, and the transition between forms is a first-class, visually continuous experience. Rendering is GPU-native via wgpu — **WebGL2** on the web (via WASM, with automatic **WebGPU** upgrade where available) and **Vulkan / Metal / DirectX 12** on native platforms.

## Read First

→ [VISION.md](./VISION.md) — the philosophy and principles
→ [ROADMAP.md](./ROADMAP.md) — milestones and sequencing
→ [PLANNING.md](./PLANNING.md) — full architecture decisions and definitions of done

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

### Native

```
cargo run -p proteus-shell-native
```

Requires `ffmpeg`/`ffprobe` on `PATH` for MP4 playback (native decodes video by shelling out to
`ffmpeg`; see `crates/proteus-shell-native/src/mp4_player.rs`). Without it, playback just logs a
warning and the tile↔screen morph still runs.

Box-cover art and video clips are read from disk and are not committed to the repo — place your
own at `crates/proteus-shell-native/images/` and `crates/proteus-shell-native/assets/videos/`
(see the comments above `TILE_IMAGE_PATHS`/`TILE_VIDEO_PATHS` in `main.rs` for exact filenames).
Missing files degrade gracefully to placeholder colors/no video, they don't crash the app.

### Web

```
wasm-pack build crates/proteus-shell-web --target web --out-dir www/pkg
```

Then serve `crates/proteus-shell-web/www/` over HTTP (not `file://` — the page fetches its wasm,
video, and image assets) and open it in a browser, e.g.:

```
python3 -m http.server 8000 --directory crates/proteus-shell-web/www
```

Same asset requirement as native — place box-cover images and video clips at
`crates/proteus-shell-web/www/images/` and `crates/proteus-shell-web/www/videos/` (see
`TILE_IMAGE_SRCS`/`TILE_VIDEO_SRCS` in `www/index.html`). The web shell decodes video via the
browser's own `<video>` element, so there's no `ffmpeg` dependency on this target.

### Tests

```
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --check
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE) at your option.
