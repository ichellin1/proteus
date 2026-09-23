/**
 * Public TS interfaces for `proteus-sdk`'s data shapes. Field names and
 * nesting match the Rust DTOs in `proteus-sdk-web/src/dto.rs` exactly
 * (`serde(rename_all = "camelCase")`) — this file's shapes are what
 * actually crosses the wasm boundary, not just documentation.
 *
 * Units and coordinate space are **raw**: radians (not degrees), RGBA
 * floats 0–1 (not hex strings), world-space position (viewport-center
 * origin, Y-up — not top-left/CSS pixels). Use `convert.ts`'s helpers to
 * translate from the more familiar web conventions before constructing
 * these.
 */

export interface Vec2 {
  x: number;
  y: number;
}

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface Color {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** A component's geometric and visual state. */
export interface Geometry {
  position: Vec3;
  size: Vec2;
  /** Radians. */
  rotation: number;
  scale: number;
  anchor: Vec2;
  color: Color;
  cornerRadius: number;
}

/**
 * Sparse per-interaction-state override — only declare the fields that
 * change; everything else inherits from the component's declared
 * `geometry`.
 */
export interface StyleOverride {
  position?: Vec3;
  size?: Vec2;
  rotation?: number;
  scale?: number;
  anchor?: Vec2;
  color?: Color;
  cornerRadius?: number;
}

/** A single line of text (M4). Rasterized and baked into `main_atlas` automatically each frame. */
export interface TextSpec {
  content: string;
  /** Pixels. Valid range 1–512; 12–48 is the recommended sweet spot. */
  sizePx: number;
  /** Defaults to opaque white — set a dark value for light backgrounds. */
  color?: Color;
  /** Extra tracking between glyphs, in pixels. Default `0`. */
  letterSpacingPx?: number;
}

/**
 * A static image (M9.7), given its already-loaded raw file bytes — e.g.
 * straight from a `fetch()` response's `arrayBuffer()`/`Uint8Array`, PNG or
 * JPEG (format sniffed from the data, not a file extension). Decoded and
 * baked into `main_atlas` automatically each frame, same as {@link TextSpec}.
 */
export interface ImageSpec {
  bytes: Uint8Array;
  /** Downscale cap (longest side, pixels) applied before baking. `undefined` = native resolution. */
  maxSide?: number;
}

/** Draws an SDF-based border around a component. */
export interface Border {
  /** Pixels. `0` disables the border. */
  width: number;
  color: Color;
  /** -1 inner, 0 center, 1 outer — only `-1` renders correctly. */
  offset: number;
}

/** Draws a soft radial glow behind a component. Mutually exclusive with {@link DropShadow} — if both are set, the drop shadow wins. */
export interface Glow {
  /** Halo spread in pixels. */
  radius: number;
  color: Color;
  /** Opacity multiplier (effective alpha = `color.a * intensity`). */
  intensity: number;
}

/** Draws an SDF-based drop shadow behind a component. See {@link Glow} for the mutual-exclusivity note. */
export interface DropShadow {
  /** Displacement in entity-local pixels (X right, Y up). */
  offset: Vec2;
  color: Color;
  /** Penumbra softness in pixels. Values below 0.5 are clamped in the shader. */
  softness: number;
  /** Uniform shape expansion in pixels applied before softening. */
  spread: number;
}

/** Argument to {@link ProteusApp.component}. */
export interface ComponentSpec {
  geometry: Geometry;
  hover?: StyleOverride;
  pressed?: StyleOverride;
  focused?: StyleOverride;
  disabled?: StyleOverride;
  /** Child component ids ({@link Handle.id}) — see {@link Handle.addChild} for adding children after creation. */
  children?: number[];
  /** Collapse this component (and any `children`) into one permanent textured quad. */
  bake?: boolean;
  text?: TextSpec;
  image?: ImageSpec;
  border?: Border;
  glow?: Glow;
  dropShadow?: DropShadow;
  /**
   * Opts this component out of hit-testing entirely — e.g. a full-window
   * background that shouldn't swallow clicks meant for something on top of
   * it. Every component is interactive (`onClick`/etc. "just work") by
   * default unless this is set.
   */
  nonInteractive?: boolean;
  /**
   * Whether the component is visible when spawned. Defaults to `true`.
   *
   * `false` spawns it inert — skipped by render, input and navigation —
   * until something reveals it. {@link SignalHandle.set} does that for its
   * `to` side, so a component declared hidden here is ready to be morphed
   * into without a separate reveal call.
   */
  visible?: boolean;
  /**
   * Alpha multiplier for this component and everything under it, clamped to
   * `0.0`–`1.0`. Defaults to fully opaque.
   *
   * Cascades down: a child's effective opacity is its own times its
   * parent's effective, so `0.6` over `0.6` paints at `0.36`. A child never
   * affects its parent.
   *
   * Separate from {@link ComponentSpec.visible} and unrelated to it —
   * opacity is a paint multiplier, visibility is an ECS flag. A component
   * at `0` opacity is invisible but still hit-tests; a hidden one doesn't.
   */
  opacity?: number;
}

/**
 * One of `proteus-ui`'s five built-in, already-tested easing functions
 * (implemented since the project's M2 milestone). Letting a caller supply
 * an arbitrary *custom* easing function is a separate, not-yet-built
 * feature (tracked as the framework's "pluggable interpolation interface").
 */
export type EasingName =
  | "linear"
  | "easeInQuad"
  | "easeOutQuad"
  | "easeInOutQuad"
  | "easeOutCubic";

export interface TransitionConfig {
  /** Seconds. */
  duration: number;
  /** Seconds to wait before the transition starts. Default `0`. */
  delay?: number;
  /** Default `"linear"`. */
  easing?: EasingName;
}

/**
 * How a 1→N transition ({@link Handle.splitTo}) is normalized to a set of
 * 1→1 lerps — see `proteus_ui::SplitStrategy`'s own doc for the visual
 * difference between each. `cols`/`rows` only apply to `"gridSlice"`.
 */
/**
 * Per-child transition config for a group transition — Phase A's
 * `childBehavior` iterator. Called once per target (or source) with its
 * index and the total, and its result overrides the shared config for that
 * child. The usual reason is a stagger:
 *
 * ```ts
 * source.splitTo(targets, config, { kind: "slice" },
 *   (i) => ({ duration: 0.4, delay: i * 0.08, easing: "easeOutCubic" }));
 * ```
 *
 * Called up front, when the transition is requested — never from inside the
 * engine's own update, so an ordinary closure is safe. A throw, or a return
 * value that isn't a {@link TransitionConfig}, raises an error naming the
 * index rather than silently substituting a default.
 */
export type ChildBehavior = (
  index: number,
  total: number,
) => TransitionConfig;

export type SplitStrategy =
  /**
   * One independent 1-to-1 transition per target. **Experimental for V1.**
   *
   * Each target renders its own content, starting at the source's rectangle
   * and moving to its own — not N copies of the source. No virtuals, no
   * bake, no GPU work. The hand-authored option: you decide what each target
   * is and where it lands, and you own whether the set reads well together.
   *
   * Experimental because nothing in the reference demo exercises it, and
   * because the per-target control it exists for is only half-exposed —
   * per-target *geometry* works, per-target *timing* needs `childBehavior`,
   * which the SDK doesn't surface yet.
   *
   * Completion fires on each **target**, not on the source — see
   * {@link Handle.onTransitionComplete}.
   */
  | { kind: "perTarget" }
  /**
   * Flattens the source — and its whole subtree — into one texture, then
   * hands each target a crop of it to morph from. What you want when the
   * pieces should read as parts of the thing that was there.
   */
  | { kind: "slice" }
  /** {@link SplitStrategy | `"slice"`}, but cropping a `cols`x`rows` grid rather than a row of strips. */
  | { kind: "gridSlice"; cols: number; rows: number };

/**
 * How an N→1 transition ({@link Handle.mergeFrom}) divides the destination
 * among its sources — see `proteus_ui::MergeLayout`'s own doc. `cols`/`rows`
 * only apply to `"grid"`.
 */
export type MergeLayout =
  | { kind: "horizontal" }
  | { kind: "grid"; cols: number; rows: number };

export type InteractionState =
  | "default"
  | "hover"
  | "pressed"
  | "focused"
  | "disabled";

export interface TransitionSnapshot {
  base: Geometry;
  target: Geometry;
  /** Same value as the enclosing {@link ComponentData.geometry}. */
  current: Geometry;
  /** `0`–`1`, pre-easing. */
  progress: number;
}

/** Return shape of {@link ProteusApp.get}/{@link Handle.get}. */
export interface ComponentData {
  geometry: Geometry;
  state: InteractionState;
  visible: boolean;
  /** Child component ids ({@link Handle.id}) — reconstruct with {@link ProteusApp.handleFromId}. */
  children: number[];
  /**
   * `undefined` when idle; populated for the duration of an active
   * transition. `undefined` rather than `null` — matches the rest of this
   * API's "absent" convention (`ProteusApp.get`/`TextureHandle.state`
   * likewise return `undefined`, not `null`, for "nothing here"), and is
   * `serde-wasm-bindgen`'s own default encoding for a Rust `None` crossing
   * the wasm boundary.
   */
  transition?: TransitionSnapshot;
}

export type DropReason =
  | "signalNotFound"
  | "entityNotFound"
  | "alreadyTransitioning"
  | "entityNotVisible";

/** Payload delivered to a {@link SignalHandle.onDropped} handler. */
export interface TransitionDropped {
  to: number;
  from: number;
  reason: DropReason;
}

export type TextureKind = "static" | "video" | "animated";

/** Return shape of {@link TextureHandle.state}; `undefined` if evicted/unknown. */
export interface TextureState {
  kind: TextureKind;
  width: number;
  height: number;
}
