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
- [x] Target platforms: Web (WebGL2 primary, WebGPU secondary), desktop native (macOS/Linux/Windows), XR future
- [x] Concept is validated — an original POC was built ~11 years ago using JavaScript and WebGL

### Decided

- [x] A component is an **identity** — a stable reference that exists independently of its current visual form. The framework does not maintain a closed enum of known forms per component. Any component can take any form; transitions are declarations of a new target geometric state, not selections from a predefined list. This scales without limit.
- [x] A component has two fundamental, inseparable halves:
  - **Geometric state** — the set of quads that make up the component, their positions, sizes, colors, textures, and arrangement relative to each other. This is what the GPU renders and what transitions operate on.
  - **Interaction definition** — how the component receives and responds to user input, what events it emits, and what it does when tapped, clicked, hovered, or dragged.
- [x] A component has three interaction modes:
  - **Complete** — fully resolved into a form, full interaction available
  - **Transitioning** — mid-morph, limited interaction enforced by the framework, customizable by the application designer
  - **Transitioning default** — if no custom transitioning behavior is declared, the framework safe default applies (no interaction)
- [x] Components are **composite** — a component can contain child components, each of which is a full component in its own right with its own geometric state, interaction definition, and transition capability. A list is a component whose children are list item components.
- [x] A child component has exactly one parent. Component ownership is a strict tree at the data level.
- [x] A child's geometric state is expressed relative to its parent. When the parent moves, children move with it.
- [x] Transitions cascade in both directions — a child can initiate a transition independently of its parent (list item → detail view), and a parent can drive a transition that absorbs its children (list → button is an N→1 where N includes children).

### In Progress

- [ ] **Scene graph / internal architecture model** — how components and their relationships are represented and updated internally. This is the most important architectural decision in the project. Three models under investigation (see Research Questions below).

- [ ] **Developer experience** — what does it feel like to use Proteus?
  - What does a developer write to declare a component and its possible forms?
  - What does a developer write to declare a transition between two forms?
  - What triggers a transition — user input, application state, both?
  - What is the API surface for a web consumer (TypeScript)? For a native consumer (Rust)?
  - Sketch the simplest possible real example end to end

### To Do

- [ ] Reference the original POC — document what it demonstrated, what it proved, and what it did not address. Use it as a concrete reference point for the target experience.
- [ ] Define what a "prototype" milestone looks like for V1 — the specific interaction that demonstrates the paradigm
- [ ] Resolve scene graph model (see Research Questions) before Phase B begins

---

### Research Questions — Scene Graph Model

The choice of internal scene graph model is the most consequential architectural decision in Proteus. It affects performance, developer experience, transition behavior, layout, and the agentic API. The following models are under investigation. Research these before Phase B begins.

**Option A — DOM-style tree**
A hierarchical tree of component nodes with event bubbling and cascading updates, similar to the browser DOM. Well understood by front-end developers. Significant drawbacks: layout changes cascade through the tree, and since geometry is changing every frame during transitions, cascading recalculation becomes the default operating mode rather than an edge case. Likely too costly for a transition-heavy framework.

**Option B — ECS (Entity Component System)** ✅ Selected for internals
Entities are stable IDs (component identities). Data is stored in flat, cache-efficient arrays grouped by type (geometric state, interaction definition, transition state). Systems process data in bulk linear loops — the render system, transition system, input system each operate on their relevant data independently. No tree traversal, no cascades. Maximum CPU cache efficiency. The performance model Proteus's transition system needs.

*Research outcome: Bevy ECS (`bevy_ecs`) is a viable candidate and should be adopted rather than writing a custom ECS. To be confirmed during Phase C (Dependencies & Tooling).*

**Option C — Reactive signals** ✅ Selected for transition triggering layer only
Signals are not a good fit for driving per-frame animation — they are event-driven, not continuous. However they are a strong fit for *triggering* transitions in response to application state changes and user events. The two layers coexist sequentially: a signal fires and hands off to the ECS transition system, which drives the animation frame by frame. When the transition completes, a signal fires on completion. Clean handoff at each boundary.

**Resolution — Hybrid: signals trigger, ECS runs**
- **Public API layer**: reactive signals — declarative, event-driven, developer-friendly, AI-agent friendly. This is what developers write.
- **Transition execution layer**: Bevy ECS — entities, components, systems. Signals hand off to the ECS when a transition is triggered. ECS drives the lerp frame by frame.
- **Render layer**: instanced draw call fed by the ECS render system each frame.

The complexity of ECS is never exposed to the developer. The signal API is what they see.

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
- [ ] Render pipeline architecture — single instanced draw call with homogeneous quad buffer; per-frame lerp updates written to instance buffer
- [ ] WGSL shader design — vertex and fragment shaders for instanced textured quad renderer
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

- [ ] Finalize milestones based on architecture and dependency decisions
- [ ] Identify the critical path — what must be built before anything else can be built
- [ ] Add definition of done to each milestone
- [ ] Sequence milestones with realistic scope
- [ ] Move the roadmap from VISION.md into a dedicated ROADMAP.md once stable

---

## Milestones

A working draft of project milestones. To be finalized during Phase D once architecture and dependencies are settled. M0 is not complete until Phases A–D are done.

### M0 — Foundation *(in progress)*
The project foundation. Nothing in M1 or beyond starts until this is complete.

- [x] Repository, Cargo workspace, crate scaffolding
- [x] Vision document
- [x] Planning document
- [ ] Architecture design (Phase B)
- [ ] Dependencies & tooling decisions (Phase C)
- [ ] Project plan and milestones finalized (Phase D)

### M1 — First Pixel
A static textured quad renders in the browser (WebGL2) and natively. The instanced draw call is proven end to end. Unit and integration tests are introduced here and maintained through every subsequent milestone. Includes a benchmark comparing WASM+instanced rendering against an equivalent pure TypeScript/WebGL2 implementation to validate that the O(1) boundary crossing mitigation is sufficient in practice.

### M2 — First Transition
A single 1→1 lerp transition. One quad morphs into another — position, size, color all interpolating smoothly. The transition model is proven.

### M3 — All Three Topologies
All three transition topologies working: 1→1, 1→N, and N→1. Reference interaction: a button splits into a list, and the list collapses back into a button.

### M4 — Text Phase 1
Single line SDF text rendering. Uniform style, left-to-right. Components can carry readable labels. Required before the reference demo.

### M5 — Reference Demo
The full paradigm demo: button → list → detail view. Labeled components, scripted (not yet interactive), runs in the browser. The thing you show someone to explain what Proteus is.

### M6 — Visual Regression Testing
Headless render target, reference image capture, per-frame diffing, CI integration. Locks in rendering correctness before the more complex work of interactivity, video, and native begins.

### M7 — Interactivity
User input drives transitions. Hit testing on quads, input events triggering transitions, mid-transition input behavior defined and implemented. The reference demo becomes interactive rather than scripted. Resolves Risk #4.

### M8 — Shader Effects Library
A built-in library of WGSL shader effects — blur, glow, color grading, distortion, and similar — applicable to component textures. Designed to serve as a reference implementation for developers who want to write their own effects post-V1.

### M9 — Video
Per-frame video texture streaming to the GPU. A list item can morph into a playing video. The reference demo is extended to demonstrate this.

### M10 — TypeScript SDK
A developer can build the full interactive reference demo in TypeScript without touching Rust. The TypeScript API is idiomatic and the WASM boundary is clean and well-documented.

### M11 — Native Parity
The full reference demo runs identically on macOS, Linux, and Windows via the native shell.

### M12 — Developer Release
Documentation, examples, pluggable interpolation interface exposed, and enough polish that an outside developer can pick up Proteus and build something. The project is ready for external contributors.

---

### Post-Release
Planned future work, not part of the V1 scope:

- Text Phase 2: multi-line text and layout (line breaking, alignment, line height)
- Text Phase 3: bidirectional text (LTR/RTL, Unicode bidi algorithm)
- Text Phase 4: inline styles (mixed bold, italic, size, color within a text run)
- Custom shader authoring experience (formal support for developer-written WGSL)
- Advanced transition effects (non-linear easing library, particle dissolution, fluid deformation)
- XR shell (WebXR / OpenXR)

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

- ~~What is the exact definition of a "component" in Proteus?~~ ✅ Resolved — see Phase A decided items
- ~~Can components be nested?~~ ✅ Resolved — yes, strict single-parent tree ownership
- ~~What happens if a user interacts with a component that is currently transitioning?~~ ✅ Resolved — limited interaction enforced by framework, customizable by designer
- ~~What internal model should represent the scene graph?~~ ✅ Resolved — signals trigger transitions, Bevy ECS runs them, instanced rendering submits to GPU
- How does a developer declare a transition — what is the minimum they need to provide?
- What triggers a transition — is it always user-initiated, or can application logic drive it?
- What is the developer-facing API for defining a component's interaction definition?
- What does the TypeScript API look like end to end for a simple component with one transition?
- How do parent and child transitions coordinate — who has priority when both are triggered simultaneously?

---

## Known Risks & Challenges

Grounded concerns that need to be resolved through design decisions, prototyping, or measurement. Not reasons not to build — but things that should be answered with data rather than assumptions.

### 1. WASM↔Browser API Boundary Cost
The concern is not WASM compute performance — Rust compiled to WASM runs math and transform calculations at near-native speed, often faster than JS. The real cost is the WASM→JS boundary crossings required every time a browser GPU API (WebGL2 or WebGPU) is called from WASM. Pure JS calling those same APIs does not pay this cost.

For a naive implementation — one draw call per component, individual uniform updates — N components means roughly O(N) boundary crossings per frame. That compounds and could hurt performance vs. an equivalent JS implementation.

**Mitigation: instanced rendering.** Since all Proteus V1 components are the same primitive (textured quads), all N instance transforms, colors, and UVs can be packed into a single GPU buffer, updated with one boundary crossing per frame, and rendered with a single instanced draw call. This collapses O(N) crossings to roughly O(1) regardless of component count. WebGPU's command buffer model makes this even more efficient than WebGL2.

Instanced rendering must be a first-class architectural decision, not a later optimisation. If it is designed in from the start, the WASM boundary cost becomes largely irrelevant for Proteus's use case.

*Resolution: V1 uses a single instanced draw call with one homogeneous GPU buffer for all components. The entire scene is submitted with one buffer update and one draw call per frame — O(1) boundary crossings regardless of component count. This is the recommended rendering strategy for web targets and is a first-class architectural requirement from Phase 1. WASM remains the web target — the boundary cost concern was valid but is addressed by instanced rendering. A benchmark in M1 will validate this with real data.*

*Future geometry types (beyond quads) will break the single-buffer model. Two options are deferred to a future native-focused iteration: multiple instance buffers per type (Option 1) or geometry atlasing (Option 2). The choice between them will be informed by benchmarks on native targets.*

### 2. WebGPU Browser Coverage ✅ Resolved
WebGPU is not universally available. Firefox support is behind a flag, and Safari's implementation has historically lagged. The web is Proteus's primary target, but a meaningful portion of users cannot run WebGPU today.

*Resolution: WebGL2 is now the primary web target. WebGPU is a secondary target — a progressive enhancement used automatically by wgpu when available. No application code changes are required to benefit from WebGPU where it exists. Proteus works for essentially all web users from day one.*

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
