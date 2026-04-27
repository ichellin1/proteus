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

## Phase F — Patent (Parallel Track)

**Status: Not Started**

*Can begin after Phase B (Architecture) is complete. Does not block the build.*

Proteus represents a novel UX paradigm and technical system that may be patentable. This is a parallel track — it runs alongside the build rather than before it.

### Context

- An original POC was built ~11 years ago in JavaScript and WebGL at a prior employer. The concept was never patented and was not developed further.
- Proteus as designed goes significantly beyond that original work: the formal transition topology model (1→1, 1→N, N→1), composable transitions as first-class objects, the agentic API design, and the portable GPU-native architecture are all novel over the POC.
- The strategy is to establish novelty *over* the original POC rather than derive from it.
- **Legal consultation required before filing.** Key questions for an attorney: employment agreement IP clauses from the time of the POC, prior art implications of the original work, and provisional vs. full application strategy.

### To Do

- [ ] Consult a patent attorney — establish IP ownership position relative to the original POC, confirm patentability, and decide on provisional vs. full application
- [ ] Document what the original POC demonstrated and what Proteus adds that is novel over it
- [ ] Draft patent claims once architecture is settled — utility patent covering the metamorphic component system, transition topology model, and composable transition primitives
- [ ] Draft design patent claims for the visual/ornamental aspects of the transition paradigm
- [ ] Identify figures needed — the patent will require precise diagrams of the transition topologies and system architecture
- [ ] Attorney review and filing

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

## Known Risks & Challenges

Grounded concerns that need to be resolved through design decisions, prototyping, or measurement. Not reasons not to build — but things that should be answered with data rather than assumptions.

### 1. WASM Performance Ceiling
WebGPU from WASM carries overhead that native WebGPU (from JavaScript) does not. Every call across the WASM boundary has a cost, and for a per-frame transition pipeline that cost compounds. This is a known, documented issue with wgpu's web target today. It may improve as the ecosystem matures, but it needs to be prototyped and measured early — before the architecture is locked in — rather than assumed to be fast enough.

*Resolution path: build a minimal benchmark during Phase 1 that measures WASM↔GPU round-trip cost at 60fps. Use the result to inform architecture decisions.*

### 2. WebGPU Browser Coverage
WebGPU is not universally available. Firefox support is behind a flag, and Safari's implementation has historically lagged. The web is Proteus's primary target, but a meaningful portion of users cannot run WebGPU today. A fallback strategy needs a decision: WebGL2 degraded mode, minimum browser requirements, or something else. This is not a blocking issue but it is one that gets harder to address the later it is left.

*Resolution path: define minimum browser requirements and a fallback position during Phase C (Dependencies & Tooling).*

### 3. 1→N and N→1 Are Harder Than They Look
The 1→1 transition is clean. The split and converge topologies have real design problems inside them: the N target components must exist in some form before the transition begins (they need a "to" state to lerp toward), but they should not be visible. Layout positions for N components may not be known until runtime. N may vary dynamically. Describing these as "N simultaneous 1→1 transitions" is a useful simplification but glosses over the hard parts.

*Resolution path: work through the 1→N and N→1 cases explicitly during Phase B (Architecture) — don't defer to implementation.*

### 4. Input During Transitions Is an Unsolved Design Problem
The Responsive principle says transitions never block input, which is correct. But the specific behavior when a user interacts mid-transition is not yet defined: does the transition reverse? Snap to completion? Spawn a new transition from the current interpolated state? Each answer has different implications for the state model and the developer API. This is an interaction design problem as much as an engineering one.

*Resolution path: define the interaction model explicitly during Phase A (Vision) before architecture begins.*

### 5. Declarative + Agentic vs. Expressive
Declarative APIs that are easy for AI agents to generate tend to be constrained. Expressive APIs that give developers full control tend to be imperative. Satisfying both requires very deliberate API design — it doesn't happen accidentally. The risk is producing something neither declarative enough for agents nor expressive enough for advanced developers.

*Resolution path: sketch the developer experience (both human and agent) during Phase A, and use those sketches as a test against API proposals in Phase B.*

### 6. Minimal vs. Developer Friendly
A minimal framework puts composition burden on the developer. A developer-friendly framework provides enough scaffolding to be productive quickly. For a novel paradigm with no existing developer mental model, too minimal may mean too confusing. More opinionated defaults may be needed early — at least until the paradigm is well understood in the community.

*Resolution path: revisit the tension between principles 5 (Developer Friendly) and 11 (Minimal by Default) when sketching the developer experience in Phase A.*

---

## Reference Material

- [VISION.md](./VISION.md) — the philosophy, paradigm, geometry model, and high-level roadmap
- Original POC — JavaScript + WebGL implementation, built ~11 years ago. Validates the core concept. *(link or path to be added)*
