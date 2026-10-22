/**
 * The browser benchmark: Proteus against the other common ways to draw on the
 * web, at growing component counts. Open the page, click Run, and copy the
 * results.
 *
 * What's measured is the time between animation frames, the frame rate a
 * user sees. A browser caps that at the display's refresh rate, which the
 * page measures first: a renderer at the cap is keeping up, and the counts
 * where each falls below it are what to compare. Three scenes, in
 * `scene.ts`: small and large components, still, which every renderer but
 * CSS redraws every frame; and moving, where every component is always
 * moving, in `motion.ts`, which CSS joins.
 *
 * Proteus runs twice: through the TypeScript SDK, its frames timed from
 * `mount`'s `update` function, which runs once a frame; and as an app written
 * in Rust, `../rust`, which reports its frames itself. Both draw with the
 * same web host and renderer; the Rust app makes its calls without crossing
 * from JavaScript. Each of the others has its own canvas, or for CSS a `div`,
 * of the same size.
 */

import { mount, type Handle, type ProteusApp } from "proteus-sdk";

// The same app in Rust, compiled to WebAssembly: `../rust`. `make bench-browser` builds it.
import * as rust from "../rust/pkg/proteus_bench_web";

import { batched, canvas2d, type MakeRenderer, naive } from "./baselines";
import { cssScene } from "./css";
import { Motion } from "./motion";
import { HEIGHT, scene, SCENES, type SceneKind, WIDTH } from "./scene";

/** The counts measured, unless the page's address gives others: `?counts=100,5000`. Past
 * 100,000, Proteus needs much more GPU memory: 160 bytes a component, twice. */
const COUNTS = [100, 1_000, 5_000, 10_000, 25_000, 50_000, 100_000];
/** Frames run before timing starts at each count, or this many milliseconds if that's sooner. */
const WARM_UP = 60;
const WARM_UP_LIMIT = 2_000;
/**
 * The large scene's largest count. Past it, the large scene only repeats what
 * 50,000 shows, Proteus's per-pixel cost, while each frame takes long enough
 * that Safari resets its GPU process and the tab crashes.
 */
const LARGE_LIMIT = 50_000;
/**
 * A renderer slower than this, in milliseconds a frame, is skipped at the
 * scene's larger counts: below 10 frames a second it's unusable, and larger
 * counts would take minutes and risk crashing the tab.
 */
const SLOW_LIMIT = 100;
/**
 * Where the run's progress is kept, so a crashed tab can resume. Kept apart
 * for each path: the published page and every staging preview share an origin.
 */
const STORAGE_KEY = `proteus-bench:${location.pathname}`;
/** The most frames timed at each count. */
const FRAMES = 300;
/** The most time spent timing one count, in milliseconds: the naive renderer gets slow. */
const TIME_LIMIT = 15_000;

const status = document.getElementById("status")!;
const output = document.getElementById("results")!;
const runButton = document.getElementById("run") as HTMLButtonElement;
const resetButton = document.getElementById("reset") as HTMLButtonElement;
const surfaces = {
  proteus: document.getElementById("proteus") as HTMLCanvasElement,
  proteusRust: document.getElementById("proteus-rust") as HTMLCanvasElement,
  webgl: document.getElementById("webgl") as HTMLCanvasElement,
  canvas2d: document.getElementById("canvas2d") as HTMLCanvasElement,
  css: document.getElementById("css")!,
};

const params = new URLSearchParams(location.search);
const counts =
  params
    .get("counts")
    ?.split(",")
    .map(Number)
    .filter((n) => n > 0) ?? COUNTS;
/** The scenes run, unless the address names some: `?scenes=small`. */
const scenes = (params.get("scenes")?.split(",") ?? ["small", "large", "moving"]).filter(
  (kind): kind is SceneKind => kind in SCENES,
);

// --- What Proteus logs at startup: the adapter, and so WebGPU or WebGL2 ----

let proteusAdapter = "unknown";
for (const level of ["log", "info"] as const) {
  const original = console[level].bind(console);
  console[level] = (...args: unknown[]) => {
    // The SDK's logger styles its messages with `%c` and CSS arguments;
    // only the first argument is the message.
    const text = String(args[0] ?? "").replace(/%c/g, "");
    const match = text.match(/proteus-host-web: adapter (.*)/);
    if (match) {
      const backend = match[1].match(/\((\w+)\)\s*$/)?.[1] ?? "";
      const name = match[1].replace(/\(\w+\)\s*$/, "").trim();
      const api = /WebGpu/i.test(backend) ? "WebGPU" : /Gl/i.test(backend) ? "WebGL2" : backend;
      proteusAdapter = `${api}${name ? `, ${name}` : " (the browser doesn't name the GPU)"}`;
    }
    original(...args);
  };
}

// --- Timing frames ------------------------------------------------------------

interface Timing {
  median: number;
  p99: number;
  frames: number;
}

function summarize(intervals: number[]): Timing {
  const sorted = [...intervals].sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.round((sorted.length - 1) * q)];
  return { median: at(0.5), p99: at(0.99), frames: sorted.length };
}

/**
 * Calls `onFrame` once a frame, through `subscribe`, and resolves with the
 * timing of the frames after the warm-up.
 */
function timeFrames(subscribe: (onFrame: (now: number) => void) => () => void): Promise<Timing> {
  return new Promise((resolve) => {
    const intervals: number[] = [];
    let seen = 0;
    let last = 0;
    let started = 0;
    let first = 0;
    let warm = false;
    const unsubscribe = subscribe((now) => {
      seen += 1;
      if (seen === 1) first = now;
      if (!warm && (seen >= WARM_UP || now - first > WARM_UP_LIMIT)) {
        warm = true;
        started = now;
        last = now;
        return;
      }
      if (warm) intervals.push(now - last);
      last = now;
      if (warm && (intervals.length >= FRAMES || now - started > TIME_LIMIT)) {
        unsubscribe();
        resolve(summarize(intervals));
      }
    });
  });
}

/** Frames from `requestAnimationFrame`, calling `draw` in each. */
function animationFrames(draw: (now: number) => void) {
  return (onFrame: (now: number) => void) => {
    let running = true;
    const tick = (now: number) => {
      if (!running) return;
      draw(now);
      onFrame(now);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return () => {
      running = false;
    };
  };
}

// --- Proteus ----------------------------------------------------------------

/** Room for the largest scene asked for: Proteus draws at most 4096 a frame by default. */
const maxInstances = Math.max(4096, 2 ** Math.ceil(Math.log2(Math.max(...counts))));

let proteus: ProteusApp | undefined;
/** Called from `mount`'s `update`, once a frame, while Proteus is being timed. */
let proteusFrame: ((now: number) => void) | undefined;

await mount("proteus", {
  setup(app) {
    proteus = app;
  },
  update() {
    proteusFrame?.(performance.now());
  },
  config: {
    render: { clearColor: [0, 0, 0, 1] },
    // Room for the largest scene: the default draws at most 4096 a frame.
    memory: { maxInstances },
    // Draw every frame, as the other renderers do. A still scene would
    // otherwise be drawn once, and its frame rate would mean nothing.
    frame: { skipIdleFrames: false },
  },
});
await rust.start("proteus-rust", maxInstances);

async function timeProteusRust(kind: SceneKind, count: number): Promise<Timing> {
  rust.show(kind, count);
  const timing = await timeFrames((onFrame) => {
    rust.on_frame(onFrame);
    return () => rust.stop_frames();
  });
  rust.clear();
  return timing;
}

async function timeProteus(kind: SceneKind, count: number): Promise<Timing> {
  const app = proteus!;
  const rects = scene(kind, count);
  // Proteus measures from the center of the canvas, with y up.
  const geometry = (x: number, y: number, i: number) => ({
    position: { x: x - WIDTH / 2, y: HEIGHT / 2 - y, z: 0 },
    size: { width: rects[i].width, height: rects[i].height },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: { r: rects[i].r, g: rects[i].g, b: rects[i].b, a: 1 },
    cornerRadius: SCENES[kind].radius,
  });
  const handles: Handle[] = rects.map((r, i) => app.component({ geometry: geometry(r.x, r.y, i) }));
  const motion = kind === "moving" ? new Motion(rects) : undefined;
  const timing = await timeFrames((onFrame) => {
    proteusFrame = (now) => {
      // A new move for every component, every half second, which Proteus
      // animates itself.
      const targets = motion?.retarget(now);
      if (targets) {
        handles.forEach((handle, i) =>
          handle.animateTo(geometry(targets[i * 2], targets[i * 2 + 1], i), {
            duration: 0.5,
            easing: "easeInOutQuad",
          }),
        );
      }
      onFrame(now);
    };
    return () => {
      proteusFrame = undefined;
    };
  });
  for (const handle of handles) handle.destroy();
  return timing;
}

// --- The others ---------------------------------------------------------------

const density = window.devicePixelRatio;
for (const canvas of [surfaces.webgl, surfaces.canvas2d]) {
  canvas.width = WIDTH * density;
  canvas.height = HEIGHT * density;
}
const gl = surfaces.webgl.getContext("webgl2", { antialias: false })!;
const context = surfaces.canvas2d.getContext("2d")!;
context.scale(density, density);

/** Times a renderer that redraws every frame, at fixed positions or moving ones. */
async function timeRedrawn(make: MakeRenderer, kind: SceneKind, count: number): Promise<Timing> {
  const rects = scene(kind, count);
  const motion = new Motion(rects);
  const renderer = make(rects, motion.current, SCENES[kind].radius);
  const timing = await timeFrames(
    animationFrames((now) => {
      if (kind === "moving") {
        motion.retarget(now);
        motion.advance(now);
      }
      renderer.draw();
    }),
  );
  renderer.dispose();
  return timing;
}

async function timeCss(count: number): Promise<Timing> {
  const css = cssScene(surfaces.css, scene("moving", count), SCENES.moving.radius);
  const timing = await timeFrames(animationFrames((now) => css.frame(now)));
  css.dispose();
  return timing;
}

// --- The run ------------------------------------------------------------------

type Surface = keyof typeof surfaces;

function show(which: Surface) {
  for (const [name, element] of Object.entries(surfaces)) {
    element.style.visibility = name === which ? "visible" : "hidden";
  }
}

/** Each renderer: its column title, the surface it draws on, and how to time it. */
interface Contender {
  title: string;
  surface: Surface;
  time: (kind: SceneKind, count: number) => Promise<Timing>;
}

const PROTEUS: Contender = { title: "Proteus (TypeScript)", surface: "proteus", time: timeProteus };
const PROTEUS_RUST: Contender = {
  title: "Proteus (Rust)",
  surface: "proteusRust",
  time: timeProteusRust,
};
const CSS: Contender = { title: "CSS", surface: "css", time: (_, count) => timeCss(count) };
const CANVAS_2D: Contender = {
  title: "Canvas 2D",
  surface: "canvas2d",
  time: (kind, count) => timeRedrawn(canvas2d(context), kind, count),
};
const NAIVE: Contender = {
  title: "Naive WebGL2",
  surface: "webgl",
  time: (kind, count) => timeRedrawn(naive(gl), kind, count),
};
const BATCHED: Contender = {
  title: "Batched WebGL2",
  surface: "webgl",
  time: (kind, count) => timeRedrawn(batched(gl), kind, count),
};

/** The renderers in each scene: CSS only moves, since a still page of `div`s costs nothing. */
const CONTENDERS: Record<SceneKind, Contender[]> = {
  small: [PROTEUS, PROTEUS_RUST, CANVAS_2D, NAIVE, BATCHED],
  large: [PROTEUS, PROTEUS_RUST, CANVAS_2D, NAIVE, BATCHED],
  moving: [PROTEUS, PROTEUS_RUST, CSS, CANVAS_2D, NAIVE, BATCHED],
};
const LIMITS: Record<SceneKind, number> = {
  small: Infinity,
  large: LARGE_LIMIT,
  moving: Infinity,
};
const TITLES: Record<SceneKind, string> = {
  small: `Small components (${SCENES.small.min}–${SCENES.small.max} px), still: the cost per component`,
  large: `Large components (${SCENES.large.min}–${SCENES.large.max} px), still: filling pixels dominates`,
  moving: `Moving components (${SCENES.moving.min}–${SCENES.moving.max} px): every one always moving`,
};

function environment(refresh: number): string {
  const debug = gl.getExtension("WEBGL_debug_renderer_info");
  const gpu = debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
  return [
    `- Browser: ${navigator.userAgent}`,
    `- GPU: ${gpu}`,
    `- Proteus's adapter: ${proteusAdapter}`,
    `- Device pixel ratio: ${density}; canvas ${WIDTH} × ${HEIGHT} CSS pixels`,
    `- Refresh rate: about ${(1000 / refresh).toFixed(0)} Hz (a frame every ${refresh.toFixed(2)} ms)`,
  ].join("\n");
}

// --- Progress, kept across a crash ---------------------------------------------

/**
 * The run so far. It's saved after every measurement, and before each one
 * starts, it records which: if the tab crashes, the page finds that on
 * reload, marks it "crashed", and resumes after it.
 */
interface Progress {
  /** The page's address settings the run is for. */
  settings: string;
  refresh?: number;
  /** Each measurement's cell, by `scene|count|renderer`. */
  cells: Record<string, string>;
  /** Each renderer left out of the rest of a scene, by `scene|renderer`, and why. */
  dropped: Record<string, string>;
  /** The measurement under way, if any. */
  running?: string;
}

function load(): Progress {
  const fresh: Progress = { settings: location.search, cells: {}, dropped: {} };
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Progress | null;
    return saved && saved.settings === location.search ? saved : fresh;
  } catch {
    return fresh;
  }
}

function save() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(progress));
  } catch {
    // Without storage, a crash loses the run, but it still runs.
  }
}

const progress = load();
if (progress.running) {
  // The tab crashed during this measurement.
  const [kind, count, title] = progress.running.split("|");
  progress.cells[progress.running] = "crashed | —";
  progress.dropped[`${kind}|${title}`] = `crashed at ${count}`;
  progress.running = undefined;
  save();
}

const ms = (t: Timing) => `${t.median.toFixed(2)} | ${t.p99.toFixed(2)}`;

/** The measurements of `kind` still to make, in order. */
function remaining(kind: SceneKind): [number, Contender][] {
  return counts
    .filter((n) => n <= LIMITS[kind])
    .flatMap((count) => CONTENDERS[kind].map((c): [number, Contender] => [count, c]))
    .filter(([count, c]) => progress.cells[`${kind}|${count}|${c.title}`] === undefined);
}

async function run() {
  runButton.disabled = true;
  resetButton.disabled = true;
  if (progress.refresh === undefined) {
    show("webgl");
    status.textContent = "Measuring the refresh rate…";
    progress.refresh = (await timeFrames(animationFrames(() => {}))).median;
    save();
  }

  for (const kind of scenes) {
    for (const [count, contender] of remaining(kind)) {
      const cell = `${kind}|${count}|${contender.title}`;
      const dropped = progress.dropped[`${kind}|${contender.title}`];
      if (dropped) {
        progress.cells[cell] = "skipped | —";
        continue;
      }
      show(contender.surface);
      status.textContent = `${kind} scene, ${count} components: ${contender.title}…`;
      progress.running = cell;
      save();
      try {
        const timing = await contender.time(kind, count);
        progress.cells[cell] = ms(timing);
        if (timing.median >= SLOW_LIMIT) {
          progress.dropped[`${kind}|${contender.title}`] =
            `slower than ${SLOW_LIMIT} ms a frame at ${count}`;
        }
      } catch (error) {
        console.error(error);
        progress.cells[cell] = "failed | —";
        progress.dropped[`${kind}|${contender.title}`] = `failed at ${count}: ${String(error)}`;
      }
      progress.running = undefined;
      save();
      output.textContent = report();
    }
  }
  status.textContent = "Done. Copy the results below.";
  runButton.textContent = "Run";
  runButton.disabled = false;
  resetButton.disabled = false;
}

function report(): string {
  const lines = [environment(progress.refresh ?? 0)];
  for (const kind of scenes) {
    const titles = CONTENDERS[kind].map((c) => c.title);
    const rows = counts
      .filter((n) => n <= LIMITS[kind])
      .map((count) => titles.map((title) => progress.cells[`${kind}|${count}|${title}`]))
      .map((cells, i) => ({ count: counts.filter((n) => n <= LIMITS[kind])[i], cells }))
      .filter(({ cells }) => cells.some((c) => c !== undefined));
    if (rows.length === 0) continue;
    lines.push(
      "",
      `### ${TITLES[kind]}`,
      "",
      "Frame interval, median and p99, in milliseconds:",
      "",
      `| Components | ${titles.map((t) => `${t} | p99`).join(" | ")} |`,
      `|---:|${titles.map(() => "---:|---:").join("|")}|`,
      ...rows.map(({ count, cells }) => `| ${count} | ${cells.map((c) => c ?? "… | …").join(" | ")} |`),
    );
    const notes = titles
      .map((title) => [title, progress.dropped[`${kind}|${title}`]])
      .filter(([, why]) => why)
      .map(([title, why]) => `- ${title}: ${why}; skipped after that.`);
    if (notes.length > 0) lines.push("", ...notes);
    if (kind === "large" && counts.some((n) => n > LARGE_LIMIT)) {
      lines.push("", `The large scene stops at ${LARGE_LIMIT}: past it, Safari's tab crashes.`);
    }
  }
  return lines.join("\n");
}

runButton.addEventListener("click", () => void run());
resetButton.addEventListener("click", () => {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing saved to remove.
  }
  location.reload();
});

const started = Object.keys(progress.cells).length > 0;
const unfinished = scenes.some((kind) => remaining(kind).length > 0);
if (started) output.textContent = report();
runButton.textContent = started && unfinished ? "Resume" : "Run";
status.textContent = started
  ? unfinished
    ? "A run was under way, and is saved: Resume carries on from where it stopped."
    : "The last run is complete; its results are below. Start over to run again."
  : "Ready. Keep this tab in front while it runs: a hidden tab stops drawing.";
runButton.disabled = started && !unfinished;
resetButton.disabled = !started;
