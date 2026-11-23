import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// These are the wasm plugins. `examples/gallery/typescript/vite.config.ts` explains
// why both are needed, and why their versions are pinned.
export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  // The fonts are in the example's own folder, one level up from this one. The
  // Rust version shares them.
  server: { fs: { allow: [".."] } },
});
