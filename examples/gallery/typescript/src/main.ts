/**
 * Proteus TypeScript SDK example: an image gallery.
 *
 * A grid of photos fetched from picsum.photos. Clicking a tile transitions
 * it into a large "hero" view with `TransitionChannel.set`; going back splits the
 * hero into a fresh grid with `Handle.splitTo`.
 *
 * Techniques worth copying:
 *   - Fetch each batch of images concurrently (`Promise.all`), not with one
 *     `await` per loop iteration. Sequential fetches are slower, and leave
 *     each new tile fully visible in its final spot before the transition
 *     that should reveal it runs.
 *   - Give the hero an image *before* the tile → hero transition starts.
 *     A hero spawned blank shows an empty box that grows, and the image
 *     pops in only when the hi-res fetch lands. `setTexture` works on a
 *     component without an image, so the hero can start with the tile's
 *     photo and scale it continuously with the box.
 *   - Start the hero from the photo's *uncropped* low-res image. The grid
 *     tile shows the same texture cropped to a square, and starting the hero
 *     from that framing would differ from the hi-res photo: mid-crossfade,
 *     two differently framed copies of the photo would blend together.
 *     Cropping changes the component, not the texture, so `GridSlot.full`
 *     keeps the uncropped texture for the hero.
 *   - Crossfade to the hi-res image with a second, transparent overlay
 *     component that fades in with `animateTo` on `color.a`. There's no
 *     built-in crossfade between two images on one component, and swapping
 *     the texture on the hero would be a hard cut.
 *   - Request the tile and the hi-res photo at *exactly* the same aspect
 *     ratio. picsum.photos crops its source to the requested size, so two
 *     slightly different ratios crop differently, and the photo shifts
 *     sideways when the crossfade swaps them. Rounding one side from the
 *     other isn't precise enough for some photos, so `fetchDimensions`
 *     searches nearby sizes for the pair closest to the photo's real ratio.
 *   - Load an image straight into a texture with `app.loadTexture`, and
 *     react to a finished transition with `onTransitionComplete` rather
 *     than a timer.
 *
 *   - While the new grid downloads, a loader over the large view shows that
 *     something is happening. Its text is left-aligned with the anchor, so
 *     the words stay still as the arrow after them grows.
 *
 * To keep the example short, the layout is computed once from the canvas
 * size at load and doesn't reflow when the window is resized.
 */

import { mount, colorFrom, topLeftToWorld } from "proteus-sdk";
import type {
  ProteusApp,
  Geometry,
  Color,
  Handle,
  TextureHandle,
} from "proteus-sdk";

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
/** Whether the transition into the large view has finished. */
let heroOpened = false;

let backBtn: Handle | null = null;
let caption: Handle | null = null;

// ---------------------------------------------------------------------------
// Layout + fetch helpers
// ---------------------------------------------------------------------------

function geom(cx: number, cy: number, w: number, h: number, color: Color = CARD_COLOR, cornerRadius = 12): Geometry {
  const pos = topLeftToWorld(cx, cy, vw, vh);
  return {
    position: { x: pos.x, y: pos.y, z: 0 },
    size: { width: w, height: h },
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


/**
 * Run `fn` the first time a transition targeting `handle` completes.
 *
 * `onTransitionComplete` is persistent — it keeps firing for later
 * transitions on the same component — so anything that should happen once
 * (destroying the thing that was transitioned away from, say) guards itself.
 */
function afterTransition(handle: Handle, fn: () => void) {
  let done = false;
  handle.onTransitionComplete(() => {
    if (done) return;
    done = true;
    fn();
  });
}

/** {@link afterTransition} as a promise, for `await`-shaped code. */
function whenSettled(handle: Handle): Promise<void> {
  return new Promise((resolve) => afterTransition(handle, resolve));
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
   * The same low-res fetch as a texture, *uncropped*. `handle` wears this
   * too, but square-crops its own copy of the UVs in place, so it no longer
   * matches the photo's real framing. `enterDetail` starts the hero from
   * this instead, so it is correctly framed (just low-res) rather than
   * mismatched against the hi-res fetch that later crossfades in.
   *
   * A texture, not an off-screen component: cropping edits the *entity's*
   * UVs, never the texture, so one texture can back both the cropped tile
   * and the uncropped hero.
   */
  full: TextureHandle;
  photo: Photo;
}

interface DownloadedPhoto {
  photo: Photo;
  bytes: Uint8Array;
}

/** Downloads a grid's photos, all at once. */
async function downloadGrid(offset: number): Promise<DownloadedPhoto[]> {
  return Promise.all(
    pickPhotos(COLS * ROWS, offset).map(async (photo) => {
      const [w, h] = fetchDimensions(photo.width, photo.height, TILE * 2);
      return { photo, bytes: await fetchImageBytes(photoUrl(photo.id, w, h)) };
    }),
  );
}

/**
 * Builds the grid from downloaded photos. Each photo is loaded as a texture
 * here, and given to its tile at once: a texture must be given to a component
 * in the frame it's loaded.
 */
function buildGrid(downloaded: DownloadedPhoto[]): GridSlot[] {
  const startX = (vw - (COLS * TILE + (COLS - 1) * GAP)) / 2;
  const startY = TITLE_H + (vh - TITLE_H - (ROWS * TILE + (ROWS - 1) * GAP)) / 2;

  return downloaded.map(({ photo, bytes }, i) => {
    const col = i % COLS;
    const row = Math.floor(i / COLS);
    const cx = startX + col * (TILE + GAP) + TILE / 2;
    const cy = startY + row * (TILE + GAP) + TILE / 2;

    const full = app.loadTexture(bytes);
    if (!full) {
      throw new Error(`could not decode photo ${photo.id}`);
    }
    const handle = app.component({ geometry: geom(cx, cy, TILE, TILE), hover: { scale: 1.06 } });
    handle.setTexture(full);
    handle.cropImage({ kind: "centeredSquare" });

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
    }
  }
  gridSlots = [];

  await enterDetail(slot.photo, slot.full, (heroHandle) => {
    const channel = app.transitionChannel();
    channel.set(heroHandle, slot.handle, TRANSITION);
    // `set` reports completion on its `to` side — the hero — at which point
    // the tile it grew out of has finished being the exit and can go.
    afterTransition(heroHandle, () => slot.handle.destroy());
  });
}

/**
 * Shared hero-construction: spawns the hero already carrying `lowResSource`'s
 * baked image (the clicked tile's *uncropped* low-res fetch — see
 * `GridSlot.full`'s doc for why not the tile itself) — so `startTransition`'s
 * geometry transition scales that image up continuously instead of showing
 * a blank box — then, once both the hi-res fetch and the transition have
 * settled, cross-fades in the hi-res image via a transparent overlay rather than
 * swapping it in as a hard cut. The hero itself only gets the hi-res image
 * once the overlay has fully faded to opaque (so a later `splitTo`/
 * `mergeFrom` off this handle carries the sharp image, not the low-res one).
 */
async function enterDetail(
  photo: Photo,
  lowRes: TextureHandle,
  startTransition: (hero: Handle) => void,
) {
  const box = fitBox(photo.width, photo.height, vw * 0.7, vh - TITLE_H - 140);
  const heroHandle = app.component({
    geometry: geom(vw / 2, TITLE_H + (vh - TITLE_H) / 2, box.w, box.h, CARD_COLOR, 16),
  });
  heroHandle.setTexture(lowRes);
  startTransition(heroHandle);

  hero = heroHandle;
  heroOpened = false;
  void whenSettled(heroHandle).then(() => {
    if (hero === heroHandle) {
      heroOpened = true;
    }
  });
  buildDetailUi(photo.id);

  // Both the hi-res fetch and the transition have to finish before the
  // crossfade starts: the fetch so there is something to fade in, the
  // transition so it isn't fading in over a box that is still moving. This
  // waits for the real completion rather than a timer set to the
  // transition's duration.
  const [w, h] = fetchDimensions(photo.width, photo.height, 900);
  const [hiResBytes] = await Promise.all([
    fetchImageBytes(photoUrl(photo.id, w, h)),
    whenSettled(heroHandle),
  ]);
  if (hero !== heroHandle) return;

  // Loaded only now, as it's given to the overlay below: a texture must be
  // given to a component in the frame it's loaded.
  const hiRes = app.loadTexture(hiResBytes);
  if (!hiRes) return;

  const heroData = heroHandle.get();
  if (!heroData) return;
  const overlay = app.component({
    geometry: { ...heroData.geometry, color: { ...heroData.geometry.color, a: 0 } },
    nonInteractive: true,
  });
  overlay.setTexture(hiRes);
  overlay.animateTo({ ...heroData.geometry, color: { ...heroData.geometry.color, a: 1 } }, CROSSFADE);

  afterTransition(overlay, () => {
    if (hero === heroHandle) {
      heroHandle.setTexture(hiRes);
    }
    overlay.destroy();
  });
}

// ---------------------------------------------------------------------------
// Back to grid (1→N)
// ---------------------------------------------------------------------------

async function backToGrid() {
  // Wait until the large view has opened: it can't split while it's still
  // moving.
  if (screen !== "detail" || !hero || !heroOpened) return;
  const heroSource = hero;
  hero = null;
  screen = "grid";
  destroyUiChrome();

  // The loader shows that the new grid is downloading, and plays through at
  // least once.
  const playedOnce = showLoader(heroSource);
  const [downloaded] = await Promise.all([
    downloadGrid(Math.floor(Math.random() * PHOTOS.length)),
    playedOnce,
  ]);
  removeLoader();
  if (screen !== "grid") return;
  const slots = buildGrid(downloaded);
  wireGridTiles(slots);

  heroSource.splitTo(
    slots.map((s) => s.handle),
    TRANSITION,
    { kind: "grid", cols: COLS, rows: ROWS },
  );
  // A slicing split reports completion once, on the *source*, when every
  // target has arrived — so the hero tidies itself up the moment the last
  // tile lands.
  afterTransition(heroSource, () => heroSource.destroy());
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/** The loader's text, one after another, each for a quarter of a second. */
const LOADER_FRAMES = [
  "Fetching Images ->",
  "Fetching Images -->",
  "Fetching Images --->",
  "Fetching Images ---->",
];
const LOADER_FRAME_SECONDS = 0.25;
const LOADER_W = 250;
const LOADER_H = 44;
const LOADER_TEXT_SIZE = 18;

/**
 * "Fetching Images" over the large view, with an arrow that grows in steps,
 * so the wait for the new grid's photos doesn't look like the app has
 * stopped. `update` plays it.
 */
interface Loader {
  /** The dark box the text sits on. */
  panel: Handle;
  /** The text, a child of `panel`. */
  label: Handle;
  /** Seconds since the loader appeared. */
  elapsed: number;
  /** Which of `LOADER_FRAMES` is showing. */
  frame: number;
  /** Called once the loader has played through once. */
  onPlayedOnce: () => void;
}

let loader: Loader | null = null;

/**
 * Shows the loader in the middle of `view`. The promise resolves once it has
 * played through once.
 */
function showLoader(view: Handle): Promise<void> {
  const center = view.get()?.geometry.position ?? { x: 0, y: 0, z: 0 };
  const panel = app.component({
    geometry: {
      position: center,
      size: { width: LOADER_W, height: LOADER_H },
      rotation: 0,
      scale: 1,
      anchor: { x: 0.5, y: 0.5 },
      color: colorFrom("#00000099"),
      cornerRadius: LOADER_H / 2,
    },
    nonInteractive: true,
  });
  // The text is left-aligned, so the words stay still as the arrow grows: the
  // label's anchor, which places its text, is its left edge. A child is
  // placed from its parent's anchor, the panel's center.
  const label = app.component({
    geometry: {
      position: { x: -LOADER_W / 2 + 24, y: 0, z: 0 },
      size: { width: LOADER_W - 48, height: LOADER_H },
      rotation: 0,
      scale: 1,
      anchor: { x: 0, y: 0.5 },
      color: colorFrom("transparent"),
      cornerRadius: 0,
    },
    nonInteractive: true,
    text: { content: LOADER_FRAMES[0], sizePx: LOADER_TEXT_SIZE, color: CARD_COLOR },
  });
  panel.addChild(label);
  return new Promise((resolve) => {
    loader = { panel, label, elapsed: 0, frame: 0, onPlayedOnce: resolve };
  });
}

/** Moves the loader on by `dt` seconds. */
function advanceLoader(dt: number) {
  if (!loader) return;
  const before = loader.elapsed;
  loader.elapsed += dt;
  const once = LOADER_FRAMES.length * LOADER_FRAME_SECONDS;
  if (before < once && loader.elapsed >= once) {
    loader.onPlayedOnce();
  }
  const frame = Math.floor(loader.elapsed / LOADER_FRAME_SECONDS) % LOADER_FRAMES.length;
  if (frame !== loader.frame) {
    loader.frame = frame;
    loader.label.setText({
      content: LOADER_FRAMES[frame],
      sizePx: LOADER_TEXT_SIZE,
      color: CARD_COLOR,
    });
  }
}

function removeLoader() {
  // Destroying the panel destroys the label, its child, too.
  loader?.panel.destroy();
  loader = null;
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

  wireGridTiles(buildGrid(await downloadGrid(0)));

  console.log("[example] ready");
}

await mount("app", {
  setup(proteusApp) {
    void main(proteusApp);
  },
  update(deltaSeconds) {
    advanceLoader(deltaSeconds);
  },
});
