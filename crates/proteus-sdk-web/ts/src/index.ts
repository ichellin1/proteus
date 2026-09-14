/**
 * `proteus-sdk` — the TypeScript layer over `proteus-sdk-web`'s wasm
 * bridge. This is the primary developer-facing entry point (M12.4):
 * `Handle`/`SignalHandle`/`TextureHandle` restore the `button.onClick(cb)`
 * ergonomic the raw bridge deliberately doesn't have (its methods take
 * `(handle, cb)` on `ProteusApp` instead, mirroring `proteus-sdk`'s own
 * Rust shape 1:1 — see `../src/lib.rs`'s top doc) by having each handle
 * close over its owning `ProteusApp`.
 */

import {
  Handle as WasmHandle,
  ProteusApp as WasmApp,
  SignalHandle as WasmSignalHandle,
  TextureHandle as WasmTextureHandle,
} from "../pkg/proteus_sdk_web.js";

import type {
  ComponentData,
  ComponentSpec,
  TextureState,
  TransitionConfig,
  TransitionDropped,
} from "./types.js";

export * from "./types.js";
export * from "./convert.js";
export * from "./mount.js";

type PlainCallback = () => void;
type DragCallback = (delta: { x: number; y: number }) => void;
type DroppedCallback = (dropped: TransitionDropped) => void;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

export class Handle {
  /**
   * Prefer {@link ProteusApp.component} or {@link ProteusApp.handleFromId}
   * over calling this directly.
   */
  constructor(
    private readonly app: ProteusApp,
    /** @internal */
    public readonly wasmHandle: WasmHandle,
  ) {}

  id(): number {
    return this.wasmHandle.id();
  }

  get(): ComponentData | undefined {
    return this.app.get(this);
  }

  onClick(cb: PlainCallback): void {
    this.app.wasmApp.onClick(this.wasmHandle, cb);
  }

  onHoverEnter(cb: PlainCallback): void {
    this.app.wasmApp.onHoverEnter(this.wasmHandle, cb);
  }

  onHoverExit(cb: PlainCallback): void {
    this.app.wasmApp.onHoverExit(this.wasmHandle, cb);
  }

  onPress(cb: PlainCallback): void {
    this.app.wasmApp.onPress(this.wasmHandle, cb);
  }

  onRelease(cb: PlainCallback): void {
    this.app.wasmApp.onRelease(this.wasmHandle, cb);
  }

  onFocus(cb: PlainCallback): void {
    this.app.wasmApp.onFocus(this.wasmHandle, cb);
  }

  onBlur(cb: PlainCallback): void {
    this.app.wasmApp.onBlur(this.wasmHandle, cb);
  }

  onDrag(cb: DragCallback): void {
    this.app.wasmApp.onDrag(this.wasmHandle, (x: number, y: number) =>
      cb({ x, y }),
    );
  }

  addChild(child: Handle): void {
    this.app.wasmApp.addChild(this.wasmHandle, child.wasmHandle);
  }

  removeChild(child: Handle, destroy = false): void {
    this.app.wasmApp.removeChild(this.wasmHandle, child.wasmHandle, destroy);
  }

  destroy(): void {
    this.app.wasmApp.destroy(this.wasmHandle);
  }

  freeResources(): void {
    this.app.wasmApp.freeResources(this.wasmHandle);
  }
}

// ---------------------------------------------------------------------------
// SignalHandle
// ---------------------------------------------------------------------------

export class SignalHandle {
  /** Prefer {@link ProteusApp.signal} over calling this directly. */
  constructor(
    private readonly app: ProteusApp,
    /** @internal */
    public readonly wasmHandle: WasmSignalHandle,
  ) {}

  /**
   * Declare a transition: `to` should morph into its own declared geometry,
   * appearing to originate from `from`'s current geometry — mirrors
   * `signal.set([to, from], config)`.
   */
  set(
    to: Handle,
    from: Handle,
    config: TransitionConfig,
    interruptible = false,
  ): void {
    this.app.wasmApp.signalSet(
      this.wasmHandle,
      to.wasmHandle,
      from.wasmHandle,
      config,
      interruptible,
    );
  }

  /**
   * Persistent handler for requests on this signal that were declined
   * (already transitioning without `interruptible`, missing/invisible
   * entity) — fires on every drop, not just the first.
   */
  onDropped(cb: DroppedCallback): void {
    this.app.wasmApp.onDropped(this.wasmHandle, cb);
  }

  destroy(): void {
    this.app.wasmApp.signalDestroy(this.wasmHandle);
  }
}

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

export class TextureHandle {
  /** Prefer {@link ProteusApp.texture} over calling this directly. */
  constructor(
    private readonly app: ProteusApp,
    /** @internal */
    public readonly wasmHandle: WasmTextureHandle,
  ) {}

  id(): number {
    return this.wasmHandle.id();
  }

  /** `undefined` if this texture has been evicted or the id is unknown. */
  state(): TextureState | undefined {
    return this.app.wasmApp.textureState(this.wasmHandle) as
      | TextureState
      | undefined;
  }
}

// ---------------------------------------------------------------------------
// ProteusApp
// ---------------------------------------------------------------------------

export class ProteusApp {
  /** @internal */
  readonly wasmApp: WasmApp;

  /**
   * @param wasmApp Wrap an existing wasm-bindgen app instance instead of
   * constructing a fresh one — used by {@link mount} (see `mount.ts`),
   * which receives a `ProteusApp` minted by `proteus-host-web`'s own,
   * separately-compiled wasm binary rather than this package's own
   * `pkg/proteus_sdk_web.js`. That's safe to wrap directly: both binaries
   * compile the identical `#[wasm_bindgen] impl ProteusApp` block, so the
   * generated classes are structurally identical, and every generated
   * method call binds to its own module's wasm instance — nothing here
   * ever crosses between the two. Most callers should omit this and get a
   * fresh, standalone app.
   */
  constructor(wasmApp?: WasmApp) {
    this.wasmApp = wasmApp ?? new WasmApp();
  }

  component(spec: ComponentSpec): Handle {
    return new Handle(this, this.wasmApp.component(spec));
  }

  /**
   * Reconstruct a {@link Handle} from an id obtained from
   * {@link Handle.id} or a {@link ComponentData.children} entry.
   */
  handleFromId(id: number): Handle {
    return new Handle(this, WasmHandle.fromId(id));
  }

  signal(owner?: Handle): SignalHandle {
    // wasm-bindgen doesn't support Option<&CustomStruct> parameters, and
    // taking Handle by value would consume the caller's `owner` object —
    // see ../src/lib.rs's `signal()` doc — so the raw bridge takes an id.
    return new SignalHandle(this, this.wasmApp.signal(owner?.id()));
  }

  texture(id: number): TextureHandle {
    return new TextureHandle(this, this.wasmApp.texture(id));
  }

  /** Reconstruct a {@link TextureHandle} from an id obtained from {@link TextureHandle.id}. */
  textureFromId(id: number): TextureHandle {
    return new TextureHandle(this, WasmTextureHandle.fromId(id));
  }

  /** `undefined` if `handle` no longer refers to a live component. */
  get(handle: Handle): ComponentData | undefined {
    return this.wasmApp.get(handle.wasmHandle) as ComponentData | undefined;
  }

  /** Advances one frame. `deltaSeconds` is the elapsed wall-clock time. */
  tick(deltaSeconds: number): void {
    this.wasmApp.tick(deltaSeconds);
  }

  /**
   * `x`/`y` are **world-space** (viewport-center origin, Y-up) — convert
   * from screen/CSS coordinates with {@link topLeftToWorld} first if needed.
   */
  pointerMoved(x: number, y: number): void {
    this.wasmApp.pointerMoved(x, y);
  }

  pointerLeft(): void {
    this.wasmApp.pointerLeft();
  }

  pointerPressed(): void {
    this.wasmApp.pointerPressed();
  }

  pointerReleased(): void {
    this.wasmApp.pointerReleased();
  }

  /**
   * Wires `requestAnimationFrame` to call {@link tick} automatically,
   * converting the RAF timestamp (ms) to a delta in seconds. Returns a
   * function that stops the loop. Manual `tick()` calls remain available
   * for custom render-loop integration — this is purely a convenience
   * default, not the only way to drive the app.
   */
  startAnimationLoop(): () => void {
    let lastTime: number | null = null;
    let stopped = false;

    const step = (time: number) => {
      if (stopped) return;
      if (lastTime !== null) {
        this.tick((time - lastTime) / 1000);
      }
      lastTime = time;
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);

    return () => {
      stopped = true;
    };
  }
}
