/**
 * Proteus TypeScript SDK example: a browse screen.
 *
 * A TV-style screen, used with the arrow keys, Tab and Enter as well as the
 * pointer. A bar of genres, Drama, Action and Comedy, sits above two rows of
 * tiles, Films and Series, which run off the right edge and show the chosen
 * genre's titles. Enter on a tile, or a click, merges every tile into the
 * title's page, with a description, Play and Close. Play turns the page into
 * a full-screen video, as the genre bar fades out; Escape, Backspace or Delete
 * comes back to the page, and again, or Close, splits it back into the tiles.
 * Choosing another genre slides both rows back to their first tile.
 * Holding Enter on a tile for half a second favourites it.
 *
 * Techniques worth copying:
 *   - Focus groups, each made by giving a component the others are under a
 *     `focusGroup`. The genre bar wraps left and right round, and focus
 *     entering it lands on the chosen genre, its default focus. Each row keeps
 *     left and right, lets up and down leave, and focus entering it from the
 *     bar or the other row lands on the nearest tile. The page keeps the
 *     arrows, and wraps Tab round.
 *   - Scrolling a row: a tile off screen can still take focus, as a member of
 *     the row focus is in, and its `onFocus` slides the row to show it.
 *   - The tiles become the page and back with a grid merge and split, 7 by 2,
 *     filled row by row, so each tile moves to its own part of the page. The
 *     page becomes the video and back through a transition channel.
 *   - Focus is put where it belongs from code: the app clears it as each
 *     transition starts, and gives it to Play, or to the tile the page opened
 *     from, as it ends.
 *   - The genre buttons are disabled while the page shows, so neither focus
 *     nor the pointer reaches them. They change color as a transition, and set
 *     `transitionInteraction` so they still take focus and clicks while it
 *     runs: a transitioning component otherwise doesn't.
 *   - The genre bar fades out for the video, its opacity eased frame by frame
 *     in `update`: opacity isn't part of a transition.
 *   - The focused style, with a glow and a border, shows only while the keys
 *     are in use, and the hover style only while the pointer is, so there's
 *     one highlight at a time. The glow rests at radius 0, so the focused
 *     style has one to grow.
 *   - A tap or a hold: each tile handles `"activate"` itself, so the key press
 *     doesn't click it, opens on a release that wasn't a long press, and
 *     favourites on the long press.
 *   - Back is the app's to define: its `onKey` steps back from the video to
 *     the page, and from the page to the tiles. Escape and Backspace are bound
 *     to it already, and the app binds Delete too, with `bindKey`.
 *   - The hint follows `inputMode()`, so it names the keys or the pointer,
 *     whichever is in use.
 *   - Text is drawn centered on its component, so a heading aligned left is
 *     sized to its text once the text is baked, with `bakedTextSize` and
 *     `setDeclaredGeometry`; and a tile's title too wide for it is cut short
 *     with "…", measured the same way. Text is a single line, so the
 *     description is one component a line.
 */

import { colorFrom, mount } from "proteus-sdk";
import type { Color, ComponentSpec, Geometry, Handle, ProteusApp, StyleOverride } from "proteus-sdk";

const BACKGROUND = colorFrom("#14131c");
const SURFACE = colorFrom("#24222f");
const SURFACE_HOVER = colorFrom("#302d3e");
const EDGE = colorFrom("#3a3650");
const EDGE_HOVER = colorFrom("#5c5778");
const ACCENT = colorFrom("#8f74e0");
const GOLD = colorFrom("#e0b33a");
const WHITE = colorFrom("#ffffff");
const DIM = colorFrom("#a9a4bf");
const DISABLED = colorFrom("#1c1a24");
const PAGE = colorFrom("#1b1a25");
const VIDEO = colorFrom("#050508");
const CLEAR = colorFrom("transparent");

const TILE_W = 220;
const TILE_H = 124;
/** From one tile's center to the next. */
const TILE_STRIDE = 244;
const TILES_PER_ROW = 7;
/** How close to the screen's edge a focused tile may come before its row slides. */
const EDGE_MARGIN = 40;
/** The space a tile's title keeps from the tile's edges, on each side. */
const TITLE_PAD = 10;
/** The widest a tile's title may be: wider ones are cut short with "…". */
const TITLE_MAX_WIDTH = TILE_W - 2 * TITLE_PAD;
const TITLE_SIZE = 17;
/** How long the genre bar takes to fade out or in, in seconds. */
const FADE_SECS = 0.3;

/** A genre, and the titles its rows show: films, then series. */
interface Genre {
  name: string;
  rows: [string[], string[]];
}

const GENRES: Genre[] = [
  {
    name: "Drama",
    rows: [
      ["The Shawshank Redemption", "The Godfather", "Forrest Gump", "Schindler's List", "Parasite", "Whiplash", "Good Will Hunting"],
      ["Breaking Bad", "Succession", "The Wire", "The Sopranos", "Mad Men", "The Crown", "Chernobyl"],
    ],
  },
  {
    name: "Action",
    rows: [
      ["Mad Max: Fury Road", "The Dark Knight", "John Wick", "Die Hard", "Gladiator", "The Matrix", "Top Gun: Maverick"],
      ["The Mandalorian", "The Boys", "Daredevil", "Reacher", "Vikings", "24", "Gangs of London"],
    ],
  },
  {
    name: "Comedy",
    rows: [
      ["Superbad", "The Hangover", "Anchorman: The Legend of Ron Burgundy", "Step Brothers", "Mean Girls", "Shaun of the Dead", "Knives Out"],
      ["The Office (US)", "Ted Lasso", "Friends", "Parks and Recreation", "Brooklyn Nine-Nine", "Abbott Elementary", "Arrested Development"],
    ],
  },
];

const ROW_NAMES = ["Films", "Series"];
/** The description on every title's page. */
const DESCRIPTION = [
  "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod",
  "tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim",
  "veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip.",
];

/** Which screen shows, or which change between two is under way. */
type Screen = "tiles" | "toPage" | "page" | "toTiles" | "toVideo" | "video" | "fromVideo";

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

/** A transparent box that only holds text or other components. */
function holder(x: number, y: number, width: number, height: number): ComponentSpec {
  return { geometry: box(x, y, width, height, CLEAR, 0), nonInteractive: true };
}

function label(content: string, sizePx: number, color: Color) {
  return { content, sizePx, color };
}

/** The focused style: it grows, its border lights up and a glow spreads. */
/**
 * How many characters to show of a title that shows `shown` of them and is
 * `width` wide, to fit within `TITLE_MAX_WIDTH`; `null` if it fits already. A
 * guess in proportion to the width, and always at least one fewer, so fitting
 * ends.
 */
function shorterTitle(shown: number, width: number): number | null {
  if (width <= TITLE_MAX_WIDTH || shown <= 1) return null;
  const guess = Math.floor((shown * TITLE_MAX_WIDTH) / width);
  return Math.min(Math.max(guess, 1), shown - 1);
}

/** The first `n` characters of `title`, without trailing spaces, and "…". */
function cutTitle(title: string, n: number): string {
  return `${[...title].slice(0, n).join("").trimEnd()}…`;
}

const FOCUSED: StyleOverride = { scale: 1.06, borderColor: ACCENT, borderWidth: 3, glowRadius: 16 };
/** The hover style: lighter, and a little larger. */
const HOVER: StyleOverride = { scale: 1.03, color: SURFACE_HOVER, borderColor: EDGE_HOVER };

/**
 * A button or tile at rest: a dim border, and a glow of radius 0 for the
 * focused style to grow.
 */
function control(geometry: Geometry): ComponentSpec {
  return {
    geometry,
    border: { width: 2, color: EDGE, offset: -1 },
    glow: { radius: 0, color: ACCENT, intensity: 1 },
    focused: FOCUSED,
    hover: HOVER,
    pressed: { scale: 0.97 },
  };
}

const canvas = document.getElementById("app") as HTMLCanvasElement;

function main(app: ProteusApp): void {
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  const half = width / 2;
  let screen: Screen = "tiles";
  /** The tile the page opened from, by row and tile. */
  let opened: [number, number] | null = null;
  /** The genre bar's opacity, as it fades. */
  let barOpacity = 1;

  app.component({ ...holder(0, 310, 600, 40), text: label("Browse", 28, WHITE) });

  // The genre bar: focus entering it lands on the chosen genre, which
  // `showGenre` marks as its default focus, and left and right wrap round.
  // The buttons' color changes as a transition when the genre does; they
  // still take focus and clicks while it runs. Disabled while the page shows.
  const genreButtons = GENRES.map((genre, i) =>
    app.component({
      ...control(box((i - 1) * 150, 0, 130, 44, SURFACE, 22)),
      text: label(genre.name, 16, WHITE),
      disabled: { color: DISABLED, borderColor: EDGE },
      transitionInteraction: { allowPointer: true, allowNavigation: true },
    }),
  );
  const bar = app.component({
    ...holder(0, 250, 460, 60),
    children: genreButtons.map((b) => b.id()),
    focusGroup: { wrap: ["left", "right"], entry: "default" },
  });

  // The rows: left and right stay in a row, up and down leave it, and focus
  // coming from the bar or the other row lands on the nearest tile.
  const homeX = -half + EDGE_MARGIN + 40 + TILE_W / 2;
  const rows: { holder: Handle; tiles: Handle[]; heading: Handle; headingFitted: boolean; x: number }[] = [];
  const marks: Handle[][] = [];
  for (const [r, y] of [90, -120].entries()) {
    // Sized to its text in `fitHeadings`, once the text is baked.
    const heading = app.component({
      ...holder(homeX, y + TILE_H / 2 + 22, TILE_W, 30),
      text: label(ROW_NAMES[r], 18, DIM),
    });
    const rowMarks: Handle[] = [];
    const tiles = GENRES[0].rows[r].map((title, i) => {
      // A gold dot in the corner marks a favourite.
      const mark = app.component({
        geometry: box(TILE_W / 2 - 18, TILE_H / 2 - 18, 12, 12, GOLD, 6),
        nonInteractive: true,
        visible: false,
      });
      rowMarks.push(mark);
      return app.component({
        ...control(box(i * TILE_STRIDE, 0, TILE_W, TILE_H, SURFACE, 14)),
        text: label(title, TITLE_SIZE, WHITE),
        children: [mark.id()],
      });
    });
    const rowHolder = app.component({
      ...holder(homeX, y, TILE_STRIDE, TILE_H),
      children: tiles.map((t) => t.id()),
      focusGroup: { stop: ["left", "right"], entry: "nearest" },
    });
    rows.push({ holder: rowHolder, tiles, heading, headingFitted: false, x: homeX });
    marks.push(rowMarks);
  }
  /** Every tile, films then series: the order the page's 7 by 2 grid fills in, row by row. */
  const allTiles = rows.flatMap((r) => r.tiles);
  /** The favourite titles, kept across genres. */
  const favourites = new Set<string>();
  /**
   * How many of its title's characters each tile shows, by row and tile, while
   * it's still being fitted; `null` once it fits.
   */
  const titleFits: (number | null)[][] = rows.map((r) => r.tiles.map(() => null));
  /** The index in `GENRES` of the genre shown. */
  let genre = 0;

  function title(row: number, i: number): string {
    return GENRES[genre].rows[row][i];
  }

  // The page: the title, its description, and Play and Close. Hidden until
  // the tiles merge into it. The arrows stay in it, and Tab wraps round.
  const pageWidth = width - 2 * EDGE_MARGIN;
  const pageTitle = app.component(holder(0, 150, pageWidth, 40));
  const description = DESCRIPTION.map((line, i) =>
    app.component({ ...holder(0, 85 - i * 28, pageWidth, 28), text: label(line, 17, DIM) }),
  );
  const play = app.component({ ...control(box(-110, -110, 200, 56, ACCENT, 28)), text: label("Play", 18, WHITE) });
  const close = app.component({ ...control(box(110, -110, 200, 56, SURFACE, 28)), text: label("Close", 18, WHITE) });
  const page = app.component({
    geometry: box(0, -15, pageWidth, 420, PAGE, 24),
    border: { width: 2, color: EDGE, offset: -1 },
    nonInteractive: true,
    visible: false,
    children: [pageTitle, ...description, play, close].map((c) => c.id()),
    focusGroup: { stop: ["up", "down", "left", "right"], wrap: ["next", "previous"] },
  });

  const hint = app.component(holder(0, -270, 900, 30));
  const status = app.component(holder(0, -305, 900, 30));

  // The video, over everything: the page turns into it.
  const videoTitle = app.component(holder(0, 20, width, 40));
  const videoHint = app.component({ ...holder(0, -30, width, 30), text: label("Escape, Backspace or Delete to go back", 16, DIM) });
  const video = app.component({
    geometry: box(0, 0, width, height, VIDEO, 0),
    nonInteractive: true,
    visible: false,
    children: [videoTitle.id(), videoHint.id()],
  });
  /** Between the page and the video. */
  const channel = app.transitionChannel();

  function say(message: string): void {
    status.setText(label(message, 16, GOLD));
  }

  const transition = (duration: number) => ({ duration, easing: "easeOutCubic" as const });

  /** Merges every tile into the page of the tile at `[row, i]`. */
  function openPage(row: number, i: number): void {
    if (screen !== "tiles") return;
    screen = "toPage";
    opened = [row, i];
    // The page is hidden until the merge ends, so it comes in already naming
    // the title.
    pageTitle.setText(label(title(row, i), 30, WHITE));
    videoTitle.setText(label(`Playing ${title(row, i)}`, 30, WHITE));
    genreButtons.forEach((b) => b.setDisabled(true));
    app.clearFocus();
    page.mergeFrom(allTiles, transition(0.45), { kind: "grid", cols: TILES_PER_ROW, rows: 2 });
  }

  /** Splits the page back into the tiles. */
  function closePage(): void {
    if (screen !== "page") return;
    screen = "toTiles";
    app.clearFocus();
    page.splitTo(allTiles, transition(0.45), { kind: "grid", cols: TILES_PER_ROW, rows: 2 });
  }

  /** Turns the page into the video. */
  function startVideo(): void {
    if (screen !== "page") return;
    screen = "toVideo";
    app.clearFocus();
    channel.set(video, page, transition(0.5));
  }

  /** Turns the video back into the page. */
  function stopVideo(): void {
    if (screen !== "video") return;
    screen = "fromVideo";
    channel.set(page, video, transition(0.45));
  }

  /**
   * Finishes the change under way, as its transition ends: the page reports
   * the end of the merge into it, the split out of it and the change back
   * from the video, and the video the change into it.
   */
  function arrived(): void {
    if (screen === "toPage" || screen === "fromVideo") {
      screen = "page";
      play.focus();
    } else if (screen === "toTiles") {
      screen = "tiles";
      genreButtons.forEach((b) => b.setDisabled(false));
      if (opened) rows[opened[0]].tiles[opened[1]].focus();
    } else if (screen === "toVideo") {
      screen = "video";
    }
  }

  function toggleFavourite(row: number, i: number): void {
    const t = title(row, i);
    if (favourites.has(t)) favourites.delete(t);
    else favourites.add(t);
    marks[row][i].setVisible(favourites.has(t));
    say(favourites.has(t) ? `${t} is a favourite` : `${t} isn't a favourite any more`);
  }

  /**
   * Shows genre `g`'s titles in the rows, sliding each back to where it
   * starts, and marks its button as chosen, and as where focus entering the
   * bar lands.
   */
  function showGenre(g: number): void {
    genre = g;
    rows.forEach((r) => slide(r, homeX, 0.5));
    rows.forEach((r, row) =>
      r.tiles.forEach((tile, i) => {
        tile.setText(label(title(row, i), TITLE_SIZE, WHITE));
        titleFits[row][i] = [...title(row, i)].length;
        marks[row][i].setVisible(favourites.has(title(row, i)));
      }),
    );
    genreButtons.forEach((button, i) => {
      button.setDefaultFocus(i === g);
      const to = button.declaredGeometry();
      if (to) button.animateTo({ ...to, color: i === g ? ACCENT : SURFACE }, transition(0.25));
    });
  }

  /**
   * Slides row `row` so its tile `i` is on screen: no further right than the
   * screen's edge allows, and no further left than where the first tile
   * rests, so that coming back to the first tile puts the row back.
   */
  function scrollTo(row: number, i: number): void {
    const r = rows[row];
    const reach = canvas.clientWidth / 2 - EDGE_MARGIN - TILE_W / 2;
    const tileX = r.x + i * TILE_STRIDE;
    let x: number;
    if (tileX > reach) x = r.x - (tileX - reach);
    else if (tileX < homeX) x = r.x + (homeX - tileX);
    else return;
    slide(r, x, 0.3);
  }

  /** Slides row `r` across to `x`, over `secs` seconds, easing out. */
  function slide(r: (typeof rows)[number], x: number, secs: number): void {
    if (r.x === x) return;
    r.x = x;
    const to = r.holder.declaredGeometry();
    if (to) r.holder.animateTo({ ...to, position: { ...to.position, x } }, transition(secs));
  }

  genreButtons.forEach((button, g) =>
    button.onClick(() => {
      showGenre(g);
      say(`Showing ${GENRES[g].name}`);
    }),
  );
  showGenre(0);

  rows.forEach((r, row) =>
    r.tiles.forEach((tile, i) => {
      tile.onClick(() => openPage(row, i));
      tile.onFocus(() => scrollTo(row, i));
      // A tap opens the tile, and a hold favourites it. Handling "activate"
      // here stops its press from clicking the tile, so it opens on the
      // release, once it's known not to be a hold.
      tile.onKey((e) => {
        if (e.action !== "activate") return false;
        if (e.state === "longPressed") toggleFavourite(row, i);
        else if (e.state === "released" && !e.afterLongPress) openPage(row, i);
        return true;
      });
    }),
  );

  play.onClick(startVideo);
  close.onClick(closePage);
  page.onTransitionComplete(arrived);
  video.onTransitionComplete(arrived);

  // Escape and Backspace are bound to "back" already; the app binds Delete
  // too. Back means whatever the app says: here, a step back from the video or
  // the page.
  app.bindKey({ key: "Delete" }, "back");
  app.onKey((e) => {
    if (e.action === "back" && e.state === "pressed") {
      if (screen === "video") stopVideo();
      else if (screen === "page") closePage();
      return true;
    }
    return false;
  });

  // Focus starts on the first film; it shows once the keys are used.
  rows[0].tiles[0].focus();

  everyFrame = (dt: number) => {
    showHint();
    fitHeadings();
    fitTitles();
    fadeBar(dt);
  };

  // Sizes each heading to its text once it's baked, with its left edge where
  // its row's first tile starts: text is drawn centered on its component.
  function fitHeadings(): void {
    for (const r of rows) {
      if (r.headingFitted) continue;
      const text = r.heading.bakedTextSize();
      const to = r.heading.declaredGeometry();
      if (!text || !to) continue;
      r.heading.setDeclaredGeometry({
        ...to,
        size: { ...to.size, width: text.width },
        position: { ...to.position, x: homeX - TILE_W / 2 + text.width / 2 },
      });
      r.headingFitted = true;
    }
  }

  // Cuts short, with "…", each tile title wider than the tile allows, once its
  // text is baked: text is a single line, drawn at its own size. A title is
  // measured again after each cut, so it may take a few frames.
  function fitTitles(): void {
    rows.forEach((r, row) =>
      r.tiles.forEach((tile, i) => {
        const shown = titleFits[row][i];
        if (shown === null) return;
        const text = tile.bakedTextSize();
        if (!text) return;
        const next = shorterTitle(shown, text.width);
        titleFits[row][i] = next;
        if (next !== null) tile.setText(label(cutTitle(title(row, i), next), TITLE_SIZE, WHITE));
      }),
    );
  }

  // Eases the genre bar's opacity toward 0 while the video plays, or is about
  // to, and toward 1 otherwise.
  function fadeBar(dt: number): void {
    const target = screen === "toVideo" || screen === "video" ? 0 : 1;
    if (barOpacity === target) return;
    const step = dt / FADE_SECS;
    barOpacity = barOpacity < target ? Math.min(barOpacity + step, target) : Math.max(barOpacity - step, target);
    bar.setOpacity(barOpacity);
  }

  // Names the keys or the pointer in the hint, whichever is in use, for the
  // screen that shows.
  let hintShown: string | null = null;
  function showHint(): void {
    const keys = app.inputMode() === "navigating";
    let text: string;
    if (screen === "tiles") {
      text = keys
        ? "Arrows and Tab to move, Enter to open, hold Enter to favourite"
        : "Click a tile to open it, or use the arrow keys";
    } else if (screen === "page") {
      text = keys ? "Left and Right to choose, Enter to select, Escape, Backspace or Delete to go back" : "Click Play or Close";
    } else {
      return;
    }
    if (text === hintShown) return;
    hintShown = text;
    hint.setText(label(text, 16, DIM));
  }
}

let everyFrame = (_dt: number): void => {};

await mount("app", {
  setup: main,
  update(deltaSeconds) {
    everyFrame(deltaSeconds);
  },
  config: {
    render: { clearColor: [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 1] },
    // Keys reach the canvas without a click first, and Tab wraps round, as in
    // a full-screen app; without `keepTab`, Tab would leave the canvas at
    // either end.
    input: { autofocus: true, keepTab: true },
  },
});
