/**
 * Proteus TypeScript SDK example: an image gallery.
 *
 * Full assembly of the incremental rebuild (see PLANNING.md's M13.8
 * section) — every mechanism here was individually proven, in isolation,
 * before being combined:
 *   - grid tile → detail hero:      1→1  (`SignalHandle.set`)
 *   - detail hero → fresh grid:     1→N  (`Handle.splitTo` — "back to grid")
 *
 * `Handle.mergeFrom` (N→1) was exercised in its own isolated step during
 * the rebuild (a "related row" merging back into a new hero) but dropped
 * from this assembled example — it read as an extra, slightly confusing
 * detour rather than a natural part of the gallery flow. `mergeFrom` stays
 * proven working; it's just not part of this particular app.
 *
 * Every photo is fetched over the network from picsum.photos; `Text` and
 * `Image` both render throughout.
 *
 * Five lessons carried over from the individual steps, all load-bearing:
 *   - Fetch every batch of images *concurrently* (`Promise.all`), not one
 *     `await` per loop iteration — sequential fetches are both slower and
 *     leave each newly-created tile fully visible in its final spot before
 *     the transition that's supposed to hide-then-reveal it ever runs
 *     (found and fixed during step 6's `splitTo` work).
 *   - A tile → hero morph needs the hero to already carry an image *before*
 *     the geometry transition starts — a hero spawned blank shows an empty
 *     box that only grows, with the image popping in only once the hi-res
 *     fetch lands. `copyBakedImageFrom` works on an imageless entity
 *     precisely so the hero can start out showing an image and have it
 *     scale continuously with the box.
 *   - That initial image must be the tile's *uncropped* low-res fetch, not
 *     its (by-then square-cropped) own `BakedImage` — each grid tile keeps
 *     its uncropped low-res bake alive in an off-screen `full` handle (see
 *     `GridSlot.full`) for exactly this. Using the cropped tile instead
 *     would frame the photo differently than the eventual hi-res fetch,
 *     which shows up mid-crossfade as a "double exposure" (two
 *     differently-framed copies of the same photo blending together).
 *     Mirrors `proteus-demo`'s own `gallery.tile_full` stash.
 *   - There's no built-in single-entity image crossfade, so swapping in the
 *     hi-res image once it's fetched uses a second, transparent overlay
 *     entity that fades to opaque via `animateTo`'s `color.a` (the same
 *     mechanism `proteus-demo`'s own gallery uses for this) — an instant
 *     texture swap on the hero itself would be a visible hard cut.
 *   - picsum.photos crops its source photo to whatever exact width/height a
 *     request asks for. The tile fetch and the hi-res fetch request two
 *     different sizes of the *same* photo — if their requested aspect
 *     ratios aren't identical down to a fraction of a percent, picsum crops
 *     each source slightly differently, which shows up as a small
 *     left/right content shift the instant the crossfade swaps one for the
 *     other. Naively pinning the larger axis exactly and rounding the other
 *     isn't precise enough (some photos' true ratios round badly at small
 *     sizes); `fetchDimensions` instead searches a small window of
 *     candidate larger-axis values for whichever integer pair lands closest
 *     to the photo's real ratio. Mirrors `proteus-demo`'s own
 *     `gallery_fetch::fetch_dimensions` fix for the identical bug.
 *
 * One deliberate simplification: layout is computed once from the canvas's
 * size at load and never reflows on window resize.
 */

import { mount, colorFrom, topLeftToWorld } from "proteus-sdk";
import type { ProteusApp, Geometry, Color, Handle } from "proteus-sdk";

const COLS = 4;
const ROWS = 3;
const TILE = 110;
const GAP = 14;
const TITLE_H = 60;

const BG_COLOR = colorFrom("#ece6f7");
const CARD_COLOR = colorFrom("#ffffff");
const TEXT_COLOR = colorFrom("#3a2d52");
const ACCENT_COLOR = colorFrom("#7a5fb0");

const TRANSITION = { duration: 0.5, easing: "easeOutCubic" as const };
const CROSSFADE = { duration: 0.3, easing: "easeOutCubic" as const };

interface Photo {
  id: number;
  width: number;
  height: number;
}

/** A subset of `proteus-demo`'s own curated nature-photo list — enough distinct photos for a full grid with no repeats. */
const PHOTOS: Photo[] = [
  { id: 12, width: 2500, height: 1667 },
  { id: 18, width: 2500, height: 1667 },
  { id: 54, width: 3264, height: 2176 },
  { id: 66, width: 3264, height: 2448 },
  { id: 108, width: 2000, height: 1333 },
  { id: 114, width: 3264, height: 2448 },
  { id: 132, width: 1600, height: 1066 },
  { id: 162, width: 1500, height: 998 },
  { id: 168, width: 1920, height: 1280 },
  { id: 174, width: 1600, height: 589 },
  { id: 198, width: 3456, height: 2304 },
  { id: 216, width: 2500, height: 1667 },
  { id: 222, width: 1800, height: 977 },
  { id: 228, width: 4608, height: 3456 },
  { id: 282, width: 5000, height: 3333 },
  { id: 294, width: 3753, height: 2309 },
];

type Screen = "grid" | "detail";

let app: ProteusApp;
let vw = 0;
let vh = 0;

let screen: Screen = "grid";
let gridSlots: GridSlot[] = [];
let hero: Handle | null = null;

let backBtn: Handle | null = null;
let caption: Handle | null = null;

// ---------------------------------------------------------------------------
// Layout + fetch helpers
// ---------------------------------------------------------------------------

function geom(cx: number, cy: number, w: number, h: number, color: Color = CARD_COLOR, cornerRadius = 12): Geometry {
  const pos = topLeftToWorld(cx, cy, vw, vh);
  return {
    position: { x: pos.x, y: pos.y, z: 0 },
    size: { x: w, y: h },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color,
    cornerRadius,
  };
}

function fitBox(aspectW: number, aspectH: number, maxW: number, maxH: number) {
  const scale = Math.min(maxW / aspectW, maxH / aspectH);
  return { w: aspectW * scale, h: aspectH * scale };
}

/** How far below `sidePx` to search for a closer-fitting larger-axis value — see `fetchDimensions`'s doc. */
const FETCH_DIMENSIONS_SEARCH_WINDOW = 6;

/**
 * The larger axis pinned near `sidePx`, the other derived to match the real
 * aspect ratio as closely as an integer pixel size allows. Naively pinning
 * the larger axis at *exactly* `sidePx` and rounding the other axis can land
 * measurably off the true ratio for some photos (rounding is coarser the
 * smaller `sidePx` is) — enough that picsum.photos, which crops its source
 * to whatever exact size is requested, crops this fetch's source noticeably
 * differently than a same-photo fetch at a different `sidePx`. Searching a
 * small window of candidate larger-axis values (`sidePx`, `sidePx - 1`, ...)
 * for whichever integer pair comes closest to the true ratio fixes that —
 * mirrors `proteus-demo`'s own `gallery_fetch::fetch_dimensions`.
 */
function fetchDimensions(aspectW: number, aspectH: number, sidePx: number): [number, number] {
  const ratio = aspectW / aspectH;
  const roundedSidePx = Math.max(1, Math.round(sidePx));
  let bestW = roundedSidePx;
  let bestH = 1;
  let bestError = Infinity;
  const window = Math.min(FETCH_DIMENSIONS_SEARCH_WINDOW, roundedSidePx - 1);
  for (let delta = 0; delta <= window; delta++) {
    const larger = roundedSidePx - delta;
    const [w, h] =
      aspectW >= aspectH
        ? [larger, Math.max(1, Math.round(larger / ratio))]
        : [Math.max(1, Math.round(larger * ratio)), larger];
    const error = Math.abs(w / h - ratio);
    if (error < bestError) {
      bestW = w;
      bestH = h;
      bestError = error;
    }
  }
  return [bestW, bestH];
}

function photoUrl(id: number, w: number, h: number): string {
  return `https://picsum.photos/id/${id}/${w}/${h}`;
}

/** `count` distinct photos starting at `offset` (wrapping) — safe from repeats as long as `count <= PHOTOS.length`. */
function pickPhotos(count: number, offset: number): Photo[] {
  return Array.from({ length: count }, (_, i) => PHOTOS[(offset + i) % PHOTOS.length]);
}

async function fetchImageBytes(url: string): Promise<Uint8Array> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`fetch failed: ${url}: ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

function waitForBake(handles: Handle[]): Promise<void> {
  return new Promise((resolve) => {
    function check() {
      if (handles.every((h) => h.bakedImageSize() !== undefined)) {
        resolve();
      } else {
        requestAnimationFrame(check);
      }
    }
    check();
  });
}

async function loadBaked(url: string): Promise<Handle> {
  const bytes = await fetchImageBytes(url);
  const loader = app.component({
    geometry: geom(-9999, -9999, 1, 1),
    image: { bytes },
    nonInteractive: true,
  });
  await waitForBake([loader]);
  return loader;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function destroySoon(handle: Handle) {
  setTimeout(() => handle.destroy(), TRANSITION.duration * 1000 + 150);
}

function destroyUiChrome() {
  backBtn?.destroy();
  caption?.destroy();
  backBtn = null;
  caption = null;
}

function buildDetailUi(photoId: number) {
  destroyUiChrome();

  backBtn = app.component({
    geometry: geom(75, 30, 140, 36, CARD_COLOR, 8),
    hover: { color: ACCENT_COLOR },
    text: { content: "‹ Back to grid", sizePx: 15, color: TEXT_COLOR },
  });
  backBtn.onClick(() => void backToGrid());

  caption = app.component({
    geometry: geom(vw / 2, vh - 30, 280, 26, CARD_COLOR, 0),
    text: { content: `picsum.photos #${photoId}`, sizePx: 14, color: TEXT_COLOR },
  });
}

// ---------------------------------------------------------------------------
// Grid
// ---------------------------------------------------------------------------

interface GridSlot {
  /** The square-cropped tile shown in the grid. */
  handle: Handle;
  /**
   * The same low-res fetch, kept alive off-screen *before* cropping —
   * `handle`'s own `BakedImage` is square-cropped in place and so no longer
   * matches the photo's real framing. `enterDetail` uses this, not
   * `handle`, as the hero's initial image, so it starts out correctly
   * framed (just low-res) instead of mismatched against the hi-res fetch
   * that later crossfades in.
   */
  full: Handle;
  photo: Photo;
}

async function buildGrid(offset: number): Promise<GridSlot[]> {
  const photos = pickPhotos(COLS * ROWS, offset);
  const startX = (vw - (COLS * TILE + (COLS - 1) * GAP)) / 2;
  const startY = TITLE_H + (vh - TITLE_H - (ROWS * TILE + (ROWS - 1) * GAP)) / 2;

  const loaded = await Promise.all(
    photos.map(async (photo) => {
      const [w, h] = fetchDimensions(photo.width, photo.height, TILE * 2);
      const full = await loadBaked(photoUrl(photo.id, w, h));
      return { photo, full };
    }),
  );

  return loaded.map(({ photo, full }, i) => {
    const col = i % COLS;
    const row = Math.floor(i / COLS);
    const cx = startX + col * (TILE + GAP) + TILE / 2;
    const cy = startY + row * (TILE + GAP) + TILE / 2;

    const handle = app.component({ geometry: geom(cx, cy, TILE, TILE), hover: { scale: 1.06 } });
    handle.copyBakedImageFrom(full);
    handle.centerCropToSquare();

    return { handle, full, photo };
  });
}

function wireGridTiles(slots: GridSlot[]) {
  gridSlots = slots;
  for (const slot of slots) {
    slot.handle.onClick(() => void showDetail(slot));
  }
}

// ---------------------------------------------------------------------------
// Detail (1→1 from a grid tile)
// ---------------------------------------------------------------------------

async function showDetail(slot: GridSlot) {
  if (screen !== "grid") return;
  screen = "detail";

  for (const s of gridSlots) {
    if (s.handle !== slot.handle) {
      s.handle.destroy();
      s.full.destroy();
    }
  }
  gridSlots = [];

  await enterDetail(slot.photo, slot.full, (heroHandle) => {
    const sig = app.signal();
    sig.set(heroHandle, slot.handle, TRANSITION);
    destroySoon(slot.handle);
    destroySoon(slot.full);
  });
}

/**
 * Shared hero-construction: spawns the hero already carrying `lowResSource`'s
 * baked image (the clicked tile's *uncropped* low-res fetch — see
 * `GridSlot.full`'s doc for why not the tile itself) — so `startTransition`'s
 * geometry morph scales that image up continuously instead of showing a
 * blank box — then, once both the hi-res fetch and the morph have settled,
 * cross-fades in the hi-res image via a transparent overlay rather than
 * swapping it in as a hard cut. The hero itself only gets the hi-res image
 * once the overlay has fully faded to opaque (so a later `splitTo`/
 * `mergeFrom` off this handle carries the sharp image, not the low-res one).
 */
async function enterDetail(photo: Photo, lowResSource: Handle, startTransition: (hero: Handle) => void) {
  const box = fitBox(photo.width, photo.height, vw * 0.7, vh - TITLE_H - 140);
  const heroHandle = app.component({
    geometry: geom(vw / 2, TITLE_H + (vh - TITLE_H) / 2, box.w, box.h, CARD_COLOR, 16),
  });
  heroHandle.copyBakedImageFrom(lowResSource);
  startTransition(heroHandle);

  hero = heroHandle;
  buildDetailUi(photo.id);

  const [w, h] = fetchDimensions(photo.width, photo.height, 900);
  const [loader] = await Promise.all([
    loadBaked(photoUrl(photo.id, w, h)),
    sleep(TRANSITION.duration * 1000),
  ]);
  if (hero !== heroHandle) {
    loader.destroy();
    return;
  }

  const heroData = heroHandle.get();
  if (!heroData) {
    loader.destroy();
    return;
  }
  const overlay = app.component({
    geometry: { ...heroData.geometry, color: { ...heroData.geometry.color, a: 0 } },
    nonInteractive: true,
  });
  overlay.copyBakedImageFrom(loader);
  overlay.animateTo({ ...heroData.geometry, color: { ...heroData.geometry.color, a: 1 } }, CROSSFADE);

  setTimeout(() => {
    if (hero === heroHandle) {
      heroHandle.copyBakedImageFrom(loader);
    }
    overlay.destroy();
    loader.destroy();
  }, CROSSFADE.duration * 1000 + 50);
}

// ---------------------------------------------------------------------------
// Back to grid (1→N)
// ---------------------------------------------------------------------------

async function backToGrid() {
  if (screen !== "detail" || !hero) return;
  const heroSource = hero;
  hero = null;
  screen = "grid";
  destroyUiChrome();

  const slots = await buildGrid(Math.floor(Math.random() * PHOTOS.length));
  if (screen !== "grid") {
    for (const s of slots) {
      s.handle.destroy();
      s.full.destroy();
    }
    return;
  }
  wireGridTiles(slots);

  heroSource.splitTo(
    slots.map((s) => s.handle),
    TRANSITION,
    { kind: "gridSlice", cols: COLS, rows: ROWS },
  );
  destroySoon(heroSource);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

async function main(proteusApp: ProteusApp) {
  app = proteusApp;

  const canvas = document.getElementById("app") as HTMLCanvasElement;
  const rect = canvas.getBoundingClientRect();
  vw = rect.width;
  vh = rect.height;

  app.component({
    geometry: geom(vw / 2, vh / 2, vw, vh, BG_COLOR, 0),
    nonInteractive: true,
  });

  app.component({
    geometry: geom(vw / 2, TITLE_H / 2, 240, 34, BG_COLOR, 0),
    nonInteractive: true,
    text: { content: "Gallery", sizePx: 26, color: TEXT_COLOR },
  });

  const slots = await buildGrid(0);
  wireGridTiles(slots);

  console.log("[example] ready");
}

await mount("app", {
  setup(proteusApp) {
    void main(proteusApp);
  },
});
