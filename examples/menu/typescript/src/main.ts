/**
 * Proteus TypeScript SDK example: a menu.
 *
 * A theme picker. Clicking the button splits it into a column of menu items
 * (1→N). Picking a theme merges the items back into the button (N→1), which
 * now names the new theme, and the preview card above fades to its color.
 * Clicking anywhere else while the menu is open merges it back unchanged.
 * Nothing is opened or closed: the button becomes the menu, and the menu
 * becomes the button.
 *
 * Techniques worth copying:
 *   - Create the menu items once, hidden, and reuse them: a split reveals its
 *     targets when it ends, and a merge hides its sources when it starts.
 *   - Change the button while it's hidden during the menu, with `setText`,
 *     so it comes back from the merge already naming the new choice.
 *   - Track whether the menu is opening, open, closing or closed, and ignore
 *     clicks that don't fit. The button's `onTransitionComplete` reports the
 *     end of both: a split reports on its source, and a merge on its
 *     destination.
 *   - Interaction styles: the items grow on hover and shrink while pressed,
 *     and the disabled item is gray. A disabled component is still there,
 *     blocking the pointer, but its callbacks don't run.
 *   - A transparent component behind the menu, shown only while it's open,
 *     catches a click on empty space.
 */

import { colorFrom, mount } from "proteus-sdk";
import type { Color, Geometry, Handle, ProteusApp } from "proteus-sdk";

interface Theme {
  name: string;
  color: Color;
  /** Whether the theme can't be picked yet. */
  soon?: boolean;
}

const THEMES: Theme[] = [
  { name: "Violet", color: colorFrom("#7a5fb0") },
  { name: "Teal", color: colorFrom("#2a9d8f") },
  { name: "Amber", color: colorFrom("#d08a1e") },
  { name: "Rose", color: colorFrom("#c9506f") },
  { name: "Gold", color: colorFrom("#b8961f"), soon: true },
];

const BACKGROUND = colorFrom("#1d1b26");
const BUTTON = colorFrom("#2d2a3a");
const BUTTON_HOVER = colorFrom("#3d394f");
const WHITE = colorFrom("#ffffff");
const GRAY = colorFrom("#4a4756");

const ITEM_W = 260;
const ITEM_H = 52;
const ITEM_GAP = 8;

/** Where the button is, and where the first menu item goes. */
const BUTTON_Y = -20;

type MenuState = "closed" | "opening" | "open" | "closing";

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

function main(app: ProteusApp): void {
  let state: MenuState = "closed";
  let current = THEMES[0];

  // The preview card, which shows the current theme.
  const preview = app.component({
    geometry: box(0, 150, 360, 180, current.color, 20),
    nonInteractive: true,
    text: { content: current.name, sizePx: 30, color: WHITE },
  });

  // Behind the menu: catches a click on empty space while the menu is open.
  // Transparent, but it still takes the pointer.
  const backdrop = app.component({
    geometry: box(0, 0, 4000, 4000, colorFrom("transparent"), 0),
    visible: false,
  });

  const button = app.component({
    geometry: box(0, BUTTON_Y, ITEM_W, 56, BUTTON, 28),
    hover: { color: BUTTON_HOVER },
    pressed: { scale: 0.96 },
    text: { content: `Theme: ${current.name}`, sizePx: 18, color: WHITE },
  });

  // The menu items, one per theme, in a column starting where the button is.
  // Hidden until the button splits into them.
  const items: Handle[] = THEMES.map((theme, i) =>
    app.component({
      geometry: box(0, BUTTON_Y - i * (ITEM_H + ITEM_GAP), ITEM_W, ITEM_H, theme.color, 12),
      hover: { scale: 1.04 },
      pressed: { scale: 0.97 },
      disabled: { color: GRAY },
      startDisabled: theme.soon,
      visible: false,
      text: { content: theme.soon ? `${theme.name} (soon)` : theme.name, sizePx: 18, color: WHITE },
    }),
  );

  function open(): void {
    if (state !== "closed") return;
    state = "opening";
    backdrop.setVisible(true);
    // The button is cut into a strip per item, top to bottom. Each strip
    // starts a little after the one above it.
    button.splitTo(items, { duration: 0.35 }, { kind: "column" }, (i) => ({
      duration: 0.3,
      delay: i * 0.04,
      easing: "easeOutCubic",
    }));
  }

  function close(picked: Theme | null): void {
    if (state !== "open") return;
    state = "closing";
    backdrop.setVisible(false);
    if (picked && picked !== current) {
      current = picked;
      // The button is hidden while the menu is open, so it comes back from
      // the merge already naming the new theme.
      button.setText({ content: `Theme: ${picked.name}`, sizePx: 18, color: WHITE });
      const card = preview.get();
      if (card) {
        preview.animateTo({ ...card.geometry, color: picked.color }, { duration: 0.4 });
      }
      preview.setText({ content: picked.name, sizePx: 30, color: WHITE });
    }
    // The items come back from the bottom up.
    button.mergeFrom(items, { duration: 0.3 }, { kind: "column" }, (i, total) => ({
      duration: 0.25,
      delay: (total - 1 - i) * 0.03,
      easing: "easeOutCubic",
    }));
  }

  button.onClick(open);
  backdrop.onClick(() => close(null));
  items.forEach((item, i) => item.onClick(() => close(THEMES[i])));

  // A split reports its end on its source, and a merge on its destination:
  // both are the button.
  button.onTransitionComplete(() => {
    if (state === "opening") state = "open";
    else if (state === "closing") state = "closed";
  });
}

await mount("app", {
  setup: main,
  config: { render: { clearColor: [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 1] } },
});
