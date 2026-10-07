import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

// The wasm plugins: see `examples/gallery/vite.config.ts` for why both are
// needed, and why their versions are pinned.
export default defineConfig({
  plugins: [wasm(), topLevelAwait()],
  // Serves the native demo's sample videos, so this example needs no copy of
  // its own: `/tiger.mp4` and so on.
  publicDir: "../../crates/proteus-shell-native/assets/videos",
});
