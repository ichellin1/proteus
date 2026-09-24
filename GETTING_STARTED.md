# Getting Started

Detailed setup instructions for running the reference demo. See [README.md](./README.md)
for a project overview, and [PLANNING.md](./PLANNING.md) / [ROADMAP.md](./ROADMAP.md) for
architecture and milestones.

## What runs what

One crate, `proteus-demo`, *is* the reference demo: it implements `proteus_runtime::App` and
knows nothing about windows, canvases or GPUs. The two shells are thin entry points that hand
it to a host — `proteus-shell-native` → `proteus-host-winit`, `proteus-shell-web` →
`proteus-host-web`. "Native shell" and "web shell" below mean those entry points plus their
host; the demo itself is the same code either way.

`examples/gallery` is the third front door: the TypeScript SDK, driven from TS with no Rust in
the application at all.

## Platform support

The native shell is currently only built and verified on **macOS**. It's plain `wgpu` +
`winit` with no macOS-specific code, so Linux/Windows likely work too, but they haven't been
tested — a cross-platform CI matrix is planned for M14 (see [ROADMAP.md](./ROADMAP.md)). The
web shell runs in any browser with WebGL2 (Chrome, Firefox, Safari) and isn't platform-limited.

## Install dependencies

Both shells:

- **Rust toolchain** — install via [rustup](https://rustup.rs/) if you don't already have
  `cargo`.

Native shell only:

- **ffmpeg / ffprobe** on `PATH` — native decodes MP4 by shelling out to `ffmpeg`
  (`crates/proteus-host-winit/src/mp4_player.rs`). On macOS: `brew install ffmpeg`. Without
  it, video playback logs a warning and skips, but the tile↔screen morph still runs.

Web shell only:

- **wasm-pack** — `cargo install wasm-pack`, used to build the WASM bundle.
- **Python 3** — `make serve-web` uses its built-in HTTP server to serve
  `crates/proteus-shell-web/www/`. Not needed if you'd rather serve that directory with
  something else (anything that serves static files works).

TypeScript SDK only:

- **Node 20+** — for `crates/proteus-sdk-web/ts/` and `examples/gallery`. `wasm-pack` is
  needed here too; `make build-sdk-web` runs it for both `proteus-sdk-web` and
  `proteus-host-web` before invoking `tsc`.

## Demo assets

The box-cover images and video clips are committed directly under each shell's asset
directories (`crates/proteus-shell-native/images/` + `assets/videos/`, and
`crates/proteus-shell-web/www/images/` + `www/videos/`) — nothing to download or fetch
separately, they come with the repo.

The web shell plays HLS, not the `.mp4`s beside it: `www/videos/hls/{tiger,sintel,jellyfish}/`
hold the segmented output, produced from those source files by `www/videos/make_hls.sh`. Re-run
that script if you replace them.

### Using your own assets

To swap in your own images/videos, place them at the same paths and filenames. The
authoritative lists are in each shell's entry point, not in any HTML or config file:

- native — `ASSET_DIR` and `TILE_VIDEO_PATHS` in `crates/proteus-shell-native/src/main.rs`
- web — `asset_keys()` and `video_keys()` in `crates/proteus-shell-web/src/lib.rs`

Image keys (`bg/…`, `icons/…`, `logo/…`) resolve against the host's own asset base: a
directory on native, a set prefetched before startup on the web. Video keys don't — native
takes literal filesystem paths and the web takes an HLS manifest path plus a codec string.
Both lists are kept in lockstep with what `DemoApp::setup` asks for, by hand.

Without assets, the demo still runs — tiles just fall back to solid-color placeholders and
there's no video.

## Building and Running on Native

```
cargo run -p proteus-shell-native
```

## Building and Running on a Web Browser

The page fetches its wasm, video, and image assets, so it needs to be served over HTTP —
opening `crates/proteus-shell-web/www/index.html` directly as a `file://` URL won't work.
Build and serve in one step:

```
make serve-web
```

This builds the WASM bundle (`wasm-pack build crates/proteus-shell-web --target web --out-dir
www/pkg`) then serves `crates/proteus-shell-web/www/` on <http://localhost:8080> via Python's
built-in HTTP server. To just build (e.g. to serve it with a different HTTP server), run `make
build-web` on its own.

The web shell decodes video via the browser's own `<video>` element and `MediaSource`
(`crates/proteus-host-web/src/hls_video.rs`), so there's no `ffmpeg` dependency on this
target.

## Building the TypeScript SDK

```
make build-sdk-web
```

Builds `proteus-sdk-web` and `proteus-host-web` to wasm (into `ts/pkg` and `ts/pkg-host`),
then compiles the TypeScript package into `crates/proteus-sdk-web/ts/dist/`. Run this before
the example below — it depends on the package by `file:`, resolving into `dist/`.

## Running the TypeScript example

```
cd examples/gallery
npm install
npm run dev
```

Vite serves it with hot reload. `npm run typecheck` checks it without building.

## Tests

```
make check
```

Runs the same fmt + clippy + test checks CI does. Each is also its own target
(`make fmt` / `make clippy` / `make test`) if you want to run just one.
