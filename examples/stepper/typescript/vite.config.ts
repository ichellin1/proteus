import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// The wasm plugins: see `examples/gallery/typescript/vite.config.ts` for why both are
// needed, and why their versions are pinned.
export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
});
