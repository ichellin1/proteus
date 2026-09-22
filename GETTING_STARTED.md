# Getting Started

Detailed setup instructions for running the reference demo. See [README.md](./README.md)
for a project overview, and [PLANNING.md](./PLANNING.md) / [ROADMAP.md](./ROADMAP.md) for
architecture and milestones.

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
  (`crates/proteus-shell-native/src/mp4_player.rs`). On macOS: `brew install ffmpeg`. Without
  it, video playback logs a warning and skips, but the tile↔screen morph still runs.

Web shell only:

- **wasm-pack** — `cargo install wasm-pack`, used to build the WASM bundle.
- **Python 3** — `make serve-web` uses its built-in HTTP server to serve
  `crates/proteus-shell-web/www/`. Not needed if you'd rather serve that directory with
  something else (anything that serves static files works).

## Demo assets

The box-cover images and video clips are committed directly under each shell's asset
directories (`crates/proteus-shell-native/images/` + `assets/videos/`, and
`crates/proteus-shell-web/www/images/` + `www/videos/`) — nothing to download or fetch
separately, they come with the repo.

### Using your own assets

To swap in your own images/videos, place them at the same paths and filenames (see
`TILE_IMAGE_PATHS`/`TILE_VIDEO_PATHS` in `crates/proteus-shell-native/src/main.rs`, and
`TILE_IMAGE_SRCS`/`TILE_VIDEO_SRCS` in `crates/proteus-shell-web/www/index.html`, for the
authoritative list). Without assets, the demo still runs — tiles just fall back to
solid-color placeholders and there's no video.

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

The web shell decodes video via the browser's own `<video>` element, so there's no `ffmpeg`
dependency on this target.

## Tests

```
make check
```

Runs the same fmt + clippy + test checks CI does. Each is also its own target
(`make fmt` / `make clippy` / `make test`) if you want to run just one.
