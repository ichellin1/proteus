# Proteus Benchmarks

This document records performance measurements and methodology for Proteus's core architectural claim: **the cost of the WASM→GPU boundary does not scale with component count**.

---

## The claim

A traditional WebGL2 UI renderer issues one draw call per component. At N components that means N JavaScript→GPU boundary crossings per frame. As N grows, so does the overhead — O(N).

Proteus uses a single instanced draw call regardless of how many components are visible:

1. `upload_instances(buffer)` — one CPU→GPU transfer of the full instance buffer
2. `draw_indexed(0..6, 0, 0..N)` — one draw call, GPU handles the fan-out

The WASM→JS boundary cost is therefore O(1) per frame. The GPU work scales with N, but that scaling happens entirely on the GPU where it is cheap — there is no per-component JS overhead.

---

## M1 benchmark: instanced WASM vs pure TypeScript/WebGL2

### What is measured

Frame time (ms) as a function of component count N, rendering a scene of N colored rounded rectangles with randomized positions, sizes, and colors. Both implementations render visually equivalent output.

| Implementation | Draw calls / frame | WASM boundary crossings / frame |
|---|---|---|
| Proteus (WASM + instanced) | 1 | 1 |
| Baseline (pure TS / WebGL2) | N | 0 (JS native) |

The baseline has zero WASM crossings (it is pure JS), so any difference in frame time at low N is WASM startup amortization. At high N, the instanced approach should win decisively because N draw calls become the dominant cost.

### Methodology

**Scene**: N quads, each with:
- Random position within a 1280×800 viewport
- Random size 20–200px
- Random RGBA color
- Corner radius 8px
- No textures (white-pixel atlas entry, color tint only)

**Measurement**:
- Warm up: 60 frames discarded before measurement begins
- Sample: 300 frames recorded via `performance.now()` around the render call
- Report: median frame time and p99 frame time (median is robust to GC pauses; p99 reveals worst-case behavior)
- Hardware: record CPU model, GPU model, browser, and browser version with each result

**Component counts**: 100, 500, 1000, 2500, 5000, 10 000

**Environment**: Chrome (latest stable) on macOS, headless Chrome on Ubuntu CI

### Baseline implementation

The JS baseline renders each quad as a separate `drawElements` call with per-component uniform uploads:

```typescript
for (const quad of quads) {
  gl.uniform4fv(colorLoc, quad.color);
  gl.uniform2fv(posLoc, quad.position);
  gl.uniform2fv(sizeLoc, quad.size);
  gl.drawElements(gl.TRIANGLES, 6, gl.UNSIGNED_SHORT, 0);
}
```

This is intentionally naïve — it represents the pattern that most hand-written WebGL2 UI renderers use, not a state-of-the-art batching implementation.

### Results

> ⏳ **Pending.** The web shell itself has run in-browser since well before this was last updated
> (M1–M9.8 are all complete — see [ROADMAP.md](./ROADMAP.md)). What's actually missing is the
> *harness*: a JS baseline implementation to compare against, and ideally the TypeScript SDK
> (M12) so the comparison uses the same public API a real developer would. Results will be
> recorded here once both exist.

#### Median frame time (ms) — Chrome, Apple M-series

| N components | Proteus (WASM) | Baseline (TS/WebGL2) | Speedup |
|---|---|---|---|
| 100 | TBD | TBD | TBD |
| 500 | TBD | TBD | TBD |
| 1 000 | TBD | TBD | TBD |
| 2 500 | TBD | TBD | TBD |
| 5 000 | TBD | TBD | TBD |
| 10 000 | TBD | TBD | TBD |

#### p99 frame time (ms) — Chrome, Apple M-series

| N components | Proteus (WASM) | Baseline (TS/WebGL2) |
|---|---|---|
| 100 | TBD | TBD |
| 1 000 | TBD | TBD |
| 10 000 | TBD | TBD |

---

## M13 benchmark: native performance

> ⏳ **Pending** — M13 (Developer Release), which carries the native performance benchmark
> requirement in its Definition of Done (moved there when M11 — Native Parity was retired as its
> own milestone; see [PLANNING.md](./PLANNING.md)).

Instanced rendering on native (Metal / Vulkan / DX12) with bevy_ecs driving the scene graph. Measures frames per second at component counts up to 100 000, and GPU time via `wgpu::QuerySet` timestamp queries.

---

## How to run (once implemented)

```sh
# Rust CPU-side microbenchmarks (criterion)
cargo bench -p proteus-render

# Browser benchmark harness (needs M12 — TypeScript SDK)
cd benches/browser
npm install
npm run bench
```

---

## CPU-side microbenchmarks (Rust / criterion)

These measure the pure-CPU cost of preparing the instance buffer — independent of GPU or WASM.

> ⏳ **Pending** — criterion harness to be added under `crates/proteus-render/benches/`.

Planned measurements:
- `pack_instances`: time to zero-initialize and fill N `QuadInstance` structs
- `upload_instances`: time for `queue.write_buffer` at N instances (measures PCIe/unified-memory throughput)
- `ortho`: cost of constructing the orthographic matrix (should be negligible)
