# Gallery — a `proteus-sdk` example

A `.ts`-only image gallery built on the published `proteus-sdk` npm package — no direct
dependency on any Rust crate. It demonstrates two of Proteus's three transition topologies
against real, network-fetched photos (picsum.photos):

- grid tile → detail hero: **1→1** (`SignalHandle.set`)
- detail hero → fresh grid: **1→N** (`Handle.splitTo`, "back to grid")

See [`src/main.ts`](./src/main.ts)'s own top comment for the full design notes, and
[PLANNING.md](../../PLANNING.md)'s M13.8 section for how this example was built and what it
found along the way (a framework-level draw-order bug and a callback re-entrancy panic, both
fixed as part of this milestone).

## Install dependencies

- **Node.js** (v20+) and npm.
- **Rust toolchain** (via [rustup](https://rustup.rs/)) + **wasm-pack**
  (`cargo install wasm-pack`) — needed once, to build the `proteus-sdk` package this example
  depends on. Not needed again after that unless you change Rust code under
  `crates/proteus-sdk-web` or `crates/proteus-host-web`.

This example depends on `proteus-sdk` via a `file:` link
(`crates/proteus-sdk-web/ts`, see [`package.json`](./package.json)) rather than a published
npm version, so that package has to be built locally first.

## Build the SDK

From the repo root:

```bash
make build-sdk-web
```

This builds `proteus-sdk-web` and `proteus-host-web` to wasm (`wasm-pack --target bundler`)
and compiles the TypeScript wrapper's `dist/` — everything `examples/gallery` imports from
`proteus-sdk`. Equivalent to what `.github/workflows/ci.yml`'s `sdk-web` job runs before
building this example.

## Run the example

```bash
cd examples/gallery
npm install
npm run dev
```

Then open the printed local URL (Vite's default is `http://localhost:5173`) in a browser.
Any browser with WebGL2 works; Proteus prefers WebGPU where available.

## Build for production

```bash
npm run build
```

Type-checks (`tsc --noEmit`) then runs `vite build`, emitting static output to `dist/`.
`npm run preview` serves that build locally to sanity-check it.

## Troubleshooting

- **Vite can't resolve the wasm import / build fails with an odd `@swc/core` error** — see
  [`vite.config.ts`](./vite.config.ts)'s own comment; `vite-plugin-top-level-await` and
  `@swc/core` are pinned to specific versions for a reason.
- **Changed Rust code under `proteus-sdk-web`/`proteus-host-web` but the example doesn't pick
  it up** — rerun `make build-sdk-web`, then restart `npm run dev` (Vite's dev-dependency
  cache can hold on to a stale wasm module across a rebuild; if a restart alone doesn't clear
  it, delete `node_modules/.vite` in this directory and restart again).
