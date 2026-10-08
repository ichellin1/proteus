# Examples

Complete apps, each showing a use case or a transition pattern. Where an example is in both
languages, the two versions are the same app: Rust in `rust/`, TypeScript in `typescript/`.

| Example | Rust | TypeScript | Shows |
|---|---|---|---|
| [Gallery](./gallery) | — | ✓ | A grid of downloaded photos; a photo opens into a large view (1→1), which splits back into a new grid (1→N). |
| [Video](./video) | — | ✓ | Video from the platform's own player, shown on a component. |

The reference demo, in `crates/proteus-demo`, is a larger app that shows every transition
pattern together.

## What you need

The examples run from this repository, so they build Proteus from source.

**For the Rust examples,** Rust 1.89 or later. Check with `cargo --version`. If you don't have
it, install it with `rustup`, Rust's installer. On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows:

```bash
winget install Rustlang.Rustup
```

Then open a new terminal, so that `cargo` is on your `PATH`.

**For the TypeScript examples, and the Rust examples in a browser,** also the WebAssembly target
and `wasm-pack`, which build Proteus for the web:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**For the TypeScript examples,** Node.js 20 or later. Check with `node --version`. If you don't
have it, install it on macOS with Homebrew:

```bash
brew install node
```

on Windows:

```bash
winget install OpenJS.NodeJS.LTS
```

or on Linux with your distribution's package manager, for example on Ubuntu:

```bash
sudo apt install nodejs npm
```

## Running an example

Each example's README has its commands. From the repository's root:

- **A TypeScript example:** `make example-gallery-ts`. The first run builds the TypeScript SDK
  from source, which takes a few minutes; later runs rebuild it only when Proteus's code has
  changed. Then open the address Vite prints, usually <http://localhost:5173>.
