import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// `proteus-sdk`'s wasm output is built with `wasm-pack --target bundler`
// (see its own crate doc for why) — that target emits a plain ESM
// `import * as wasm from "./x_bg.wasm"`, which Vite doesn't resolve out of
// the box. `vite-plugin-wasm` teaches Vite that import shape;
// `vite-plugin-top-level-await` is required alongside it because the
// generated glue (`wasm.__wbindgen_start()` at module scope) uses top-level
// await once instantiated as an ES module.
//
// `vite-plugin-top-level-await` and `@swc/core` are pinned to exact versions
// in package.json — `vite-plugin-top-level-await@^1.6.0` resolves a
// `@swc/core` whose AST serialization no longer matches what the plugin's
// 1.5.0-era code expects ("missing field `type`" during `vite build`, dev
// server unaffected). `1.5.0` + `@swc/core@1.10.16` is the last combination
// confirmed to build cleanly.
export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
});
