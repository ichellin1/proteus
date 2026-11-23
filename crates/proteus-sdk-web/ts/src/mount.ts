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
   * A misspelled field, an unknown value, a font that can't be read, or a
   * value the device can't support, such as an atlas larger than WebGL2
   * allows, throws an error that names the field.
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
 *     const open = app.transitionChannel();
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
  const config = opts.config ?? {};
  // The built-in font, Inter, isn't in the wasm: it's a file of this package,
  // fetched only when the app gives no default font of its own.
  const font = config.text?.font ?? (await builtInFont());
  await wasmMount(
    canvasId,
    (rawApp: WasmApp) => opts.setup(new ProteusApp(rawApp)),
    opts.update ? (dt: number) => opts.update?.(dt) : undefined,
    { ...config, text: { ...config.text, font } },
  );
}

/**
 * Fetches the built-in font, Inter, from this package's `fonts/`, which a
 * bundler such as Vite copies to the app's output, as it does an image the
 * app's own code refers to with `new URL(…, import.meta.url)`.
 *
 * @throws if the font can't be fetched.
 */
async function builtInFont(): Promise<Uint8Array> {
  const url = new URL("../fonts/Inter-Variable.ttf", import.meta.url);
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(
      `proteus-sdk: the built-in font couldn't be fetched from ${url} (${response.status}). ` +
        "Serve the package's fonts/ folder with the app, or set config.text.font.",
    );
  }
  return new Uint8Array(await response.arrayBuffer());
}
