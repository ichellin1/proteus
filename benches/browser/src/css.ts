/**
 * The moving scene drawn with CSS, the way most web interfaces are built: a
 * `div` for each component, with rounded corners, positioned absolutely and
 * moved with `transform`. Every half second each gets a new target, and a CSS
 * transition animates it there, with the same easing as the others.
 *
 * Only the moving scene uses it. A still page of `div`s costs the browser
 * nothing per frame once it's drawn, so a still scene wouldn't measure
 * anything.
 */

import { CSS_EASING, Motion, MOVE_MS } from "./motion";
import type { Rect } from "./scene";

export interface CssScene {
  /** Starts a new move when one is due. Call once a frame. */
  frame(now: number): void;
  /** Removes the `div`s. */
  dispose(): void;
}

export function cssScene(stage: HTMLElement, rects: Rect[], radius: number): CssScene {
  const motion = new Motion(rects);
  const fragment = document.createDocumentFragment();
  const elements = rects.map((r, i) => {
    const div = document.createElement("div");
    const style = div.style;
    style.position = "absolute";
    style.left = "0";
    style.top = "0";
    style.width = `${r.width}px`;
    style.height = `${r.height}px`;
    style.borderRadius = `${radius}px`;
    style.background = `rgb(${Math.round(r.r * 255)} ${Math.round(r.g * 255)} ${Math.round(r.b * 255)})`;
    style.transform = translate(motion.current[i * 2], motion.current[i * 2 + 1], r);
    style.transition = `transform ${MOVE_MS}ms ${CSS_EASING}`;
    fragment.append(div);
    return div;
  });
  stage.append(fragment);

  return {
    frame(now) {
      const targets = motion.retarget(now);
      if (!targets) return;
      elements.forEach((div, i) => {
        div.style.transform = translate(targets[i * 2], targets[i * 2 + 1], rects[i]);
      });
    },
    dispose() {
      stage.replaceChildren();
    },
  };
}

/** The transform that puts `r`'s center at (`x`, `y`). */
function translate(x: number, y: number, r: Rect): string {
  return `translate(${x - r.width / 2}px, ${y - r.height / 2}px)`;
}
