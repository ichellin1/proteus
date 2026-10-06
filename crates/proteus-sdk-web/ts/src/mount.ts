// `mount`, which runs a TypeScript app on a canvas using `proteus-host-web`.
//
// The host is a separately compiled wasm module, and the app object it passes
// to `setup` comes from that module, not from this package's own. Wrapping it
// in this package's `ProteusApp` is safe: both modules compile the same
// `ProteusApp` bindings, and each method call stays within the module that
// created the object.

import { mount as wasmMount } from "../pkg-host/proteus_host_web.js";
import type { ProteusApp as WasmApp } from "../pkg/proteus_sdk_web.js";

import { ProteusApp } from "./index.js";
import type { ProteusConfigOverrides } from "./types.js";

/** Options for {@link mount}. */
export interface MountOptions {
  /** Called once, before the first frame, to create the app's components. */
  setup: (app: ProteusApp) => void;
  /**
   * Called every frame, after the app has been ticked, with the time since
   * the previous frame in seconds. To use the app here, keep the one passed
   * to `setup`.
   *
   * Optional, because many apps need no per-frame code: an app that reacts
   * only through callbacks registered in `setup`, such as `onClick` and
   * `onTransitionComplete`, can leave it out. Use it for work that runs
   * every frame, such as a countdown or an animation of your own.
   */
  update?: (deltaSeconds: number) => void;
  /**
   * Engine settings. Omit it, or any field, to keep the web default.
   *
   * ```ts
   * await mount("canvas", {
   *   setup,
   *   config: {
   *     render: { clearColor: [0.05, 0.05, 0.08, 1] },
   *     resources: { imageMaxSide: 512 },
   *   },
   * });
   * ```
   *
   * A misspelled field, an unknown value, or a value the device can't
   * support, such as an atlas larger than WebGL2 allows, throws an error that
   * names the field.
   */
  config?: ProteusConfigOverrides;
}

/**
 * Runs an app on the `<canvas>` element with the given id: creates a
 * {@link ProteusApp}, calls `setup` with it, then draws and ticks it every
 * frame and reports pointer input to it.
 *
 * @example
 * ```ts
 * await mount("canvas", {
 *   setup(app) {
 *     const button = app.component({ geometry: buttonGeometry });
 *     const panel = app.component({ geometry: panelGeometry, visible: false });
 *     const open = app.signal();
 *     button.onClick(() => open.set(panel, button, { duration: 0.4 }));
 *   },
 * });
 * ```
 *
 * @throws if the canvas isn't found, the GPU can't be initialized, or
 * `config` is invalid.
 */
export async function mount(
  canvasId: string,
  opts: MountOptions,
): Promise<void> {
  await wasmMount(
    canvasId,
    (rawApp: WasmApp) => opts.setup(new ProteusApp(rawApp)),
    opts.update ? (dt: number) => opts.update?.(dt) : undefined,
    opts.config ?? null,
  );
}
