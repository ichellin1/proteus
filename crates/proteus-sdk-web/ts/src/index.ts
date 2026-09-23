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
  ChildBehavior,
  ComponentData,
  ComponentSpec,
  Geometry,
  MergeLayout,
  SplitStrategy,
  TextureState,
  TransitioningConfig,
  TransitionConfig,
  TransitionDropped,
  Vec2,
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

/**
 * A handle to one component.
 *
 * ## Using a handle after its component is destroyed
 *
 * Every mutating method below **throws** if this handle's component is no
 * longer alive — destroyed directly, despawned as a descendant of a destroyed
 * parent, or reconstructed via {@link ProteusApp.handleFromId} from a stale
 * id. Read-only accessors don't throw: {@link Handle.get} returns `undefined`,
 * and {@link Handle.bakedTextSize}/{@link Handle.bakedImageSize} do too.
 *
 * These used to *panic* the wasm module instead, which freezes the canvas with
 * no recovery short of a page reload. A throw is catchable and tells you which
 * call went wrong; the Rust side logs a warning alongside it.
 *
 * A method returning `boolean` uses it for "there was nothing to do" — an image
 * that hasn't finished baking, a texture that's been evicted — which is a
 * routine state, not an error, and never throws.
 */
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

  /**
   * Fires each time a transition targeting this component finishes.
   *
   * Which component that is, per topology:
   *
   * - {@link Handle.animateTo} — this component.
   * - {@link SignalHandle.set} — the `to` side.
   * - {@link Handle.splitTo} with `"slice"` or `"gridSlice"` — the
   *   **source**, once, when every target has arrived. One group is one
   *   completion, not one per target.
   * - {@link Handle.mergeFrom} — the **destination**, once, when every
   *   source has arrived.
   * - {@link Handle.splitTo} with `"bake"` — the **targets**, each
   *   independently. *Not* the source: `bake` is N independent 1-to-1
   *   transitions with no virtual entities, so the source has nothing of
   *   its own to finish. Listen on the targets, or use `"slice"` if you
   *   want one completion for the group.
   *
   * Persistent — it keeps firing for later transitions until the component
   * is destroyed. Replaces guessing with `setTimeout(duration)`.
   */
  onTransitionComplete(cb: PlainCallback): void {
    this.app.wasmApp.onTransitionComplete(this.wasmHandle, cb);
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

  /**
   * Split this component into `targets` — a 1→N group transition. Each
   * target's geometry is resolved automatically from its own
   * `component()`-declared rest state, mirroring {@link SignalHandle.set}.
   * This component is hidden once the transition completes — no separate
   * visibility call needed.
   */
  splitTo(
    targets: Handle[],
    config: TransitionConfig,
    strategy: SplitStrategy,
    childBehavior?: ChildBehavior,
  ): void {
    const ids = new Float64Array(targets.map((t) => t.id()));
    if (childBehavior) {
      this.app.wasmApp.splitToWithBehavior(
        this.wasmHandle,
        ids,
        config,
        strategy,
        childBehavior,
      );
    } else {
      this.app.wasmApp.splitTo(this.wasmHandle, ids, config, strategy);
    }
  }

  /**
   * Merge `sources` into this component — an N→1 group transition.
   * `sources` are hidden immediately — "the morph is the exit", the same
   * convention 1→1 signals already use.
   */
  mergeFrom(
    sources: Handle[],
    config: TransitionConfig,
    layout: MergeLayout,
    childBehavior?: ChildBehavior,
  ): void {
    const ids = new Float64Array(sources.map((s) => s.id()));
    if (childBehavior) {
      this.app.wasmApp.mergeFromWithBehavior(
        this.wasmHandle,
        ids,
        config,
        layout,
        childBehavior,
      );
    } else {
      this.app.wasmApp.mergeFrom(this.wasmHandle, ids, config, layout);
    }
  }

  /**
   * Destroys this component and (via `bevy_ecs`'s `ChildOf`/`Children`
   * relationship) every descendant.
   *
   * @throws if this handle's component was already destroyed. Harmless to
   * ignore, but reported so a double-destroy doesn't pass for a successful one.
   */
  destroy(): void {
    this.app.wasmApp.destroy(this.wasmHandle);
  }

  freeResources(): void {
    this.app.wasmApp.freeResources(this.wasmHandle);
  }

  /**
   * {@link splitTo}, but with each target's rest geometry given explicitly
   * instead of resolved from its own declared/live state — needed whenever
   * the natural declared geometry would be wrong, or (when this component is
   * also one of `targets`) unsafe to derive automatically.
   */
  splitToWithStates(
    targets: { handle: Handle; state: Geometry }[],
    config: TransitionConfig,
    strategy: SplitStrategy,
  ): void {
    this.app.wasmApp.splitToWithStates(
      this.wasmHandle,
      targets.map((t) => ({ id: t.handle.id(), state: t.state })),
      config,
      strategy,
    );
  }

  /**
   * Overwrites both this component's live geometry and its declared rest
   * state — {@link splitTo}/{@link mergeFrom} resolve a target's rest state
   * from the declared value, not the live one, so plain geometry mutation
   * elsewhere won't update what a future group transition resolves to. Use
   * whenever a component's real resting layout is only known after spawn —
   * e.g. sized from its own {@link bakedTextSize}/{@link bakedImageSize}.
   */
  setDeclaredGeometry(state: Geometry): void {
    this.app.wasmApp.setDeclaredGeometry(this.wasmHandle, state);
  }

  /**
   * Ad-hoc 1→1 morph to `to`, starting from this component's current live
   * geometry — no signal or second entity involved, unlike
   * {@link SignalHandle.set}. Useful for repeatedly re-targeting the same
   * entity to a fresh destination with nothing else to resolve against.
   */
  animateTo(to: Geometry, config: TransitionConfig): void {
    this.app.wasmApp.animateTo(this.wasmHandle, to, config);
  }

  /**
   * The baked glyph run's pixel footprint, if this component's `text` has
   * finished baking — `undefined` before baking completes or if it was
   * never given one.
   */
  bakedTextSize(): Vec2 | undefined {
    return this.app.wasmApp.bakedTextSize(this.wasmHandle) as
      | Vec2
      | undefined;
  }

  /**
   * The baked image's pixel footprint, if this component's `image` has
   * finished baking — `undefined` before baking completes or if it was
   * never given one.
   */
  bakedImageSize(): Vec2 | undefined {
    return this.app.wasmApp.bakedImageSize(this.wasmHandle) as
      | Vec2
      | undefined;
  }

  /**
   * Copies whichever baked image `source` currently shows onto this
   * component — `false` (no-op) if `source` has no baked image yet. Useful
   * when one entity needs to immediately show what another already-baked
   * entity looks like, e.g. an "enlarged view" coordinator a group
   * transition is about to reveal.
   */
  copyBakedImageFrom(source: Handle): boolean {
    return this.app.wasmApp.copyBakedImageFrom(
      this.wasmHandle,
      source.wasmHandle,
    );
  }

  /**
   * Crops this component's current baked image to a centered square, in
   * place — `false` (no-op) if it has no baked image yet. Useful for square
   * display cells (e.g. a photo grid tile) fed from images of varying aspect
   * ratios — crop instead of stretch.
   */
  centerCropToSquare(): boolean {
    return this.app.wasmApp.centerCropToSquare(this.wasmHandle);
  }

  /**
   * Toggles this component's click/hover eligibility at runtime. Every
   * component is interactive by default unless {@link ComponentSpec.nonInteractive}
   * was set at spawn — this is the same toggle, applied later.
   */
  setInteractive(interactive: boolean): void {
    this.app.wasmApp.setInteractive(this.wasmHandle, interactive);
  }

  /**
   * Shows or hides this component. Hidden components stay in the world but
   * are skipped by render, input and navigation; children cascade.
   *
   * {@link SignalHandle.set} already hides its `from` and reveals its `to`,
   * so a signal-driven morph needs no call here. This is for visibility a
   * signal doesn't own.
   *
   * Rendering stops on the next frame; hit-testing stops one tick after
   * that. Input is resolved against what was last painted, so a click
   * arriving in the same tick as the hide still lands.
   */
  setVisible(visible: boolean): void {
    this.app.wasmApp.setVisible(this.wasmHandle, visible);
  }

  /**
   * Sets this component's alpha multiplier, clamped to `0.0`–`1.0`.
   *
   * Cascades down: a child's effective opacity is its own times its
   * parent's effective, so `0.6` over `0.6` paints at `0.36`. A child never
   * affects its parent.
   *
   * Unrelated to {@link Handle.setVisible} — opacity is a paint multiplier,
   * visibility is an ECS flag. A component at `0` opacity is invisible but
   * still hit-tests; a hidden one doesn't. Use visibility to take something
   * out of the UI, opacity to fade it.
   */
  setOpacity(opacity: number): void {
    this.app.wasmApp.setOpacity(this.wasmHandle, opacity);
  }

  /**
   * Disables or re-enables this component.
   *
   * A disabled component still renders and still cascades to its children;
   * it is excluded from hit-testing entirely — no hover/press/click/focus —
   * and wears whatever {@link ComponentSpec.disabled} style it declared, so
   * it can look dimmed rather than merely stop responding.
   *
   * Not the same as {@link Handle.setInteractive}, which removes the
   * component as a click target permanently and has no associated look.
   */
  setDisabled(disabled: boolean): void {
    this.app.wasmApp.setDisabled(this.wasmHandle, disabled);
  }

  /**
   * Sets whether this component receives input while mid-transition. Pass
   * `undefined` to remove the opt-in, restoring the default of no
   * interaction during a morph.
   */
  setTransitioningConfig(config: TransitioningConfig | undefined): void {
    this.app.wasmApp.setTransitioningConfig(this.wasmHandle, config ?? null);
  }

  /**
   * Shows an already-registered texture on this component, replacing
   * whatever image/text/composite it previously showed — `false` (no-op) if
   * `texture` is evicted/unknown. The sanctioned way to do frame-swap
   * animation off a pre-baked set.
   */
  setTexture(texture: TextureHandle): boolean {
    return this.app.wasmApp.setTexture(this.wasmHandle, texture.wasmHandle);
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
   *
   * The id isn't checked against the live world here — if it names a component
   * that has since been destroyed, the returned handle behaves like any other
   * stale one: mutating methods throw, {@link Handle.get} returns `undefined`.
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
