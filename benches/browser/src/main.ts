/**
 * The browser benchmark: Proteus against two renderers written by hand with
 * WebGL2, drawing the same scene at growing component counts. Open the page,
 * click Run, and copy the results.
 *
 * Each renderer redraws the whole scene every frame. What's measured is the
 * time between animation frames, the frame rate a user sees. A browser caps
 * that at the display's refresh rate, which the page measures first: a
 * renderer at the cap is keeping up, and the counts where each falls below it
 * are what to compare.
 *
 * Proteus runs through the TypeScript SDK, on its own canvas; its frames are
 * timed from `mount`'s `update` function, which runs once a frame. The
 * hand-written renderers share a second canvas of the same size.
 */

import { mount, type Handle, type ProteusApp } from "proteus-sdk";

import { batched, naive, type Renderer } from "./baselines";
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
/** The most frames timed at each count. */
const FRAMES = 300;
/** The most time spent timing one count, in milliseconds: the naive renderer gets slow. */
const TIME_LIMIT = 15_000;

const status = document.getElementById("status")!;
const output = document.getElementById("results")!;
const runButton = document.getElementById("run") as HTMLButtonElement;
const proteusCanvas = document.getElementById("proteus") as HTMLCanvasElement;
const baselineCanvas = document.getElementById("baseline") as HTMLCanvasElement;

const params = new URLSearchParams(location.search);
const counts =
  params
    .get("counts")
    ?.split(",")
    .map(Number)
    .filter((n) => n > 0) ?? COUNTS;
/** The scenes run, unless the address names some: `?scenes=small`. */
const scenes = (params.get("scenes")?.split(",") ?? ["small", "large"]).filter(
  (kind): kind is SceneKind => kind === "small" || kind === "large",
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
function animationFrames(draw: () => void) {
  return (onFrame: (now: number) => void) => {
    let running = true;
    const tick = (now: number) => {
      if (!running) return;
      draw();
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
    memory: { maxInstances: Math.max(4096, 2 ** Math.ceil(Math.log2(Math.max(...counts)))) },
  },
});

async function timeProteus(kind: SceneKind, count: number): Promise<Timing> {
  const app = proteus!;
  // Proteus measures from the center of the canvas, with y up.
  const handles: Handle[] = scene(kind, count).map((r) =>
    app.component({
      geometry: {
        position: { x: r.x - WIDTH / 2, y: HEIGHT / 2 - r.y, z: 0 },
        size: { width: r.width, height: r.height },
        rotation: 0,
        scale: 1,
        anchor: { x: 0.5, y: 0.5 },
        color: { r: r.r, g: r.g, b: r.b, a: 1 },
        cornerRadius: SCENES[kind].radius,
      },
    }),
  );
  const timing = await timeFrames((onFrame) => {
    proteusFrame = onFrame;
    return () => {
      proteusFrame = undefined;
    };
  });
  for (const handle of handles) handle.destroy();
  return timing;
}

// --- The hand-written renderers -----------------------------------------------

const density = window.devicePixelRatio;
baselineCanvas.width = WIDTH * density;
baselineCanvas.height = HEIGHT * density;
const gl = baselineCanvas.getContext("webgl2", { antialias: false })!;

async function timeBaseline(make: typeof naive, kind: SceneKind, count: number): Promise<Timing> {
  const renderer: Renderer = make(gl, scene(kind, count), SCENES[kind].radius);
  const timing = await timeFrames(animationFrames(() => renderer.draw()));
  renderer.dispose();
  return timing;
}

// --- The run ------------------------------------------------------------------

function show(which: "proteus" | "baseline") {
  proteusCanvas.style.visibility = which === "proteus" ? "visible" : "hidden";
  baselineCanvas.style.visibility = which === "baseline" ? "visible" : "hidden";
}

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

const ms = (t: Timing) => `${t.median.toFixed(2)} | ${t.p99.toFixed(2)}`;

async function run() {
  runButton.disabled = true;
  output.textContent = "";
  show("baseline");
  status.textContent = "Measuring the refresh rate…";
  const refresh = (await timeFrames(animationFrames(() => {}))).median;

  const tables: Record<SceneKind, string[]> = { large: [], small: [] };
  for (const kind of scenes) {
    for (const count of counts.filter((n) => kind === "small" || n <= LARGE_LIMIT)) {
      show("proteus");
      status.textContent = `${kind} scene, ${count} components: Proteus…`;
      const p = await timeProteus(kind, count);
      show("baseline");
      status.textContent = `${kind} scene, ${count} components: naive WebGL2…`;
      const n = await timeBaseline(naive, kind, count);
      status.textContent = `${kind} scene, ${count} components: batched WebGL2…`;
      const b = await timeBaseline(batched, kind, count);
      tables[kind].push(`| ${count} | ${ms(p)} | ${ms(n)} | ${ms(b)} |`);
      output.textContent = report(refresh, tables);
    }
  }
  status.textContent = "Done. Copy the results below.";
  runButton.disabled = false;
}

function report(refresh: number, tables: Record<SceneKind, string[]>): string {
  const table = (kind: SceneKind, title: string) =>
    tables[kind].length === 0
      ? []
      : [
          "",
          `### ${title}`,
          "",
          "Frame interval, median and p99, in milliseconds:",
          "",
          "| Components | Proteus | p99 | Naive WebGL2 | p99 | Batched WebGL2 | p99 |",
          "|---:|---:|---:|---:|---:|---:|---:|",
          ...tables[kind],
        ];
  return [
    environment(refresh),
    ...table("small", `Small components (${SCENES.small.min}–${SCENES.small.max} px): the cost per component`),
    ...table("large", `Large components (${SCENES.large.min}–${SCENES.large.max} px): filling pixels dominates`),
    ...(tables.large.length > 0 && counts.some((n) => n > LARGE_LIMIT)
      ? ["", `The large scene stops at ${LARGE_LIMIT}: past it, Safari's tab crashes.`]
      : []),
  ].join("\n");
}

runButton.addEventListener("click", () => void run());
status.textContent = "Ready. Keep this tab in front while it runs: a hidden tab stops drawing.";
runButton.disabled = false;
