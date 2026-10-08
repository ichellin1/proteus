// Global declarations for the TypeScript snippets in docs/, type-checked by
// check-ts-snippets.mjs: a snippet can use these names without declaring
// them, as a Rust snippet hides its setup behind `# ` lines. A snippet can
// still declare a name of its own; it shadows the one here.
declare const app: import("proteus-sdk").ProteusApp;
declare const button: import("proteus-sdk").Handle;
declare const list: import("proteus-sdk").Handle;
declare const panel: import("proteus-sdk").Handle;
declare const tile: import("proteus-sdk").Handle;
declare const card: import("proteus-sdk").Handle;
declare const caption: import("proteus-sdk").Handle;
declare const items: import("proteus-sdk").Handle[];
declare const channel: import("proteus-sdk").TransitionChannel;
