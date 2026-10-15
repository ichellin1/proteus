<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/ichellin1/proteus/main/brand/logo/assets/lockup-color-dark.png">
    <img alt="Proteus" src="https://raw.githubusercontent.com/ichellin1/proteus/main/brand/logo/assets/lockup-color-light.png" width="210">
  </picture>
</p>

# proteus-sdk

The TypeScript SDK for [Proteus](https://github.com/ichellin1/proteus), a UI framework whose components transform into one
another instead of loading new screens: a button can become the panel it opens. It draws on a
`<canvas>` with WebGPU, or WebGL2 where WebGPU isn't available. Its core is written in Rust and
compiled to WebAssembly.

```bash
npm install proteus-sdk
npm install --save-dev vite-plugin-wasm vite-plugin-top-level-await@1.5.0 @swc/core@1.10.16
```

With Vite, add both plugins to `vite.config.ts`: `plugins: [wasm(), topLevelAwait()]`. Then, with
a `<canvas id="app">` on the page, a button that turns into a panel when it's clicked, and back
when the panel is clicked:

```ts
import { colorFrom, mount, type Geometry } from "proteus-sdk";

function centered(width: number, height: number, cornerRadius: number): Geometry {
  return {
    position: { x: 0, y: 0, z: 0 },
    size: { width, height },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: colorFrom("#7359cc"),
    cornerRadius,
  };
}

await mount("app", {
  setup(app) {
    const button = app.component({
      geometry: centered(200, 60, 30),
      text: { content: "Open", sizePx: 24 },
    });
    const panel = app.component({
      geometry: centered(480, 320, 16),
      text: { content: "Close", sizePx: 24 },
      visible: false,
    });
    const channel = app.transitionChannel();
    button.onClick(() => channel.set(panel, button, { duration: 0.4 }));
    panel.onClick(() => channel.set(button, panel, { duration: 0.4 }));
  },
});
```

## Learn more

- [Getting started in TypeScript](https://github.com/ichellin1/proteus/blob/main/docs/getting-started/typescript.md),
  step by step.
- [Guides](https://github.com/ichellin1/proteus/tree/main/docs): the concepts, and how-tos.
- [API reference](https://ichellin1.github.io/proteus/api/ts/).
- [Examples](https://github.com/ichellin1/proteus/tree/main/examples), and
  [the reference demo](https://ichellin1.github.io/proteus/), running in your browser.
- Proteus is written in Rust, and its [Rust crates](https://crates.io/crates/proteus-sdk) can be
  used directly.

## License

MIT or Apache-2.0, at your option.
