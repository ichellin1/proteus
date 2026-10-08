/**
 * Proteus TypeScript SDK example: an onboarding flow, the same app as the Rust
 * stepper example.
 *
 * Four steps, each a card with its own shape and color. Next and Back turn the
 * current card into the next or previous one with a transition channel (1→1):
 * nothing is swapped, the card becomes the next step. Dots below show the
 * progress, and Back is disabled on the first step.
 *
 * Techniques worth copying:
 *   - One transition channel drives the whole flow. The code that calls `set`
 *     decides where each step goes; the cards don't refer to each other.
 *   - Clicking while a card is still moving needs no special handling: the
 *     next card starts from wherever the moving one is, and with an
 *     interruptible request, a card asked to come back changes course.
 *   - Back is disabled, not hidden, on the first step: it stays in place,
 *     turns gray, and ignores clicks.
 *   - Positions are measured from the canvas's center, so the layout needs no
 *     canvas size.
 */

import { colorFrom, mount } from "proteus-sdk";
import type { Color, Geometry, Handle, ProteusApp } from "proteus-sdk";

const BACKGROUND = colorFrom("#1d1b26");
const BUTTON = colorFrom("#2d2a3a");
const BUTTON_HOVER = colorFrom("#3d394f");
const DOT = colorFrom("#4a4756");
const WHITE = colorFrom("#ffffff");

const STEP = { duration: 0.5, easing: "easeOutCubic" } as const;
const DOT_CHANGE = { duration: 0.25, easing: "easeOutCubic" } as const;

/** One step of the flow: its card's title, size, corner radius and color. */
interface Step {
  title: string;
  width: number;
  height: number;
  cornerRadius: number;
  color: Color;
}

const STEPS: Step[] = [
  { title: "Welcome", width: 240, height: 240, cornerRadius: 120, color: colorFrom("#7a5fb0") },
  { title: "Choose a theme", width: 520, height: 200, cornerRadius: 24, color: colorFrom("#2a9d8f") },
  { title: "Turn on notifications", width: 340, height: 360, cornerRadius: 40, color: colorFrom("#d08a1e") },
  { title: "You're all set", width: 600, height: 320, cornerRadius: 16, color: colorFrom("#c9506f") },
];

const CARD_Y = 60;
const DOTS_Y = -180;
const BUTTONS_Y = -250;
const DOT_SIZE = 12;
const CURRENT_DOT_WIDTH = 36;
const DOT_GAP = 12;

/** Geometry for a box of the given size, centered at `(x, y)`. */
function box(x: number, y: number, width: number, height: number, color: Color, cornerRadius: number): Geometry {
  return {
    position: { x, y, z: 0 },
    size: { width, height },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color,
    cornerRadius,
  };
}

/**
 * Dot `i`'s geometry when step `current` is showing: the current dot is
 * stretched and white. The row stays centered.
 */
function dotGeometry(i: number, current: number): Geometry {
  const widths = STEPS.map((_, j) => (j === current ? CURRENT_DOT_WIDTH : DOT_SIZE));
  const total = widths.reduce((sum, w) => sum + w, 0) + DOT_GAP * (STEPS.length - 1);
  const left = widths.slice(0, i).reduce((sum, w) => sum + w + DOT_GAP, 0);
  const x = -total / 2 + left + widths[i] / 2;
  return box(x, DOTS_Y, widths[i], DOT_SIZE, i === current ? WHITE : DOT, DOT_SIZE / 2);
}

/** A button: dark, lighter on hover, smaller while pressed, and gray while disabled. */
function button(app: ProteusApp, x: number, label: string, startDisabled: boolean): Handle {
  return app.component({
    geometry: box(x, BUTTONS_Y, 150, 48, BUTTON, 24),
    hover: { color: BUTTON_HOVER },
    pressed: { scale: 0.96 },
    disabled: { color: { ...BUTTON, a: 0.4 } },
    startDisabled,
    text: { content: label, sizePx: 18, color: WHITE },
  });
}

function main(app: ProteusApp): void {
  // A card for each step, all in the same place. Only the first starts
  // visible; the channel shows each as the flow reaches it.
  const cards = STEPS.map((step, i) =>
    app.component({
      geometry: box(0, CARD_Y, step.width, step.height, step.color, step.cornerRadius),
      nonInteractive: true,
      visible: i === 0,
      text: { content: step.title, sizePx: 28, color: WHITE },
    }),
  );
  const dots = STEPS.map((_, i) => app.component({ geometry: dotGeometry(i, 0), nonInteractive: true }));
  const back = button(app, -90, "Back", true);
  const next = button(app, 90, "Next", false);

  const channel = app.transitionChannel();
  /** The step showing, or arriving. */
  let current = 0;

  /** Turns the current card into step `to`'s, and updates the dots and buttons to match. */
  function go(to: number): void {
    const from = current;
    current = to;
    // Interruptible: after Next, Back and Next again in quick succession, the
    // last card is still moving when it's asked to come back, and it changes
    // course from wherever it is. Without this, that request would be dropped.
    channel.set(cards[to], cards[from], STEP, true);
    dots.forEach((dot, i) => dot.animateTo(dotGeometry(i, to), DOT_CHANGE));
    back.setDisabled(to === 0);
    const label = to === STEPS.length - 1 ? "Start over" : "Next";
    next.setText({ content: label, sizePx: 18, color: WHITE });
  }

  // After the last step, start over.
  next.onClick(() => go((current + 1) % STEPS.length));
  back.onClick(() => {
    if (current > 0) go(current - 1);
  });
}

await mount("app", {
  setup: main,
  config: { render: { clearColor: [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 1] } },
});
