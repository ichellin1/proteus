# Proteus — Planning Document

> This is a living document. It tracks the planning phases for the Proteus project, what has been decided, what is in progress, and what still needs to be worked through. Update it as decisions are made.

---

## Planning Phases

```
Phase A  Vision             ← in progress
Phase B  Architecture
Phase C  Dependencies & Tooling
Phase D  Project Plan & Roadmap
Phase E  Build
```

---

## Phase A — Vision

**Status: In Progress**

The vision document ([VISION.md](./VISION.md)) is a living artifact that will continue to be refined through this phase.

### Decided

- [x] Core paradigm: metamorphic components — UI elements that transform into other UI elements with fluid, continuous transitions
- [x] Transition topologies: 1→1, 1→N, N→1
- [x] Interpolation model: lerp as foundation, pluggable interpolation functions for future variation
- [x] V1 geometry: textured rectangles (two-triangle quads) — same primitive for all components
- [x] Technology: Rust core, wgpu for GPU abstraction, WASM for web, winit for native
- [x] Target platforms: Web (WebGPU), desktop native (macOS/Linux/Windows), XR future
- [x] Concept is validated — an original POC was built ~11 years ago using JavaScript and WebGL

### In Progress

- [ ] **Component definition** — what exactly is a component in Proteus?
  - What does a developer provide to define one?
  - What does the framework provide vs. what does the consumer define?
  - What are the atomic properties of a component (geometry, texture, state, transitions)?
  - How are components composed — can components contain other components?

- [ ] **Developer experience** — what does it feel like to use Proteus?
  - What does a developer write to declare a component and its possible forms?
  - What does a developer write to declare a transition between two forms?
  - What triggers a transition — user input, application state, both?
  - What is the API surface for a web consumer (TypeScript)? For a native consumer (Rust)?
  - Sketch the simplest possible real example end to end

### To Do

- [ ] Reference the original POC — document what it demonstrated, what it proved, and what it did not address. Use it as a concrete reference point for the target experience.
- [ ] Define what a "prototype" milestone looks like for V1 — the specific interaction that demonstrates the paradigm

---

## Phase B — Architecture

**Status: Not Started**

*Prereqs: Phase A complete*

### To Do

- [ ] Component model — internal representation of a metamorphic component
- [ ] Transition state machine — what states does a component pass through (idle, transitioning, arrived)? What happens to application logic and input during a transition?
- [ ] Input handling during transitions — clicks, taps, and gestures while a component is mid-morph
- [ ] Scene graph — how components are organized and rendered relative to each other
- [ ] Layout model — how component positions and sizes are determined before transitions begin
- [ ] Render pipeline architecture — how the GPU pipeline is structured to support per-frame lerp updates
- [ ] WGSL shader design — vertex and fragment shaders for the textured quad renderer
- [ ] Web ↔ Rust boundary — what crosses the WASM boundary and how
- [ ] Native shell architecture — event loop, windowing, GPU surface lifecycle

---

## Phase C — Dependencies & Tooling

**Status: Not Started**

*Prereqs: Phase B complete*

### To Do

- [ ] Audit workspace dependencies already in Cargo.toml — confirm each is still the right choice after architecture is settled
- [ ] Identify any gaps — libraries needed that aren't yet included
- [ ] Licensing audit — confirm all dependencies are compatible with the intended Proteus license
- [ ] Define the owned vs. borrowed boundary — what Proteus owns outright vs. what it delegates to dependencies
- [ ] Developer tooling — build system, test harness, hot reload, WASM bundler (trunk?), CI
- [ ] Decide on the Proteus license

---

## Phase D — Project Plan & Roadmap

**Status: Not Started**

*Prereqs: Phase C complete*

### To Do

- [ ] Break the build into milestones with clear, testable definitions of done
- [ ] Identify the critical path — what must be built before anything else can be built
- [ ] Define the V1 prototype milestone — the specific demo that proves the paradigm in the new framework
- [ ] Sequence phases with realistic scope
- [ ] Move the roadmap from VISION.md into a dedicated ROADMAP.md once it is stable enough to stand alone

---

## Phase E — Build

**Status: Not Started**

*Prereqs: Phase D complete*

The fun part.

---

## Open Questions

Questions that have surfaced but don't yet have answers. Pull them into the relevant phase above as they get resolved.

- What is the exact definition of a "component" in Proteus? Where are its boundaries?
- How does a developer declare the valid transitions a component can make?
- What triggers a transition — is it always user-initiated, or can application logic drive it?
- What is the state of a component mid-transition from the application's perspective?
- What happens if a user interacts with a component that is currently transitioning?
- Can components be nested? If so, how do parent/child transitions relate?
- What does the TypeScript API look like for a web developer consuming Proteus?

---

## Reference Material

- [VISION.md](./VISION.md) — the philosophy, paradigm, geometry model, and high-level roadmap
- Original POC — JavaScript + WebGL implementation, built ~11 years ago. Validates the core concept. *(link or path to be added)*
