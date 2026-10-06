/**
 * The Proteus TypeScript SDK: create components, connect them, and transition
 * between them.
 *
 * Start with {@link mount}, which runs an app on a `<canvas>`. Each handle
 * ({@link Handle}, {@link SignalHandle}, {@link TextureHandle}) keeps a
 * reference to the {@link ProteusApp} it came from, so its methods are called
 * directly: `button.onClick(() => ...)`.
 *
 * @packageDocumentation
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
  TextureRequest,
  TextureState,
  TransitioningConfig,
  TransitionConfig,
  TransitionDropped,
  Vec2,
} from "./types.js";

export * from "./types.js";
export * from "./convert.js";
export * from "./mount.js";

/** A callback with no arguments, such as {@link Handle.onClick}'s. */
export type PlainCallback = () => void;
/**
 * {@link Handle.onDrag}'s callback: how far the pointer moved since the last
 * frame, in world units (x right, y up).
 */
export type DragCallback = (delta: { x: number; y: number }) => void;
/** {@link SignalHandle.onDropped}'s callback: the request that couldn't run, and why. */
export type DroppedCallback = (dropped: TransitionDropped) => void;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/**
 * A handle to one component.
 *
 * Callbacks registered with the `on*` methods run every time their event
 * happens, until the component is destroyed. They run after the app's update
 * for that tick, so anything they start takes effect on the next tick.
 *
 * Once the component is destroyed, methods that change it throw, and the
 * failure is also logged. Methods that read it return `undefined`. A method
 * that returns `boolean` uses `false` for "there was nothing to do", such as
 * an image that hasn't been baked yet; that is not an error and doesn't throw.
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

  /** The component's ID, for {@link ProteusApp.handleFromId} and {@link ComponentSpec.children}. */
  id(): number {
    return this.wasmHandle.id();
  }

  /** A snapshot of the component's current state, or `undefined` if it has been destroyed. */
  get(): ComponentData | undefined {
    return this.app.get(this);
  }

  /** Calls `cb` each time the pointer is pressed on this component. A click also gives it focus. */
  onClick(cb: PlainCallback): void {
    this.app.wasmApp.onClick(this.wasmHandle, cb);
  }

  /**
   * Calls `cb` each time a transition finishes on this component.
   *
   * Every transition reports its completion once, on one component:
   *
   * - {@link Handle.animateTo}: this component.
   * - {@link SignalHandle.set}: the `to` component.
   * - {@link Handle.splitTo} or {@link Handle.splitToWithStates} with
   *   `"slice"` or `"gridSlice"`: the source, once every target has arrived.
   * - {@link Handle.splitTo} or {@link Handle.splitToWithStates} with
   *   `"perTarget"`: each target, separately. The source has no transition of
   *   its own; it is hidden as soon as the split starts.
   * - {@link Handle.mergeFrom}: the destination, once every source has
   *   arrived.
   *
   * Changes of interaction style, such as a hover effect, currently fire this
   * callback as well when their animation finishes.
   */
  onTransitionComplete(cb: PlainCallback): void {
    this.app.wasmApp.onTransitionComplete(this.wasmHandle, cb);
  }

  /** Calls `cb` each time the pointer moves onto this component. */
  onHoverEnter(cb: PlainCallback): void {
    this.app.wasmApp.onHoverEnter(this.wasmHandle, cb);
  }

  /** Calls `cb` each time the pointer moves off this component. */
  onHoverExit(cb: PlainCallback): void {
    this.app.wasmApp.onHoverExit(this.wasmHandle, cb);
  }

  /**
   * Calls `cb` each time the pointer is pressed on this component. This fires
   * at the same moment as {@link Handle.onClick}; use it with
   * {@link Handle.onRelease} to follow a press from start to end.
   */
  onPress(cb: PlainCallback): void {
    this.app.wasmApp.onPress(this.wasmHandle, cb);
  }

  /**
   * Calls `cb` each time the pointer is released after a press on this
   * component, even if the pointer has moved off it.
   */
  onRelease(cb: PlainCallback): void {
    this.app.wasmApp.onRelease(this.wasmHandle, cb);
  }

  /** Calls `cb` each time this component gains focus. */
  onFocus(cb: PlainCallback): void {
    this.app.wasmApp.onFocus(this.wasmHandle, cb);
  }

  /** Calls `cb` each time this component loses focus. */
  onBlur(cb: PlainCallback): void {
    this.app.wasmApp.onBlur(this.wasmHandle, cb);
  }

  /**
   * Calls `cb` every tick while this component is pressed, with the distance
   * the pointer moved since the previous tick, in world units (y points up).
   */
  onDrag(cb: DragCallback): void {
    this.app.wasmApp.onDrag(this.wasmHandle, (x: number, y: number) =>
      cb({ x, y }),
    );
  }

  /**
   * Makes `child` a child of this component. Its geometry becomes relative to
   * this component's.
   */
  addChild(child: Handle): void {
    this.app.wasmApp.addChild(this.wasmHandle, child.wasmHandle);
  }

  /**
   * Removes `child` from this component.
   *
   * If `destroy` is `true`, the child is also destroyed, exactly as by
   * {@link Handle.destroy}. Otherwise it is kept as a top-level component. Its
   * geometry is then no longer relative to this component, so it moves on
   * screen unless you update its geometry as well.
   */
  removeChild(child: Handle, destroy = false): void {
    this.app.wasmApp.removeChild(this.wasmHandle, child.wasmHandle, destroy);
  }

  /**
   * Splits this component into `targets`: a 1→N transition.
   *
   * Each target ends at its own declared geometry. This component is hidden
   * as soon as the split starts. With `"slice"` and `"gridSlice"`, slices of
   * this component move into place and the targets appear when they arrive;
   * with `"perTarget"`, the targets themselves move. The transition starts on
   * the next tick.
   *
   * `childBehavior`, if given, sets each target's timing separately. It is
   * called once per target with its index and the total; see
   * {@link ChildBehavior}.
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
   * Merges `sources` into this component: an N→1 transition.
   *
   * The sources are hidden as soon as the merge starts. `layout` decides
   * which part of this component each source moves toward. The transition
   * starts on the next tick. `childBehavior`, if given, sets each source's
   * timing separately; see {@link ChildBehavior}.
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
   * Destroys this component and its children, along with their callbacks and
   * the signals they own.
   *
   * @throws if it was already destroyed. Safe to ignore, but reported so that
   * destroying twice is visible.
   */
  destroy(): void {
    this.app.wasmApp.destroy(this.wasmHandle);
  }

  // check accuracy. Matches the Rust doc; the step 5 fixes to
  // freeResources (re-baking of text and images) will change it.
  /**
   * Releases this component's references to its baked text, image or
   * content. The component itself remains.
   *
   * This doesn't free atlas space immediately. A texture that no component
   * references becomes available for reuse, and the atlas reclaims its space
   * when it needs room for another texture. Textures marked `eternal` are
   * never reclaimed.
   *
   * A component with text or an image keeps it, and the host bakes it again
   * the next time it renders a frame.
   */
  freeResources(): void {
    this.app.wasmApp.freeResources(this.wasmHandle);
  }

  /**
   * Like {@link Handle.splitTo}, with each target's end geometry given
   * explicitly instead of taken from its declared geometry.
   *
   * Use this when a target should end somewhere other than its declared
   * geometry, or when this component is also one of the targets. In the
   * second case, pass its end geometry here rather than calling
   * {@link Handle.setDeclaredGeometry} first, which would move it before the
   * split begins.
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
   * Sets this component's declared geometry and moves it there immediately.
   *
   * Use this when a component's layout can only be worked out after it is
   * created, such as a cell sized to fit its text
   * ({@link Handle.bakedTextSize}). Transitions into the component, and its
   * interaction styles, use the new geometry.
   */
  setDeclaredGeometry(state: Geometry): void {
    this.app.wasmApp.setDeclaredGeometry(this.wasmHandle, state);
  }

  /**
   * Transitions this component from its current geometry to `to`.
   *
   * Unlike {@link SignalHandle.set}, only this component is involved, which
   * makes this a good fit for moving a component around repeatedly. Calling
   * it during a transition starts a new one from wherever the component is.
   * The transition starts on the next tick.
   *
   * @example
   * ```ts
   * card.animateTo(
   *   { ...card.get()!.geometry, position: { x: 300, y: 0, z: 0 } },
   *   { duration: 0.3, easing: "easeOutCubic" },
   * );
   * ```
   */
  animateTo(to: Geometry, config: TransitionConfig): void {
    this.app.wasmApp.animateTo(this.wasmHandle, to, config);
  }

  /**
   * The size in pixels of this component's baked text, or `undefined` if the
   * text hasn't been baked yet or the component has none. The host bakes text
   * the next time it renders a frame.
   */
  bakedTextSize(): Vec2 | undefined {
    return this.app.wasmApp.bakedTextSize(this.wasmHandle) as
      | Vec2
      | undefined;
  }

  /**
   * The size in pixels of this component's baked image, or `undefined` if the
   * image hasn't been baked yet or the component has none. This is the full
   * image size, even after {@link Handle.centerCropToSquare}.
   */
  bakedImageSize(): Vec2 | undefined {
    return this.app.wasmApp.bakedImageSize(this.wasmHandle) as
      | Vec2
      | undefined;
  }

  /**
   * Shows `source`'s baked image on this component, replacing its current
   * image. Returns `false` if `source` has no baked image yet.
   *
   * The two components share the texture, which stays in the atlas as long as
   * either one references it. Use this to prepare a hidden component before a
   * transition reveals it: for example, before merging grid tiles into a
   * detail view, copy the selected tile's image onto the detail view.
   */
  copyBakedImageFrom(source: Handle): boolean {
    return this.app.wasmApp.copyBakedImageFrom(
      this.wasmHandle,
      source.wasmHandle,
    );
  }

  /**
   * Crops this component's baked image to a centered square. Returns `false`
   * if it has no baked image yet.
   *
   * Use it to fill square cells, such as grid tiles, with images of any shape.
   * Only the visible region of the texture changes; no pixels are copied.
   */
  centerCropToSquare(): boolean {
    return this.app.wasmApp.centerCropToSquare(this.wasmHandle);
  }

  /**
   * Sets whether this component responds to input.
   *
   * A non-interactive component is never the target of any input, whether
   * pointer, touch, keyboard, gamepad or remote: it is never hovered, pressed,
   * dragged or focused. `false` has the same effect as
   * {@link ComponentSpec.nonInteractive}, applied after creation. For a
   * control that is temporarily unavailable and should look unavailable, use
   * {@link Handle.setDisabled}.
   *
   * Proteus currently handles pointer input only (on the web, that includes
   * touch and pen). Other kinds of input will follow the same rule.
   */
  setInteractive(interactive: boolean): void {
    this.app.wasmApp.setInteractive(this.wasmHandle, interactive);
  }

  /**
   * Shows or hides this component and its children.
   *
   * A hidden component is neither drawn nor hit-tested.
   * {@link SignalHandle.set} already hides the component it transitions from
   * and shows the one it transitions to; use this for everything else.
   *
   * A hidden component stops being drawn on the next frame and stops receiving
   * input one tick later. Input is matched against what was last drawn, so a
   * click in the same tick as the hide still reaches it.
   */
  setVisible(visible: boolean): void {
    this.app.wasmApp.setVisible(this.wasmHandle, visible);
  }

  /**
   * Sets this component's opacity, clamped to `0`–`1`.
   *
   * Opacity multiplies down the hierarchy: a child at `0.6` under a parent at
   * `0.6` is drawn at `0.36`. It only affects drawing, so a component at `0`
   * still receives pointer input. To take a component out of input as well,
   * use {@link Handle.setVisible}.
   */
  setOpacity(opacity: number): void {
    this.app.wasmApp.setOpacity(this.wasmHandle, opacity);
  }

  /**
   * Disables or re-enables this component.
   *
   * A disabled component is still drawn but ignores all input, and shows its
   * {@link ComponentSpec.disabled} style. Use it for a control that isn't
   * available yet, such as a submit button. For something that is never a
   * control, use {@link Handle.setInteractive}.
   */
  setDisabled(disabled: boolean): void {
    this.app.wasmApp.setDisabled(this.wasmHandle, disabled);
  }

  /**
   * Sets whether this component accepts input while transitioning.
   * `undefined` restores the default, where a transitioning component ignores
   * input.
   */
  setTransitioningConfig(config: TransitioningConfig | undefined): void {
    this.app.wasmApp.setTransitioningConfig(this.wasmHandle, config ?? null);
  }

  /**
   * Sets this component's image to `texture`, replacing its current image.
   * Returns `false` if the texture has been evicted.
   *
   * Only the image changes: text on the component is still drawn on top, and
   * video, if the component is showing it, still plays. Swapping textures is
   * cheap, which makes this suitable for frame-by-frame animation from
   * textures loaded in advance.
   */
  setTexture(texture: TextureHandle): boolean {
    return this.app.wasmApp.setTexture(this.wasmHandle, texture.wasmHandle);
  }
}

// ---------------------------------------------------------------------------
// SignalHandle
// ---------------------------------------------------------------------------

/**
 * A signal, which transitions one component into another: see
 * {@link SignalHandle.set}.
 */
export class SignalHandle {
  /** Prefer {@link ProteusApp.signal} over calling this directly. */
  constructor(
    private readonly app: ProteusApp,
    /** @internal */
    public readonly wasmHandle: WasmSignalHandle,
  ) {}

  /**
   * Transitions `from` into `to`: a 1→1 transition.
   *
   * `from` is hidden, and `to` is shown and moves from `from`'s current
   * geometry to its own declared geometry. If `to` is already transitioning,
   * the request is dropped unless `interruptible` is set; then a new
   * transition starts from wherever `to` is. The transition starts on the
   * next tick. A request that can't run is reported to
   * {@link SignalHandle.onDropped}.
   *
   * @example
   * ```ts
   * const open = app.signal();
   * button.onClick(() => open.set(panel, button, { duration: 0.4 }));
   * ```
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
   * Calls `cb` with the reason each time a {@link SignalHandle.set} request on
   * this signal can't run. See {@link DropReason}.
   */
  onDropped(cb: DroppedCallback): void {
    this.app.wasmApp.onDropped(this.wasmHandle, cb);
  }

  /** Destroys this signal and its `onDropped` handlers. Later `set` calls do nothing. */
  destroy(): void {
    this.app.wasmApp.signalDestroy(this.wasmHandle);
  }
}

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

/**
 * A texture in the atlas. Show it on a component with
 * {@link Handle.setTexture}.
 *
 * A texture can't be freed through its handle: once no component references
 * it, the atlas can reclaim its space when it needs room. See
 * {@link Handle.freeResources}.
 */
export class TextureHandle {
  /** Prefer {@link ProteusApp.texture} over calling this directly. */
  constructor(
    private readonly app: ProteusApp,
    /** @internal */
    public readonly wasmHandle: WasmTextureHandle,
  ) {}

  /** The texture's ID, for {@link ProteusApp.textureFromId}. */
  id(): number {
    return this.wasmHandle.id();
  }

  /** The texture's kind and size, or `undefined` if it has been evicted or never existed. */
  state(): TextureState | undefined {
    return this.app.wasmApp.textureState(this.wasmHandle) as
      | TextureState
      | undefined;
  }
}

// ---------------------------------------------------------------------------
// ProteusApp
// ---------------------------------------------------------------------------

/**
 * An app's components, signals and callbacks.
 *
 * {@link mount} creates one and passes it to your `setup` function; that is the
 * usual way to get one. `new ProteusApp()` creates a standalone app that draws
 * nothing, which is useful for tests and other headless use.
 */
export class ProteusApp {
  /** @internal */
  readonly wasmApp: WasmApp;

  /**
   * Creates a standalone app that draws nothing.
   *
   * @param wasmApp For internal use by {@link mount}; omit it.
   */
  constructor(wasmApp?: WasmApp) {
    this.wasmApp = wasmApp ?? new WasmApp();
  }

  /** Creates a component from `spec` and returns its handle. */
  component(spec: ComponentSpec): Handle {
    return new Handle(this, this.wasmApp.component(spec));
  }

  /**
   * Returns a {@link Handle} for a component ID, from {@link Handle.id} or
   * {@link ComponentData.children}.
   *
   * The ID isn't checked. If its component has been destroyed, the handle
   * behaves like any handle to a destroyed component.
   */
  handleFromId(id: number): Handle {
    return new Handle(this, WasmHandle.fromId(id));
  }

  /**
   * Creates a signal, which transitions one component into another with
   * {@link SignalHandle.set}. If `owner` is given, the signal is destroyed
   * along with it.
   */
  signal(owner?: Handle): SignalHandle {
    // The bridge takes the owner's ID rather than its handle object, which
    // it would otherwise consume.
    return new SignalHandle(this, this.wasmApp.signal(owner?.id()));
  }

  /** Returns a {@link TextureHandle} for a texture ID. The same as {@link ProteusApp.textureFromId}. */
  texture(id: number): TextureHandle {
    return new TextureHandle(this, this.wasmApp.texture(id));
  }

  /**
   * Decodes a PNG or JPEG image and adds it to the atlas, returning a handle
   * to show with {@link Handle.setTexture}.
   *
   * The pixels are on the GPU when this returns, so there is nothing to wait
   * for. You supply the bytes:
   *
   * ```ts
   * const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
   * const tex = app.loadTexture(bytes, { maxSide: 512 });
   * if (tex) tile.setTexture(tex);
   * ```
   *
   * Show the texture on a component right away: until a component uses it,
   * it may be evicted to make room.
   *
   * Returns `undefined` if the bytes can't be decoded. An image that decodes
   * but doesn't fit the atlas returns a handle to no texture, which draws
   * nothing.
   */
  loadTexture(
    bytes: Uint8Array,
    request?: TextureRequest,
  ): TextureHandle | undefined {
    const wasm = this.wasmApp.loadTexture(bytes, request ?? null);
    return wasm ? new TextureHandle(this, wasm) : undefined;
  }

  /**
   * Adds RGBA pixels to the atlas and returns a handle to them. `rgba` holds
   * `width * height * 4` bytes. For PNG or JPEG data, use
   * {@link ProteusApp.loadTexture}.
   *
   * Show the texture on a component right away: until a component uses it, it
   * may be evicted to make room. If the atlas is full, the handle refers to no
   * texture and draws nothing.
   */
  bakeTexture(
    width: number,
    height: number,
    rgba: Uint8Array,
    request?: TextureRequest,
  ): TextureHandle {
    return new TextureHandle(
      this,
      this.wasmApp.bakeTexture(width, height, rgba, request ?? null),
    );
  }

  /** Returns a {@link TextureHandle} for an ID from {@link TextureHandle.id}. */
  textureFromId(id: number): TextureHandle {
    return new TextureHandle(this, WasmTextureHandle.fromId(id));
  }

  /** A snapshot of a component's current state, or `undefined` if it has been destroyed. */
  get(handle: Handle): ComponentData | undefined {
    return this.wasmApp.get(handle.wasmHandle) as ComponentData | undefined;
  }

  /**
   * Advances the app by `deltaSeconds`, then runs callbacks for the events
   * that occurred. {@link mount} calls this for you every frame.
   */
  tick(deltaSeconds: number): void {
    this.wasmApp.tick(deltaSeconds);
  }

  /**
   * Updates the pointer position, in world units: the origin is the center of
   * the viewport and y points up. Convert from page coordinates with
   * {@link topLeftToWorld}. {@link mount} reports the pointer for you.
   */
  pointerMoved(x: number, y: number): void {
    this.wasmApp.pointerMoved(x, y);
  }

  /** Records that the pointer left the viewport. */
  pointerLeft(): void {
    this.wasmApp.pointerLeft();
  }

  /** Records that the pointer was pressed. */
  pointerPressed(): void {
    this.wasmApp.pointerPressed();
  }

  /** Records that the pointer was released. */
  pointerReleased(): void {
    this.wasmApp.pointerReleased();
  }

  /**
   * Calls {@link ProteusApp.tick} on every animation frame, and returns a
   * function that stops the loop.
   *
   * Only for a standalone app created with `new ProteusApp()`. An app from
   * {@link mount} is already ticked every frame; starting this loop as well
   * would tick it twice.
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
