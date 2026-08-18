/**
 * Convenience conversions between familiar web conventions (degrees, hex/
 * named colors, top-left screen coordinates) and the raw values every
 * `proteus-sdk` API actually expects (radians, RGBA floats, world-space
 * center-origin Y-up coordinates). The wasm bridge (`proteus-sdk-web`'s
 * Rust side) stays unit-agnostic-free — these conversions are TS-only, by
 * design (see `../src/lib.rs`'s top doc).
 */

import type { Color, Vec2 } from "./types.js";

export function degreesToRadians(degrees: number): number {
  return (degrees * Math.PI) / 180;
}

export function radiansToDegrees(radians: number): number {
  return (radians * 180) / Math.PI;
}

/**
 * A deliberately small set of CSS named colors — enough for quick
 * prototyping, not a full palette. Extend as needed; not this SDK's job to
 * bundle a complete named-color table.
 */
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
 * Parses `#rgb`, `#rrggbb`, `#rrggbbaa` (with or without the leading `#`),
 * or one of {@link NAMED_COLORS}, into a `proteus-sdk` {@link Color}
 * (each channel `0`–`1`).
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
 * Converts a top-left-origin, Y-down position (the CSS/canvas/DOM
 * convention) into `proteus-sdk`'s world-space (viewport-center origin,
 * Y-up). `viewportWidth`/`viewportHeight` are the canvas's current logical
 * size, in the same units as `x`/`y`.
 */
export function topLeftToWorld(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
): Vec2 {
  return { x: x - viewportWidth / 2, y: viewportHeight / 2 - y };
}

/** Inverse of {@link topLeftToWorld}. */
export function worldToTopLeft(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
): Vec2 {
  return { x: x + viewportWidth / 2, y: viewportHeight / 2 - y };
}
