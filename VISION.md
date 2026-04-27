# Proteus — Vision Document

> *"He could take the form of any creature he chose."*
> — Homer, the Odyssey

---

## The Idea

Proteus is a new UX paradigm and the rendering framework that makes it possible.

The central idea: **UI components are metamorphic.** They do not navigate to new screens or swap out for different components — they *transform* into them. A button can become a list. A list can collapse back into a button. A list item can become a video player. The component does not disappear; it *shapeshifts*, and the transition between one form and another is a first-class, visually continuous experience.

This is the thing current frameworks cannot do — not cleanly, and not at a component level. Shared-element transitions exist, but they are cosmetic overlays on top of discrete navigation. Proteus makes metamorphism the core model, not a special effect layered on top.

---

## Metamorphic Components

Every UI element in a Proteus application is a **metamorphic component** — it has a current visual form, a set of possible target forms, and the ability to transition between them smoothly.

### Transition Topologies

Proteus supports three fundamental transition shapes:

**1 → 1**
One component transforms into one other component. The source geometry — its position, size, shape, color, and content — interpolates continuously into the target geometry. The user perceives a single object changing.

**1 → N**
One component splits into many. The source component is the shared origin point for all N targets: each target component begins at the source's geometry and animates outward to its own final position and size. A button becoming a list of five items is a 1→5 transition.

**N → 1**
Many components converge into one. All N source components animate toward the single target's geometry, collapsing into it. A list of five items becoming a button is a 5→1 transition. This is the exact inverse of the above and should feel like it.

---

## The Transition Model

### Interpolation First

All transitions are driven by **interpolation between two geometric states**: the "from" state (the current component or set of components) and the "to" state (the target component or set of components).

Every interpolatable property — position (x, y), size (width, height), shape (corner radii, path control points), color, opacity, content — is a value that can be lerped.

The initial implementation uses **linear interpolation (lerp)** as the foundation. This is intentional. A well-timed linear transition is already smooth and visually clear. It also establishes the contract: every transition is a parameterized walk from 0.0 to 1.0 between two states.

### Extensible Interpolation

Because transitions are parameterized at the framework level, the interpolation function is **pluggable**. Linear is the default, but any function that maps `t ∈ [0, 1] → [0, 1]` can be substituted:

- Standard easing curves (ease-in, ease-out, ease-in-out)
- Spring physics (overshoot, settle)
- Bounce
- Custom bezier curves
- Procedural / GPU-computed curves

This extensibility is built into the architecture from the start, even though the first implementation only uses lerp.

### More Complex Effects (Future)

Down the road, transitions can go beyond geometric interpolation:
- Particle dissolution (a component "dissolves" into particles that reform as the target)
- Fluid deformation (geometry flows like a physical material)
- GPU compute-driven morphing along arbitrary paths

These are future directions. The interpolation model described above is designed to accommodate them without structural changes.

---

## Geometry Model

### V1: Textured Rectangles

Every component in the first version of Proteus is a **rectangle composed of two triangles**, with a texture mapped onto it. This is the simplest possible GPU primitive and is intentional.

A button is a textured quad. A list item is a textured quad. A video frame is a textured quad. The visual content — text, imagery, UI chrome — is rendered into the texture. The geometry itself is always the same underlying shape: two triangles forming a rectangle, defined by four vertices with position, UV coordinates, and color.

This constraint has real advantages:

- **Transition simplicity.** Morphing between any two components is always a matter of interpolating between two sets of quad vertices, a UV mapping, and a color. No special cases for different geometry types.
- **GPU efficiency.** Textured quads are the most heavily optimized primitive in any GPU pipeline. Every hardware and driver combination handles them well.
- **Predictable performance.** Because all components share the same geometry type, rendering cost is uniform and easy to reason about. The first version will run smoothly.

More complex geometry (arbitrary meshes, SDF shapes, curves) is a future extension. The textured quad model is the foundation everything else builds on.

---

## Why GPU

Smooth, visually impressive transitions at 60fps (or higher) require work happening in parallel. The CPU is the wrong place for this — it serializes layout, logic, and rendering.

Every modern device capable of running a browser has a GPU. Proteus uses it:
- Transition state is computed and animated on the GPU
- Rendering is GPU-native (not DOM/CSS compositing)
- The interpolation parameter `t` is updated per-frame and fed to GPU pipelines that handle the visual output

This is what allows transitions to be genuinely smooth — not because of clever CSS tricks, but because the work is happening where it belongs.

---

## What This Enables

The metamorphic component model opens UX patterns that don't exist today:

- A search bar expands into a full results list, then a selected result expands into a detail view — all as one continuous visual thread
- A dashboard widget collapses into an icon in a toolbar, then re-expands somewhere else
- A media thumbnail in a list becomes a full video player, with the list items scattering to make room
- A form collapses into a submission confirmation, then into a success state — the same "object" the whole way through

The user's mental model is never broken. There is no hard cut, no page load, no component swap. The interface *transforms*.

---

## Technology

**Core language: Rust**
The framework is written in Rust. It compiles to WASM for web targets (via WebGPU) and runs natively on macOS, Linux, and Windows (via Vulkan, Metal, and DX12 through `wgpu`). XR (AR/VR) is a future target.

**GPU abstraction: wgpu**
A single Rust API over WebGPU, Vulkan, Metal, and DX12. Handles device initialization, swap chains, command encoding, and pipeline management.

**Shader language: WGSL**
WebGPU Shading Language, native to WebGPU and supported by wgpu on all backends.

**Web bindings: wasm-bindgen**
The web shell exposes a TypeScript-callable API. Developers targeting the web write TypeScript; the framework core is Rust compiled to WASM.

---

## Crate Structure

```
proteus-gpu          Layer 0 — wgpu device abstraction (no UI opinion)
proteus-render       Layer 1 — scene graph, mesh, materials, transition pipeline
proteus-ui           Layer 2 — metamorphic component model, transition topologies
proteus-shell-web    Layer 3 — WebGPU/WASM shell, TypeScript bridge
proteus-shell-native Layer 3 — winit native shell (macOS, Linux, Windows)
```

---

## Roadmap

### Phase 0 — Foundation *(current)*
- [x] Repository structure, Cargo workspace, crate scaffolding
- [x] Vision document
- [ ] `proteus-gpu`: wgpu device init, surface setup, basic command encoder

### Phase 1 — Render Core
- [ ] Textured quad renderer (two-triangle rectangle, per-instance transform, UV, color)
- [ ] Texture upload and binding pipeline
- [ ] WebGPU/WASM browser demo: static layout of textured quads
- [ ] Native demo: windowed application with same layout

### Phase 2 — Transition System
- [ ] Geometric state capture (position, size, shape, color, opacity)
- [ ] Linear interpolation (lerp) transition driver
- [ ] 1→1 transition: one component morphs into another
- [ ] 1→N transition: one component splits into N
- [ ] N→1 transition: N components converge into one
- [ ] Pluggable interpolation function interface
- [ ] Browser demo: button → list → video player

### Phase 3 — Component Model
- [ ] Metamorphic component declaration API
- [ ] Transition graph: define valid transitions between component forms
- [ ] Content interpolation (text, images, media)
- [ ] Event handling across transition states

### Phase 4 — Developer Experience
- [ ] TypeScript bindings for web
- [ ] DevTools: visualize transition state and interpolation
- [ ] Documentation and examples

### Phase 5 — Advanced Transitions *(exploratory)*
- [ ] Non-linear easing library
- [ ] Particle dissolution effects
- [ ] Fluid deformation
- [ ] XR shell (WebXR / OpenXR)

---

## Name

Proteus was the ancient Greek sea god known for shapeshifting — he could take any form while remaining himself. The name is the concept: **identity that persists through continuous transformation**.
