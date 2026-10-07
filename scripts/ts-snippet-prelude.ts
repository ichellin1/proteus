// Prepended to every TypeScript snippet in docs/ before it is type-checked
// (see check-ts-snippets.mjs), so a snippet can use these names without
// declaring them, as a Rust snippet hides its setup behind `# ` lines. A
// snippet mustn't declare one of these names again at its top level.
declare const app: import("proteus-sdk").ProteusApp;
declare const button: import("proteus-sdk").Handle;
declare const list: import("proteus-sdk").Handle;
declare const panel: import("proteus-sdk").Handle;
declare const tile: import("proteus-sdk").Handle;
declare const channel: import("proteus-sdk").TransitionChannel;
