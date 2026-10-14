/**
 * The moving scene's motion: every half second, each component gets a new
 * random place, and moves there over half a second, easing in and out. Every
 * renderer makes the same moves, from the same seeded random numbers.
 *
 * Proteus and CSS animate the moves themselves, given each new target. The
 * other renderers draw each frame at the positions worked out here.
 */

import { HEIGHT, Random, type Rect, WIDTH } from "./scene";

/** How long each move takes, and how often a new one starts, in milliseconds. */
export const MOVE_MS = 500;

/**
 * The easing curve, Proteus's `easeInOutQuad`: slow, fast, slow. CSS gets it
 * as the cubic Bézier that matches it, `CSS_EASING`.
 */
export function ease(t: number): number {
  return t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2;
}
export const CSS_EASING = "cubic-bezier(0.455, 0.03, 0.515, 0.955)";

export class Motion {
  /** Each component's center, x then y, in CSS pixels from the top-left, now. */
  readonly current: Float32Array;
  private readonly from: Float32Array;
  private readonly to: Float32Array;
  private readonly random = new Random();
  private movedAt = -Infinity;

  constructor(rects: Rect[]) {
    this.current = new Float32Array(rects.flatMap((r) => [r.x, r.y]));
    this.from = this.current.slice();
    this.to = this.current.slice();
  }

  /**
   * Starts a new move if half a second has passed since the last, and
   * returns the new targets; otherwise returns `undefined`. Call once a frame.
   */
  retarget(now: number): Float32Array | undefined {
    if (now - this.movedAt < MOVE_MS) return undefined;
    this.movedAt = now;
    this.from.set(this.current);
    for (let i = 0; i < this.to.length; i += 2) {
      this.to[i] = this.random.between(0, WIDTH);
      this.to[i + 1] = this.random.between(0, HEIGHT);
    }
    return this.to;
  }

  /** Works out `current` for time `now`, for the renderers that draw each frame. */
  advance(now: number) {
    const t = ease(Math.min(1, (now - this.movedAt) / MOVE_MS));
    for (let i = 0; i < this.current.length; i++) {
      this.current[i] = this.from[i] + (this.to[i] - this.from[i]) * t;
    }
  }
}
