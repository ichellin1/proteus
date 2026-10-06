// The data types the SDK takes and returns. Their field names must match the
// Rust types in `../../src/dto.rs`, since these objects cross into the wasm
// module as they are.
//
// Units are radians, colors are RGBA with each channel from 0 to 1, and
// positions are in world units (origin at the viewport center, y up). The
// helpers in `convert.ts` convert from degrees, hex colors and page
// coordinates.

/** A 2D vector or size. */
export interface Vec2 {
  /** Horizontal: to the right, or the width. */
  x: number;
  /** Vertical: up in world units, or the height. */
  y: number;
}

/** A 3D position. Among top-level components that overlap, higher `z` is drawn on top. */
export interface Vec3 {
  /** To the right of the viewport's center. */
  x: number;
  /** Up from the viewport's center. */
  y: number;
  /** Drawing order: higher is drawn on top. */
  z: number;
}

/**
 * An RGBA color, each channel from `0` to `1`. Use {@link colorFrom} to convert
 * from a hex string or color name.
 */
export interface Color {
  /** Red, `0`–`1`. */
  r: number;
  /** Green, `0`–`1`. */
  g: number;
  /** Blue, `0`–`1`. */
  b: number;
  /** Alpha: `0` is transparent, `1` opaque. */
  a: number;
}

// DOC-
/** A component's position, size, shape and color: everything a transition animates. */
export interface Geometry {
  /** World units: origin at the viewport center, y up. See {@link topLeftToWorld}. */
  position: Vec3;
  /** Width and height, in pixels. */
  size: Vec2;
  /** Radians. See {@link degreesToRadians}. */
  rotation: number;
  /** Uniform scale; `1` is the component's natural size. */
  scale: number;
  /**
   * The point of the component that `position` refers to, as fractions of its
   * size from the top-left: `{ x: 0.5, y: 0.5 }` is the center.
   */
  anchor: Vec2;
  /** Fill color. */
  color: Color;
  /** Corner radius in pixels; `0` for square corners. */
  cornerRadius: number;
}

/**
 * The style a component takes in one interaction state, such as hover.
 * Declare only the fields that change; the rest come from the component's
 * declared geometry.
 */
export interface StyleOverride {
  /** Position, in world units. */
  position?: Vec3;
  /** Width and height, in pixels. */
  size?: Vec2;
  /** Rotation, in radians. */
  rotation?: number;
  /** Uniform scale. */
  scale?: number;
  /** The point `position` refers to, as fractions of the size. */
  anchor?: Vec2;
  /** Fill color. */
  color?: Color;
  /** Corner radius, in pixels. */
  cornerRadius?: number;
}

/** A single line of text. The host bakes it the next time it renders a frame. */
export interface TextSpec {
  /** The text: a single line. */
  content: string;
  /** Font size in pixels. */
  sizePx: number;
  /** Defaults to opaque white — set a dark value for light backgrounds. */
  color?: Color;
  /** Extra tracking between glyphs, in pixels. Default `0`. */
  letterSpacingPx?: number;
}

/**
 * An image, as the bytes of a PNG or JPEG file, for example from a `fetch()`
 * response. The format is detected from the data. The host decodes and bakes
 * it the next time it renders a frame.
 */
export interface ImageSpec {
  /** The PNG or JPEG file's bytes. */
  bytes: Uint8Array;
  /** Scale the image down so its longer side is at most this many pixels. Omit to keep its full size. */
  maxSide?: number;
}

/** A border drawn around a component. */
export interface Border {
  /** Pixels. `0` disables the border. */
  width: number;
  /** The border's color. An alpha of `0` turns it off. */
  color: Color;
  /** Where the border sits: `-1` inside the edge, `0` centered on it, `1` outside. Only `-1` draws correctly today. */
  offset: number;
}

/** A soft glow behind a component. A component shows a glow or a drop shadow, not both; if both are set, the drop shadow is drawn. */
export interface Glow {
  /** How far the glow spreads, in pixels. */
  radius: number;
  /** The glow's color. An alpha of `0` turns it off. */
  color: Color;
  /** Opacity multiplier (effective alpha = `color.a * intensity`). */
  intensity: number;
}

/** A drop shadow behind a component. Takes precedence over {@link Glow} if both are set. */
export interface DropShadow {
  /** How far the shadow is offset, in pixels (x right, y up). */
  offset: Vec2;
  /** The shadow's color. An alpha of `0` turns it off. */
  color: Color;
  /** How soft the shadow's edge is, in pixels. The minimum is `0.5`. */
  softness: number;
  /** How much larger than the component the shadow is, in pixels, before softening. */
  spread: number;
}

/**
 * A description of a component, passed to {@link ProteusApp.component}.
 *
 * @example
 * ```ts
 * const button = app.component({
 *   geometry: {
 *     position: { x: 0, y: 0, z: 0 },
 *     size: { x: 160, y: 48 },
 *     rotation: 0,
 *     scale: 1,
 *     anchor: { x: 0.5, y: 0.5 },
 *     color: colorFrom("#3366e6"),
 *     cornerRadius: 8,
 *   },
 *   hover: { scale: 1.05 },
 *   text: { content: "Open", sizePx: 18 },
 * });
 * ```
 */
export interface ComponentSpec {
  /**
   * The component's declared geometry: what it shows when no interaction
   * style applies, and where a transition into it ends.
   */
  geometry: Geometry;
  /** The style while the pointer is over the component. */
  hover?: StyleOverride;
  /** The style while the component is pressed. */
  pressed?: StyleOverride;
  /** The style while the component has focus. */
  focused?: StyleOverride;
  /** The style while the component is disabled. See {@link ComponentSpec.startDisabled} and {@link Handle.setDisabled}. */
  disabled?: StyleOverride;
  /**
   * IDs ({@link Handle.id}) of the components to make its children. Their
   * geometry becomes relative to this component's. To add children later, use
   * {@link Handle.addChild}.
   */
  children?: number[];
  /**
   * Bakes the component and its children into a single texture, permanently.
   * The host renders them once and then destroys the children, so their
   * handles stop working. Suited to detailed content that never changes.
   */
  bake?: boolean;
  /** A single line of text drawn on the component. */
  text?: TextSpec;
  /** An image drawn on the component. */
  image?: ImageSpec;
  /** A border around the component. */
  border?: Border;
  /** A glow behind the component. */
  glow?: Glow;
  /** A drop shadow behind the component. */
  dropShadow?: DropShadow;
  /**
   * Makes the component ignore all input.
   *
   * Components are interactive by default, so handlers such as
   * {@link Handle.onClick} work without an opt-in. A non-interactive component
   * is never the target of any input, whether pointer, touch, keyboard,
   * gamepad or remote. Use it for passive elements such as backgrounds and
   * labels. See {@link Handle.setInteractive}.
   */
  nonInteractive?: boolean;
  /**
   * Whether the component starts visible. Defaults to `true`.
   *
   * A hidden component is neither drawn nor hit-tested. A transition into it
   * through {@link SignalHandle.set} shows it, so a component that should
   * first appear through a transition can start hidden.
   */
  visible?: boolean;
  /**
   * The opacity of the component and everything under it, clamped to `0`–`1`.
   * Defaults to `1`.
   *
   * Opacity multiplies down the hierarchy: a child at `0.6` under a parent at
   * `0.6` is drawn at `0.36`. It only affects drawing, so a component at `0`
   * still receives pointer input; use {@link ComponentSpec.visible} to take it
   * out of input as well.
   */
  opacity?: number;
  /**
   * Creates the component disabled: drawn, but ignoring all input and showing
   * its {@link ComponentSpec.disabled} style.
   *
   * Use this for a control that is enabled later with
   * {@link Handle.setDisabled}. For something that is never a control, such as
   * a background or a label, use {@link ComponentSpec.nonInteractive} instead.
   */
  startDisabled?: boolean;
  /** Whether the component accepts input while transitioning. Without this, it doesn't. */
  transitioning?: TransitioningConfig;
}

/**
 * Engine settings for {@link mount}. Every field is optional; anything omitted
 * keeps the web default.
 *
 * Only settings that have an effect are listed. A misspelled field is
 * rejected when the app mounts, rather than silently ignored.
 */
export interface ProteusConfigOverrides {
  /** Clear color and presentation. */
  render?: {
    /** The color behind everything, and what shows through transparency: RGBA, each `0`–`1`. */
    clearColor?: [number, number, number, number];
    /** How frames are synchronized with the display. `"autoVsync"` suits most apps. */
    presentMode?:
      | "autoVsync"
      | "autoNoVsync"
      | "fifo"
      | "fifoRelaxed"
      | "immediate"
      | "mailbox";
    /** Which GPU to prefer on a device that has more than one. */
    powerPreference?: "none" | "lowPower" | "highPerformance";
  };
  /** GPU memory: atlas and instance-buffer sizes. */
  memory?: {
    /** The size and number of pages in the main texture atlas. A size larger than the device supports throws at mount. */
    mainAtlas?: {
      /** Width and height of each page, in pixels. */
      pageSize?: number;
      /** Number of pages. */
      pageCount?: number;
    };
    /** The size of the atlas used for images of components during splits and merges. */
    transitionAtlasSize?: number;
    /** The most components that can be drawn in one frame. */
    maxInstances?: number;
  };
  /** Frame timing. */
  frame?: {
    /** The longest time step one frame can take, so a tab returning from the background doesn't jump ahead. */
    dtClampSecs?: number;
  };
  /** Image and texture loading. */
  resources?: {
    /** Scale images down so their longer side is at most this many pixels. `null` keeps full size. */
    imageMaxSide?: number | null;
    /** Wait to bake a component's text or image until the component is visible. */
    lazyLoad?: boolean;
  };
}

/** How to add a texture to the atlas. */
export interface TextureRequest {
  /**
   * Scale the texture down so its longer side is at most this many pixels.
   * Omit to keep its full size; a photo from the network is usually much
   * larger than it will be drawn.
   */
  maxSide?: number;
  /**
   * Keep the texture in the atlas for the life of the app, never evicting it.
   * For textures in constant use, such as animation frames.
   */
  eternal?: boolean;
}

/** Whether a component accepts input while it is transitioning. Both default to `false`. */
export interface TransitioningConfig {
  /** Accept pointer input during a transition. */
  allowInput?: boolean;
  /** Not read yet; reserved for keyboard navigation. */
  allowNavigation?: boolean;
}

/**
 * A built-in easing curve: how a transition speeds up and slows down.
 *
 * - `"linear"`: constant speed.
 * - `"easeInQuad"`: starts slowly, then speeds up.
 * - `"easeOutQuad"`: starts quickly, then slows to a stop.
 * - `"easeInOutQuad"`: starts slowly, speeds up, then slows to a stop. The
 *   default.
 * - `"easeOutCubic"`: like `"easeOutQuad"`, with a stronger slowdown at the end.
 */
export type EasingName =
  | "linear"
  | "easeInQuad"
  | "easeOutQuad"
  | "easeInOutQuad"
  | "easeOutCubic";

/**
 * How a transition speeds up and slows down: a built-in curve's name, or a
 * cubic Bézier curve.
 *
 * `{ cubicBezier: [x1, y1, x2, y2] }` takes the same four numbers as CSS's
 * `cubic-bezier()`, so a curve can be copied from CSS or any easing tool. `x1`
 * and `x2` are clamped to `0`–`1`; `y1` and `y2` may go outside it, for a curve
 * that overshoots its target and settles back. Sizes and corner radii never go
 * below zero, and colors stay within `0`–`1`.
 *
 * An unknown name, or a malformed `cubicBezier`, throws when the transition is
 * requested.
 *
 * @example
 * ```ts
 * // Overshoots slightly, then settles: CSS's "back out" curve.
 * handle.animateTo(target, {
 *   duration: 0.4,
 *   easing: { cubicBezier: [0.34, 1.56, 0.64, 1] },
 * });
 * ```
 */
export type Easing =
  | EasingName
  | {
      /** The control points, as in CSS's `cubic-bezier(x1, y1, x2, y2)`. */
      cubicBezier: [number, number, number, number];
    };

/** How a transition is timed. */
export interface TransitionConfig {
  // DOC-REVIEW: accurate today; step 5 makes 0 instant and drops the panic,
  // and this doc changes with it.
  /**
   * How long the transition takes, in seconds. Must be positive. Zero or less
   * completes on the next tick, as if instant, and panics in a debug build of
   * the WebAssembly module.
   */
  duration: number;
  /** Seconds to wait before the transition starts. Default `0`. */
  delay?: number;
  /** How the transition speeds up and slows down. Default `"easeInOutQuad"`. */
  easing?: Easing;
}

/**
 * Sets each component's transition config (timing properties) in a split or merge. Called once per target
 * (or source) with its index and the total; its result replaces the shared
 * config for that component. The usual use is a stagger:
 *
 * ```ts
 * source.splitTo(targets, config, { kind: "slice" },
 *   (i) => ({ duration: 0.4, delay: i * 0.08, easing: "easeOutCubic" }));
 * ```
 *
 * Called once for each component when the transition is requested. If it
 * throws, or returns something that isn't a {@link TransitionConfig}, the
 * call fails with an error naming the index.
 */
export type ChildBehavior = (
  index: number,
  total: number,
) => TransitionConfig;

/**
 * How a split ({@link Handle.splitTo}) turns one component into several.
 */
export type SplitStrategy =
  /**
   * Each target moves from the source's position to its own, showing its own
   * content. **Experimental:** not yet used by the reference demo.
   *
   * Use it when you design each target yourself and want to control where and
   * when each one moves; {@link ChildBehavior} sets their timing. Completion is
   * reported on each target, not on the source; see
   * {@link Handle.onTransitionComplete}.
   */
  | {
      /** Selects this strategy. */
      kind: "perTarget";
    }
  /**
   * Bakes the source, with its children, into an image, then moves one slice
   * of that image to each target. Use it when the pieces should read as parts
   * of what was there. Slices are strips, side by side.
   */
  | {
      /** Selects this strategy. */
      kind: "slice";
    }
  /** Like `"slice"`, but the slices are a grid of `cols` by `rows`. */
  | {
      /** Selects this strategy. */
      kind: "gridSlice";
      /** Columns in the grid. */
      cols: number;
      /** Rows in the grid. */
      rows: number;
    };

/**
 * How a merge ({@link Handle.mergeFrom}) divides the destination among its
 * sources: which part of the destination each source moves toward.
 */
export type MergeLayout =
  /** Strips side by side, left to right, one per source in order. */
  | {
      /** Selects this layout. */
      kind: "horizontal";
    }
  /**
   * A grid of `cols` by `rows`, filled row by row from the top-left. Use it
   * when the sources are themselves laid out in a grid.
   */
  | {
      /** Selects this layout. */
      kind: "grid";
      /** Columns in the grid. */
      cols: number;
      /** Rows in the grid. */
      rows: number;
    };

/** The interaction style a component is showing. */
export type InteractionState =
  | "default"
  | "hover"
  | "pressed"
  | "focused"
  | "disabled";

/** A snapshot of a transition in progress. */
export interface TransitionSnapshot {
  /** The geometry the transition started from. */
  base: Geometry;
  /** The geometry the transition ends at. */
  target: Geometry;
  /** The current geometry, the same as {@link ComponentData.geometry}. */
  current: Geometry;
  /**
   * Progress through the transition, from `0` to `1`, before easing. Useful for
   * keeping other animation in step with the transition.
   */
  progress: number;
}

/**
 * A snapshot of a component's state, from {@link ProteusApp.get} or
 * {@link Handle.get}. It does not update afterwards.
 */
export interface ComponentData {
  /**
   * The component's current geometry: its declared geometry when idle, or its
   * in-progress geometry while transitioning.
   */
  geometry: Geometry;
  /**
   * The interaction style currently applied.
   *
   * Stays `"default"` for a component that declared no interaction styles,
   * even while it is disabled, and updates one tick after the change. To check
   * whether a component is disabled, use {@link ComponentData.disabled}.
   */
  state: InteractionState;
  /**
   * Whether the component is disabled. Unlike {@link ComponentData.state}, this
   * changes as soon as {@link Handle.setDisabled} is called, and is correct for
   * every component, including one with no interaction styles.
   */
  disabled: boolean;
  /** Whether the component is visible. A component inside a hidden parent is not. Updated each tick. */
  visible: boolean;
  /** The opacity it is drawn with: its own opacity multiplied by its parents'. Updated each tick. */
  opacity: number;
  /** IDs of the component's direct children. Get a handle with {@link ProteusApp.handleFromId}. */
  children: number[];
  /** The transition in progress, or `undefined` when idle. */
  transition?: TransitionSnapshot;
}

/**
 * Why a {@link SignalHandle.set} request couldn't run:
 *
 * - `"signalNotFound"`: the signal has been destroyed.
 * - `"entityNotFound"`: `to` or `from` has been destroyed.
 * - `"alreadyTransitioning"`: `to` is transitioning and the request wasn't
 *   `interruptible`.
 * - `"entityNotVisible"`: `from` is hidden, so there's nothing to transition
 *   from.
 */
export type DropReason =
  | "signalNotFound"
  | "entityNotFound"
  | "alreadyTransitioning"
  | "entityNotVisible";

/** A {@link SignalHandle.set} request that couldn't run, passed to {@link SignalHandle.onDropped}. */
export interface TransitionDropped {
  /** ID of the request's `to` component. */
  to: number;
  /** ID of the request's `from` component. */
  from: number;
  /** Why the request couldn't run. */
  reason: DropReason;
}

/** What a texture holds. `"animated"` is reserved and not used yet. */
export type TextureKind = "static" | "video" | "animated";

/** A texture's kind and size, from {@link TextureHandle.state}. */
export interface TextureState {
  /** What the texture holds. */
  kind: TextureKind;
  /** Width in pixels. */
  width: number;
  /** Height in pixels. */
  height: number;
}
