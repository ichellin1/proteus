# Contributing to Proteus

This guide covers building Proteus from source, and how code comments, documentation and
commit messages are written in this repository. To use Proteus in your own app, start with the
[guides](./docs/README.md) instead.

## Building from source

### What runs what

One crate, `proteus-demo`, is the reference demo: it implements `proteus_runtime::App` and
knows nothing about windows, canvases or GPUs. Two shells hand it to a host:
`proteus-shell-native` to `proteus-host-winit`, and `proteus-shell-web` to `proteus-host-web`.
Each shell supplies only what differs by platform: where the assets are, the window or canvas,
and a video player. The demo itself is the same code either way.

`examples/` holds complete apps, most in both Rust and TypeScript; see
[examples/README.md](./examples/README.md). The published crates and the npm package are listed
in [RELEASING.md](./RELEASING.md).

### Platform support

The native shell is only built and tested on macOS so far. It uses `wgpu` and `winit` with no
macOS-specific code, so Linux and Windows will likely work, but they haven't been tested yet.
The web shell runs in any browser with WebGL2.

### Dependencies

**Rust** 1.89 or later. Check with `cargo --version`. If you don't have it, install it with
`rustup`, Rust's installer. On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows:

```bash
winget install Rustlang.Rustup
```

**The WebAssembly target and `wasm-pack`,** for the web shell, the TypeScript SDK and the
checks, which lint the web crates for WebAssembly:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**Node 20 or later,** for the TypeScript SDK and examples. On macOS, `brew install node`; on
Windows, `winget install OpenJS.NodeJS.LTS`; on Ubuntu, `sudo apt install nodejs npm`.

**Python 3,** whose HTTP server `make serve-web` and the `make example-…-web` targets use. It
comes with macOS and most Linux distributions. Any server for static files works instead.

**`ffmpeg` and `ffprobe`,** for video in the native shell and the Rust video example, which run
them to decode MP4. On macOS, `brew install ffmpeg`; on Windows, `winget install Gyan.FFmpeg`; on
Ubuntu, `sudo apt install ffmpeg`. Without them, video logs a warning and is skipped; the rest of
the demo works.

**Optional:** `actionlint`, which the pre-push hook runs on the GitHub workflows when it's
installed (`brew install actionlint`), and `git-cliff`, which releasing needs
(`cargo install git-cliff`).

### Demo assets

The demo's images and videos are committed in `crates/proteus-demo/assets/`, one copy for both
shells. There is nothing to download.

- **The native shell** reads them there.
- **The web shell** serves a copy of `images/` from its page, `crates/proteus-shell-web/www/`,
  which `make web-images` makes, and which `make build-web` and the deploys make for you. The
  copy isn't committed.
- **Videos on the web** play as HLS, from the segmented streams committed in
  `crates/proteus-shell-web/www/videos/hls/`. `scripts/make-hls.sh` makes them from the demo's
  `.mp4` files, with `ffmpeg`; run it again, and commit its output, after replacing a video.

To use your own assets, put them at the same paths. `DemoApp::asset_keys()` in
`crates/proteus-demo/src/app.rs` lists every image the demo loads, and the web shell downloads
exactly that list before the demo starts. The videos are listed in each shell:
`TILE_VIDEO_PATHS` in `crates/proteus-shell-native/src/main.rs`, and `tile_streams()` in
`crates/proteus-shell-web/src/lib.rs`. Without its assets, the demo still runs, with plain
placeholders and no video.

### Running the demo

Natively as release:

```bash
cargo run --release -p proteus-shell-native
```

Natively as debug, Cargo's default without `--release`:

```bash
cargo run -p proteus-shell-native
```

In a browser, the page must be served over HTTP, since it fetches its WebAssembly and assets:

```bash
make serve-web
```

This builds the WebAssembly bundle with `wasm-pack` and serves `crates/proteus-shell-web/www/`
on <http://localhost:8080>. `make build-web` builds it without serving.

In a browser as debug:

```bash
make serve-web-debug
```

The debug build runs debug assertions and overflow checks, names Rust functions in the stack
traces of panics, and carries debug information: with Chrome's "C/C++ DevTools Support (DWARF)"
extension, DevTools can set breakpoints in the Rust source and step through it. It's much larger
and slower than the release build, so animation in it isn't a guide to performance.
`make build-web-debug` builds it without serving, and `make example-gallery-web-debug` builds
and serves a Rust example the same way.

### The TypeScript SDK and examples

```bash
make build-sdk-web
```

builds `proteus-sdk-web` and `proteus-host-web` to WebAssembly (into `ts/pkg` and
`ts/pkg-host`), then compiles the TypeScript package into `crates/proteus-sdk-web/ts/dist/`. The
examples depend on it through a `file:` link, so build it first. Then, in an example's
TypeScript directory, such as `examples/gallery/typescript`:

```bash
npm install
npm run dev
```

`make example-gallery-ts`, from the root, does all of this, and rebuilds the SDK only when its
sources have changed.

### Running the tests

```bash
make test
```

runs every Rust test in the workspace, including the guides' code, which the `proteus-docs`
crate runs as doctests, and a check that every link between the guides resolves. The two web
crates only build for WebAssembly, so `make clippy` checks them instead.

No tests run in WebAssembly, locally or on CI. So code in the web crates that can be tested
anywhere lives in a crate that builds natively, as the web player's HLS manifest parser lives in
`proteus-demo`, and the web crates hold only what needs a browser.

Some tests draw with the GPU: rendering in `proteus-render`, baking in `proteus-ui`, and textures
in `proteus-sdk`. On a machine without a GPU, each reports itself skipped, by name
(`skipped: <test>: no GPU adapter`), so the tests pass anywhere. Set `REQUIRE_GPU=1` to make a missing GPU fail them instead, as CI does. On Linux
without a GPU, Mesa's software Vulkan driver gives them one, as on CI:

```bash
sudo apt-get install libvulkan1 mesa-vulkan-drivers
REQUIRE_GPU=1 make test
```

The TypeScript SDK has its own checks, after `make build-sdk-web`, in
`crates/proteus-sdk-web/ts`: `npx tsc --noEmit` type-checks it, `npm run check-docs` checks that
every export is documented, and `npm run check-snippets` type-checks the guides' TypeScript.

### Checks

```bash
make check
```

runs the formatting, lint, docs and test checks that CI runs: `make fmt`, `make clippy`, for
the native and WebAssembly targets, `make docs`, with warnings as errors, and `make test`.

```bash
make install-hooks
```

installs a pre-push hook that runs the same checks except the tests before every push, and
`actionlint` on the GitHub workflows when it's installed. Run it again when the hook changes.

CI also checks code comments against this guide (`make check-comments`), the TypeScript SDK and
its snippets, builds each example, and lints the workflows.

## Writing comments and docs

### Who you're writing for

- **Doc comments** (`///`, `//!`, TSDoc) are for someone using the API who has never seen this
  codebase.
- **Plain comments** (`//`) are for the next person changing the code.

### Doc comments

1. **The first line is one sentence saying what the item does.** It is shown on its own in
   lists and search results. Functions start with a verb ("Hides…", "Returns…"); types start
   with a noun phrase.
2. **Add more only when a caller needs it:** what the item guarantees, its defaults, and the
   edge cases someone would otherwise get wrong.
3. **Use the standard sections.** `# Errors` on every function that returns `Result`,
   `# Panics` on anything that can panic, and `# Examples` on the main entry points (crate
   roots and the key types). Examples must compile; they run as doctests.
4. **Link the items you mention**, as in ``[`Handle::set_visible`]``. A rename then breaks the
   docs build instead of leaving the text quietly wrong.
5. **Say why only when the code can't show it:** a non-obvious constraint, a surprising
   default, or a trade-off the caller must know about. One or two sentences.
6. **Keep it short.** A doc that needs more than about ten lines probably belongs in a guide in
   `docs/`.
7. **Write plain, natural sentences**, the way you would explain the item to a colleague. Use
   the API's own terms, and don't introduce things it doesn't have: the pointer is "pressed",
   not "a button went down".

### Plain comments

- Explain **why**, never what. Don't narrate code that already reads clearly.
- Good uses: ordering constraints ("must run after the visibility pass because…"),
  invariants, and workarounds for a specific external bug, with a link to it.
- A `TODO` says what is missing and links a GitHub issue.

### Tests

- Use `//` comments, never `///`.
- The test name says what behaviour is locked in. A comment above it says why that matters
  when it isn't obvious, such as the bug the test guards against.
- If a test pins a deliberate quirk, such as a one-tick delay, say so, or someone will "fix"
  it.

### Never in a comment

- Milestone numbers, audit IDs, step numbers or dates.
- References to external documents. They go stale, and the comment becomes inaccurate with
  them.
- History: "was X", "used to", "previously", "no longer", "originally". Git keeps history.
- Deliberation: "we decided", "per this pass's design", "matches X's existing behaviour", or
  options that were considered and rejected.

A placeholder may say plainly what it is reserved for, once: "Not read yet; reserved for
keyboard navigation."

`scripts/check-comments.sh` enforces the first two rules and the terminology below; review
catches the rest.

### TypeScript

- Every export gets TSDoc. Use `@param` when a parameter's name doesn't make its meaning
  obvious, and `@returns` / `@throws` where they apply. `mount`, `ProteusApp`, `component`
  and `transitionChannel` each carry an `@example`.
- Write for someone who only knows TypeScript. Don't send them to Rust items ("see
  `proteus_ui::SplitStrategy`"); explain the behaviour where they are reading.

### Guides in `docs/`

- Rust is the first-class language: each page shows Rust first, then TypeScript, explaining
  the concept once. Each example sits under a heading that names its language, `#### Rust` or
  `#### TypeScript`, or a variant such as `#### Rust, web host`.
- Every code snippet is checked in CI. Rust snippets run as doctests through the
  `proteus-docs` crate (`cargo test -p proteus-docs`); add each new page to `pages!` in its
  `lib.rs`, or a test fails. TypeScript snippets are type-checked against the SDK
  (`npm run check-snippets` in `crates/proteus-sdk-web/ts`, after `make build-sdk-web`). A
  Rust snippet that uses `proteus_host_web` in its code, not just a comment, only builds for
  wasm32, so it is checked by the wasm32 clippy run instead.
- Label every code block. rustdoc tests a block with no language as Rust, so a shell command
  is `bash`, output is `text`, and TypeScript is `ts`. A TypeScript block that can't be checked
  against the SDK alone, such as a Vite config, is `ts no-check`.
- Keep snippets short. A Rust snippet hides its setup behind `# ` lines, and is `no_run` if it
  opens a window. A TypeScript snippet can use the names that
  `scripts/ts-snippet-prelude.d.ts` declares, such as `app`, `button` and `items`, without
  declaring them.
- Link to the API reference for details rather than repeating them, and never to
  `PLANNING.md`. `cargo test -p proteus-docs` also checks that every link between pages, and
  every heading a link names, exists.

### Terminology

| Use | Meaning | Instead of |
|---|---|---|
| **component** | A UI element created with `component()`. For the bevy_ecs meaning, say **ECS component**. | using both meanings unmarked |
| **transition** | The animated change from one geometry to another | "morph" |
| **1→1, 1→N (split), N→1 (merge)** | The three shapes a transition can take | "topology" in user-facing docs |
| **declared geometry** | The geometry a component returns to when idle, as set by `ComponentSpec::new` or `Handle::set_declared_geometry` | "rest state", "rest geometry", "declared state" |
| **tick** | One run of `Proteus::tick`, the update step | "frame" when a tick is meant |
| **frame** | One rendered frame. Each frame runs one tick. | |
| **host** | The crate that owns the window or canvas, the GPU surface, the loop and input: `proteus-host-winit`, `proteus-host-web` | "shell" |
| **shell** | Only the two thin entry crates, `proteus-shell-native` and `proteus-shell-web` | |
| **app** | Anything that implements `App` | |
| **bake** | Render into an atlas texture. Always say what: bake text, bake an image, or bake a component (flatten it and its children into one texture, permanently). | a bare "bake" |
| **virtual** | A temporary component a split or merge creates, and removes when it completes | |
| **all the other**, **any other** | Everything except the item just mentioned | "every other", which can also mean alternate items |

## Pull requests

1. Branch from `main`, and commit as described in [Commit messages](#commit-messages).
2. Run `make check`, and the TypeScript checks if you changed the SDK.
3. Open a pull request into `main`. CI runs on it, and a change under `crates/` also gets a
   preview of the web demo, linked in a comment; see [RELEASING.md](./RELEASING.md).
4. Update the guides and API docs with the code they describe. A change to the public API that
   breaks existing code is marked as breaking in its commit.
5. Pull requests are merged with a merge commit, not squashed, so each commit's message reaches
   the changelog. A maintainer can also merge a branch locally and push `main`, with
   `git merge --no-ff`, which makes the same merge commit.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org). The changelog is
generated from them.

```
type(scope): subject

Body: what changed and why, wrapped at 72 columns.
```

- The subject is imperative, lowercase after the colon, has no trailing period, and is at most
  72 characters.
- **Types:** `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `style`, `build`, `ci`,
  `chore`. A breaking change adds `!` after the type and a `BREAKING CHANGE:` footer.
- **The subject is the changelog's line for the change,** so write it for someone who uses
  Proteus. `test` and `style` commits, `docs(planning)` and the release commit
  (`chore(release)`) are left out of it; see `cliff.toml`.
- **Scopes:** the crate name without `proteus-` (`sdk`, `sdk-web`, `runtime`, `ui`, `render`,
  `gpu`, `host-winit`, `host-web`, `demo`, `shell`), or `examples`. For `docs:` commits, the
  document (`readme`, `planning`, `roadmap`, `guide`, `contributing`).
