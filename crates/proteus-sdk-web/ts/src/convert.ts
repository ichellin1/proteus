// Conversions from familiar web conventions (degrees, hex and named colors,
// page coordinates) to the values the SDK takes (radians, RGBA from 0 to 1,
// world units with the origin at the viewport center and y up).

import type { Color, Vec2 } from "./types.js";

/** Converts degrees to radians, the unit {@link Geometry.rotation} uses. */
export function degreesToRadians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

/** Converts radians to degrees. */
export function radiansToDegrees(radians: number): number {
  return (radians * 180) / Math.PI;
}

// A small set of CSS color names for quick prototyping. Deliberately not the
// full CSS list.
const NAMED_COLORS: Readonly<Record<string, string>> = {
  black: "#000000",
  white: "#ffffff",
  red: "#ff0000",
  green: "#008000",
  blue: "#0000ff",
  yellow: "#ffff00",
  orange: "#ffa500",
  purple: "#800080",
  gray: "#808080",
  grey: "#808080",
  transparent: "#00000000",
};

/**
 * Converts a hex color or a color name to a {@link Color}.
 *
 * Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the `#`, and the
 * names black, white, red, green, blue, yellow, orange, purple, gray (or grey)
 * and transparent.
 *
 * @throws if `input` isn't one of those.
 */
export function colorFrom(input: string): Color {
  const named = NAMED_COLORS[input.toLowerCase()];
  const hex = (named ?? input).replace(/^#/, "");

  let r: number;
  let g: number;
  let b: number;
  let a = 1;

  if (hex.length === 3) {
    r = parseInt(hex[0] + hex[0], 16);
    g = parseInt(hex[1] + hex[1], 16);
    b = parseInt(hex[2] + hex[2], 16);
  } else if (hex.length === 6 || hex.length === 8) {
    r = parseInt(hex.slice(0, 2), 16);
    g = parseInt(hex.slice(2, 4), 16);
    b = parseInt(hex.slice(4, 6), 16);
    if (hex.length === 8) {
      a = parseInt(hex.slice(6, 8), 16) / 255;
    }
  } else {
    throw new Error(`colorFrom: unrecognized color "${input}"`);
  }

  if ([r, g, b].some((c) => Number.isNaN(c))) {
    throw new Error(`colorFrom: unrecognized color "${input}"`);
  }

  return { r: r / 255, g: g / 255, b: b / 255, a };
}

/**
 * Converts a position measured from the canvas's top-left corner, with y
 * pointing down (the usual page and canvas convention), to world units, with
 * the origin at the center and y pointing up. `viewportWidth` and
 * `viewportHeight` are the canvas's size in the same units as `x` and `y`.
 */
export function topLeftToWorld(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
): Vec2 {
  return { x: x - viewportWidth / 2, y: viewportHeight / 2 - y };
}

/** Converts world units back to a position from the top-left; the inverse of {@link topLeftToWorld}. */
export function worldToTopLeft(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
): Vec2 {
  return { x: x + viewportWidth / 2, y: viewportHeight / 2 - y };
}
