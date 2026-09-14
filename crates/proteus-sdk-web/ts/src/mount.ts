/**
 * `mount` — the TS → web front door, backed by `proteus-host-web` (M13.2).
 *
 * Wraps `proteus-host-web`'s raw wasm-bindgen `mount(canvasId, setup,
 * update)` export, converting the raw app object it hands `setup` into
 * this package's ergonomic {@link ProteusApp} before calling the caller's
 * own `setup`. That raw object is minted by `proteus-host-web`'s own,
 * separately-compiled wasm binary — not this package's `pkg/
 * proteus_sdk_web.js` — which is safe to wrap directly rather than a design
 * problem: both binaries compile the identical `#[wasm_bindgen] impl
 * ProteusApp` block (confirmed structurally identical generated `.d.ts`,
 * and each generated method call binds to its own module's wasm instance,
 * so nothing here ever crosses between the two). See `PLANNING.md`'s
 * M13.2 section for the full investigation.
 */

import { mount as wasmMount } from "../pkg-host/proteus_host_web.js";
import type { ProteusApp as WasmApp } from "../pkg/proteus_sdk_web.js";

import { ProteusApp } from "./index.js";

export interface MountOptions {
  /** Called once, synchronously, before the first frame is queued. */
  setup: (app: ProteusApp) => void;
  /**
   * Called every frame after `setup`, if provided. `app` isn't passed
   * again here — capture it from `setup`'s own closure if `update` needs
   * it (mirrors the raw wasm export's own doc: avoids reconstructing a
   * fresh {@link ProteusApp} wrapper every frame for no reason).
   */
  update?: (deltaSeconds: number) => void;
}

/** Mount a TS-authored app on the `<canvas>` element with the given id. */
export async function mount(
  canvasId: string,
  opts: MountOptions,
): Promise<void> {
  await wasmMount(
    canvasId,
    (rawApp: WasmApp) => opts.setup(new ProteusApp(rawApp)),
    opts.update ? (dt: number) => opts.update?.(dt) : undefined,
  );
}
