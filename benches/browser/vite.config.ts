import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// The wasm plugins: see `examples/gallery/typescript/vite.config.ts` for why both are
// needed, and why their versions are pinned.
export default defineConfig({
  // Relative, so the page works wherever it's served: at /proteus/bench/ on GitHub Pages, and
  // under each pull request's path in staging.
  base: "./",
  plugins: [wasm(), topLevelAwait()],
});
