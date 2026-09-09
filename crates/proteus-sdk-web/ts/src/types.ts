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
