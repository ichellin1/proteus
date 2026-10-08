# Getting started in TypeScript

This guide builds the same small app as [the Rust guide](./rust.md): a button that transforms
into a panel when it's clicked, and back again when the panel is clicked. It runs in a web page,
drawn on a `<canvas>` with WebGPU, or WebGL2 where WebGPU isn't available.

The TypeScript SDK, the `proteus-sdk` npm package, is Proteus's Rust core compiled to
WebAssembly, with a TypeScript API over it.

You need Node.js 20 or later. Check with `node --version`. If you don't have it, install it on
macOS with Homebrew:

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

## Create the project

Start from a Vite project, and add Proteus and the two Vite plugins that load its WebAssembly:

```bash
npm create vite@latest hello-proteus -- --template vanilla-ts
cd hello-proteus
npm install proteus-sdk
npm install --save-dev vite-plugin-wasm vite-plugin-top-level-await@1.5.0 @swc/core@1.10.16
```

The last two are pinned because newer versions of `vite-plugin-top-level-await` fail in
`vite build`.

Create `vite.config.ts`:

```ts no-check
import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
});
```

Replace the body of `index.html` with a canvas that fills the window:

```html
<body style="margin: 0">
  <canvas id="app" style="display: block; width: 100vw; height: 100vh"></canvas>
  <script type="module" src="/src/main.ts"></script>
</body>
```

## Write the app

Replace `src/main.ts` with:

```ts
import { colorFrom, mount, type Geometry } from "proteus-sdk";

// A shape at the center of the canvas.
function centered(width: number, height: number, cornerRadius: number): Geometry {
  return {
    position: { x: 0, y: 0, z: 0 },
    size: { x: width, y: height },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: colorFrom("#7359cc"),
    cornerRadius,
  };
}

await mount("app", {
  setup(app) {
    // A pill-shaped button in the middle of the canvas.
    const button = app.component({
      geometry: centered(200, 60, 30),
      text: { content: "Open", sizePx: 24 },
    });

    // The panel the button becomes. It starts hidden; a transition into it
    // shows it.
    const panel = app.component({
      geometry: centered(480, 320, 16),
      text: { content: "Close", sizePx: 24 },
      visible: false,
    });

    // A channel transitions one component into another.
    const channel = app.transitionChannel();
    button.onClick(() => channel.set(panel, button, { duration: 0.4 }));
    panel.onClick(() => channel.set(button, panel, { duration: 0.4 }));
  },
});
```

Run it:

```bash
npm run dev
```

and open the address Vite prints. Click the button: it grows into the panel, changing shape on
the way. Click the panel to turn it back into the button.

## What's going on

**Components** are what you see: each is a shape, with a position, size, color and corner
radius, that can carry text or an image. `app.component` creates one and returns a `Handle`,
which you use to change it later. Positions are in pixels, measured from the center of the
canvas, with y pointing up; `topLeftToWorld` converts from the usual top-left coordinates.

**A transition channel** turns one component into another: `channel.set(to, from, config)` hides
`from` and moves `to` from `from`'s shape to its own, over `duration` seconds. Neither component
knows about the other; the channel holds the relationship.

**Callbacks** react to input. `onClick` is called each time the component is clicked.

**`mount`** starts Proteus on the canvas with the given `id`, and calls `setup` once to create
the components. An optional `update` runs every frame. Proteus draws every frame from then on.

## Next

The [guides](../README.md) cover each part in depth, and the
[API reference](https://ichellin1.github.io/proteus/api/ts/) lists everything `proteus-sdk` has.
