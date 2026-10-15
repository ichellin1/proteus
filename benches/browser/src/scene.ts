/**
 * The scenes every renderer draws: `count` components with random positions,
 * sizes and colors, the same every run and in every renderer.
 *
 * - **Large:** components 20 to 200 pixels across, the benchmark's original
 *   scene. At high counts they overlap many times, so filling
 *   pixels dominates, and with it how much work each renderer's shader does
 *   per pixel.
 * - **Small:** components 4 to 8 pixels across. Even 50,000 cover the canvas
 *   only about twice, so per-pixel work stays small, and what's left is each
 *   renderer's cost per component: preparing it, uploading it and drawing it.
 * - **Moving:** components 10 to 40 pixels across, about the size of an
 *   interface's, every one always moving; see `motion.ts`.
 */

/** The page's drawing area, in CSS pixels. */
export const WIDTH = 1280;
export const HEIGHT = 800;

export type SceneKind = "small" | "large" | "moving";

/** Each kind of scene's component sizes, from and to, and corner radius, in CSS pixels. */
export const SCENES: Record<SceneKind, { min: number; max: number; radius: number }> = {
  large: { min: 20, max: 200, radius: 8 },
  small: { min: 4, max: 8, radius: 2 },
  moving: { min: 10, max: 40, radius: 4 },
};

/** One component: its center and size in CSS pixels from the top-left, and its color. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
  r: number;
  g: number;
  b: number;
}

/**
 * The same xorshift generator as the native benchmark, so a run always draws
 * the same scene. JavaScript has no 64-bit integers outside `BigInt`, which
 * this uses.
 */
export class Random {
  private state = 0x9e3779b97f4a7c15n;
  private static readonly MASK = (1n << 64n) - 1n;

  /** A number from 0 to 1. */
  next(): number {
    let s = this.state;
    s ^= (s << 13n) & Random.MASK;
    s ^= s >> 7n;
    s ^= (s << 17n) & Random.MASK;
    this.state = s;
    return Number(s >> 40n) / 2 ** 24;
  }

  between(low: number, high: number): number {
    return low + (high - low) * this.next();
  }
}

export function scene(kind: SceneKind, count: number): Rect[] {
  const { min, max } = SCENES[kind];
  const random = new Random();
  return Array.from({ length: count }, () => {
    const width = random.between(min, max);
    const height = random.between(min, max);
    return {
      x: random.between(0, WIDTH),
      y: random.between(0, HEIGHT),
      width,
      height,
      r: random.next(),
      g: random.next(),
      b: random.next(),
    };
  });
}
