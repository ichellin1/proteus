# Proteus — Planning Document

> This is a living document. It tracks the planning phases for the Proteus project, what has been decided, what is in progress, and what still needs to be worked through. Update it as decisions are made.

---

## Planning Phases

```
Phase A  Vision             ✅ complete
Phase B  Architecture       ← ready to begin
Phase C  Dependencies & Tooling
Phase D  Project Plan & Roadmap
Phase E  Build
```

---

## Phase A — Vision

**Status: Complete ✅**

The vision document ([VISION.md](./VISION.md)) is stable. Phase B can begin.

### Decided

- [x] Core paradigm: metamorphic components — UI elements that transform into other UI elements with fluid, continuous transitions
- [x] Transition topologies: 1→1, 1→N, N→1
- [x] Interpolation model: lerp as foundation, pluggable interpolation functions for future variation
- [x] V1 geometry: textured rectangles (two-triangle quads) with instanced rendering — single buffer, single draw call
- [x] Technology: Rust core, wgpu for GPU abstraction, WASM for web, winit for native
- [x] Target platforms: Web (WebGL2 primary, WebGPU secondary), desktop native (macOS/Linux/Windows), XR future
- [x] Concept is validated — original POC built in JavaScript and WebGL. Well understood, no formal documentation needed.
- [x] V1 prototype interaction confirmed: button → list → detail view (M5 reference demo)
- [x] Component model: identity-based, geometric state + interaction definition, three lifecycle states, composite with single-parent ownership
- [x] Interaction states: default, hover, pressed, focused, disabled — declared sparsely, all interpolatable
- [x] V1 display characteristics: x, y, z, width, height, rotation, scale, anchor, color (RGBA), opacity, texture, corner_radius
- [x] Signal model: signals carry [to, from] UIDs, transition declared at call site with duration, easing, delay
- [x] Component handle: thin identity token, behavioral methods only, data reads via proteus.get(id)
- [x] Registry API: geometry, state, visible, children, transition (base/target/current/progress)
- [x] Composite components: Option D — declarative children array + addChild/removeChild
- [x] Child transitions: childBehavior on transition call — 'bake' default, iterator for custom per-child effects
- [x] Static baking: bake: true collapses composite into single textured quad, explicit declaration only
- [x] Visibility: ECS activation flag, default true, cascades to children
- [x] Opacity: cascades and multiplies down subtree
- [x] Component lifecycle: persistent vs ephemeral, destroy()/freeResources()
- [x] Resource management: independent lifecycle, reference counting, explicit free()
- [x] Scene graph model: signals trigger, Bevy ECS runs, instanced GPU pipeline renders
- [x] Core principles: 12 principles documented in VISION.md

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

  **Current model — resolved:**

  The signal carries the full transition declaration: `[to, from]` as component IDs. Neither component references the other. The signal owns the relationship.

  When a component is declared, the framework registers it into the ECS and assigns a UID. The declaration returns a lightweight **handle** — a thin object that wraps the UID and exposes a small set of methods (`.id()`, `.onClick()`, etc.). The handle holds no component state — all state lives in the ECS. Handles are cheap to hold in scope.

  The coupling lives in the interaction declaration, not on the component itself:

  ```typescript
  const button = component({
    geometry: { width: 120, height: 40, color: '#3B82F6' },
    label: 'View Items'
  });

  const list = component({
    geometry: [
      { width: 240, height: 48, y: 0 },
      { width: 240, height: 48, y: 52 },
      { width: 240, height: 48, y: 104 },
    ]
  });

  const image = component({
    geometry: { width: 480, height: 320 }
  });

  const contentSignal = signal(button.id());

  // Coupling lives here — in the interaction, not on the component
  button.onClick(() => contentSignal.set([list.id(), button.id()]));
  listItem.onClick(() => contentSignal.set([image.id(), listItem.id()]));
  backButton.onClick(() => contentSignal.set([list.id(), image.id()]));
  backButton.onClick(() => contentSignal.set([button.id(), list.id()]));
  ```

  **Key properties of this model:**
  - Components are fully decoupled from each other — no component references another
  - The signal owns the transition relationship: `[to, from]`
  - The `from` ID tells the framework the geometric origin of the transition
  - The same component can receive transitions from any number of origins (e.g. list receives from button going forward and from image going back)
  - Any component can trigger a transition between any two other components — cross-component triggering is a first-class feature
  - UIDs are just values — easy for AI agents to generate, store, and reference
  - Handles are lightweight and serializable — opens the door to persistent UI declarations in future

  **Transition declaration — resolved:**

  The transition is declared at the call site — always explicit, always intentional, no signal-level defaults. Every transition in the codebase is self-contained and readable in isolation.

  ```typescript
  button.onClick(() => contentSignal.set([list.id(), button.id()], {
    duration: 300,   // ms
    easing: 'linear', // pluggable — 'linear' is the V1 default
    delay: 0          // ms before transition begins
  }));
  ```

  **V1 transition options:**
  - `duration` — how long the morph takes in milliseconds
  - `easing` — interpolation function. `'linear'` in V1, pluggable for future easing curves
  - `delay` — pause before the transition begins

  **Post-V1 transition options (deferred — needs more thought):**
  - `direction` — for 1→N transitions, how the split fans out (center, left-to-right, etc.)
  - `stagger` — for 1→N transitions, whether child components animate simultaneously or with offset delays

  **Component states — resolved:**

  A component has two axes of state:

  **Lifecycle states** — how a component moves through existence:
  - `entering` — first appearance, animating in from an initial geometric state
  - `idle` — fully resolved, fully interactive (previously called "complete")
  - `transitioning` — mid-morph to another form, limited interaction
  - `exiting` — animating out before removal from the scene

  **Interaction states** — visual and behavioral states within `idle`:
  - `default` — base appearance
  - `hover` — pointer over the component
  - `pressed` — actively being interacted with
  - `focused` — keyboard focus
  - `disabled` — present but not interactive

  Interaction states are declared sparsely — only the properties that change for a given state need to be declared. Undeclared properties inherit from `default`. Every state change is a potential mini-transition, not just a CSS swap — geometry, color, opacity, size can all change per state.

  ```typescript
  const button = component({
    geometry: { width: 120, height: 40, color: '#3B82F6' }, // default state
    states: {
      hover:    { color: '#2563EB' },                        // only color changes
      pressed:  { width: 118, height: 38, color: '#1D4ED8' },// size and color change
      disabled: { color: '#93C5FD' }                         // only color changes
    }
  });
  ```

  **V1 display characteristics — resolved:**

  Every property below is declarable per interaction state (sparse — only declare what changes) and is fully interpolatable by the transition system.

  ```typescript
  {
    // Geometry
    x: number,                        // world space position
    y: number,                        // world space position
    z: number,                        // layering order (higher = on top)
    width: number,                    // dimensions
    height: number,
    rotation: number,                 // 2D angle in degrees
    scale: number,                    // uniform scale multiplier
    anchor: { x: number, y: number }, // transform origin, default { 0.5, 0.5 }

    // Visual
    color: [r, g, b, a],             // RGBA, 0.0–1.0. alpha affects color tint independently
    opacity: number,                  // 0.0–1.0, whole-component multiplier applied separately
    texture: TextureId,               // reference to a registered texture
    corner_radius: number             // rounded corners, shader-based, in pixels
  }
  ```

  **Color and opacity model:**
  `color` is RGBA — the alpha channel within color affects the color tint itself. `opacity` is a separate whole-component multiplier applied to everything (texture and color combined). In the shader: `final_alpha = color.a * opacity`. Standard compositing model, maximum flexibility.

  **corner_radius** is handled in the fragment shader via SDF calculation — not a geometry change. Rounded corners come at no vertex cost.

  **All properties are interpolatable** — during a state change or a full transition, the framework lerps between any two values cleanly. State changes (hover, pressed, etc.) are mini-transitions driven by the same interpolation system as full morphs.

  **Visibility model — resolved:**

  `visible` is a declared property on the component, defaulting to `true` if not specified. It is not a rendering flag — it is an **ECS activation flag**. When `visible: false`, the entity exists in the ECS registry but no system acts on it — not the render system, not the transition system, not the input system. It is completely inert. When `visible: true`, all systems can act on it normally.

  ```typescript
  const button = component({ visible: true, geometry: { ... } });   // active on load
  const detail = component({ visible: false, geometry: { ... } });  // inert until signal activates it
  ```

  This means transitioning a component out sets `visible: false` on completion — the entity remains in the ECS, ready to reactivate without re-creation cost. Always-visible components (headers, nav bars) simply declare `visible: true` and are never touched by a signal.

  **Component lifecycle and destruction — resolved:**

  Two categories of component lifetime:

  - **Persistent** — declared once, lives for the app lifetime. Activated/deactivated via `visible`. Default behavior.
  - **Ephemeral** — created dynamically, destroyed when no longer needed (list items, tooltips, notifications).

  Components persist unless explicitly destroyed. Automatic destruction is supported as an option (e.g. after an exit transition completes) but is opt-in, not the default.

  Handle lifecycle methods:
  ```typescript
  component.destroy()         // removes entity from ECS entirely, handle becomes invalid
  component.freeResources()   // releases all GPU resources the component references, entity remains in ECS
  ```

  **Resource management — resolved:**

  Resources (textures, video streams) have lifecycles independent of the components that reference them. They are declared and registered separately, referenced by ID. The framework tracks reference counts — GPU memory is only released when the last referencing component frees it.

  ```typescript
  // Declare resources independently
  const heroImage = proteus.texture({ src: '/images/hero.png' });
  const bgImage = proteus.texture({ src: '/images/background.png' });

  // Components reference by ID
  const hero = component({
    visible: true,
    geometry: { width: 480, height: 320 },
    texture: heroImage.id()
  });

  // Independent lifecycle
  heroImage.free();         // releases GPU memory when reference count reaches 0
  hero.freeResources();     // releases all resources this component references
  hero.destroy();           // removes from ECS entirely
  ```

  Resource concerns deferred to later milestones:
  - Loading state — textures take time to upload to GPU, components need to handle this gracefully
  - Lazy loading — don't load until component becomes visible
  - Video resource management — continuous frame streaming, separate handling from static textures

  **Interaction definition — resolved:**

  All interaction handlers use the `on` prefix consistently. Framework-managed state reactions and explicit transition triggers use the same mechanism — convention distinguishes them, not enforcement. DevTools can surface rule violations based on team-configured preferences.

  ```typescript
  button.onClick(() => contentSignal.set([list.id(), button.id()], { duration: 300 }));
  button.onHoverEnter(() => { ... });
  button.onHoverExit(() => { ... });
  button.onPress(() => { ... });
  button.onRelease(() => { ... });
  button.onFocus(() => { ... });
  button.onBlur(() => { ... });
  button.onDrag((delta) => { ... });
  ```

  **Composite components — resolved:**

  **Declaration — Option D (hybrid declarative + imperative):**
  Children are declared separately (so they have handles for signal and transition use), then passed to the parent in the declaration. Dynamic child management is available via handle methods for runtime use cases like API-loaded lists.

  ```typescript
  // Static declaration
  const item1 = component({ geometry: { width: 240, height: 48, y: 0 } });
  const item2 = component({ geometry: { width: 240, height: 48, y: 52 } });

  const list = component({
    geometry: { width: 240, height: 200 },
    children: [item1, item2]
  });

  // Dynamic management
  list.addChild(item3);
  list.removeChild(item1);                    // detaches, item1 persists in ECS
  list.removeChild(item2, { destroy: true }); // detaches and destroys
  ```

  **Coordinate space:**
  Children declare `x`, `y` relative to the parent's origin. The ECS transform system computes world space positions automatically. When the parent moves, children move with it. Standard scene graph behavior.

  **Visibility and opacity cascading:**
  - Parent `visible: false` makes the entire subtree inert — no system touches any child
  - Parent `opacity` multiplies down through children — parent `0.5` × child `0.8` = effective `0.4`

  **Transition cascading — `childBehavior`:**
  When a parent transitions, the default behavior is `'bake'` — the framework renders the parent and all children into an offscreen texture before the transition begins. That texture maps onto the parent quad for the duration of the morph. The parent transitions as a single unit, children appear embedded in it. On completion, children are restored to individual rendering. This means parent transitions cost the same regardless of child count.

  `childBehavior` is declared on the transition call, not the component, so different transitions of the same parent can have different child behaviors:

  ```typescript
  // Default — children baked into parent texture
  contentSignal.set([button.id(), list.id()], {
    duration: 300,
    childBehavior: 'bake'
  });

  // Iterator — called per child, returns a per-child transition config
  contentSignal.set([button.id(), list.id()], {
    duration: 300,
    childBehavior: (child, index, total) => ({
      duration: 300,
      easing: 'ease-out',
      delay: index * 40,           // stagger — 40ms offset per child
    })
  });

  // Scatter with custom targets
  contentSignal.set([button.id(), list.id()], {
    duration: 300,
    childBehavior: (child, index, total) => ({
      duration: 300,
      delay: index * 30,
      target: { x: Math.random() * 800, opacity: 0 }
    })
  });
  ```

  The iterator pattern makes framework-level `stagger` and `direction` primitives unnecessary — they are iterator implementations, not framework features. Both have been removed from the post-V1 scope.

  **Static baking — resolved:**

  A composite component declared with `bake: true` is a performance optimization. When the ECS encounters a baked component it:
  1. Renders the parent and all children into an offscreen texture once
  2. Maps that texture onto the parent quad
  3. Destroys the child entities from the ECS entirely
  4. The composite becomes a single leaf quad — indistinguishable from any other component

  The ECS shrinks, the render buffer shrinks, and transitions work identically — a baked component is just a textured quad like everything else. The optimization is transparent to the rest of the system.

  ```typescript
  const badge = component({
    bake: true,
    geometry: { width: 80, height: 24 },
    children: [icon, label]
  });
  ```

  `bake: true` is always explicit — the framework never infers developer intention or automatically bakes components. The DevTools can analyse the component tree and surface candidates where `bake: true` could be applied (no child interaction handlers, no signal bindings on children), but acts only as a suggestion. The developer reviews and opts in.

  The name `bake` is intentionally consistent with `childBehavior: 'bake'` on transitions — same concept, same word, shared vocabulary throughout the framework.

  **Registry API and data reads — resolved:**

  The handle is intentionally thin — an identity token with a small set of methods for attaching behavior. It holds no state. All component data lives in the ECS registry and is read via `proteus.get(id)`.

  ```typescript
  const data = proteus.get(button.id());

  // Always available — current resolved state
  // When idle: the component's declared geometry
  // When transitioning: same as data.transition.current
  data.geometry;

  // State
  data.state;       // current interaction state: 'default' | 'hover' | 'pressed' | 'focused' | 'disabled'
  data.visible;     // current visibility
  data.children;    // array of child IDs

  // Transition data — null when idle, populated during a transition
  data.transition;
  data.transition.base;     // geometric state where the transition started
  data.transition.target;   // geometric state where the transition is going
  data.transition.current;  // current interpolated state — what is being rendered right now
  data.transition.progress; // 0.0–1.0 — how far along the transition is
  ```

  `data.geometry` is always the answer to "what is this component right now" — consistent whether idle or transitioning. `data.transition` is `null` when idle, making it easy to check:

  ```typescript
  if (data.transition) {
    const pct = data.transition.progress; // 0.0–1.0
  }
  ```

  `progress` is the raw `t` value from the interpolation function — useful for dependent animations, progress indicators, or cancellation logic.

  **Full handle method set — resolved:**

  ```typescript
  // Identity
  handle.id()                          // returns the component's UID

  // Interactions
  handle.onClick(fn)
  handle.onHoverEnter(fn)
  handle.onHoverExit(fn)
  handle.onPress(fn)
  handle.onRelease(fn)
  handle.onFocus(fn)
  handle.onBlur(fn)
  handle.onDrag(fn)                    // fn receives delta: { x, y }

  // Child management
  handle.addChild(childHandle)
  handle.removeChild(childHandle, { destroy?: boolean })

  // Lifecycle
  handle.destroy()                     // removes from ECS, handle becomes invalid
  handle.freeResources()               // releases GPU resources, entity remains in ECS
  ```

  Data reads are not on the handle — they go through `proteus.get(id)`. The handle is behavioral only.

  **Still to resolve:**
  - Relative positioning and coordinate spaces — specifically how the parent's anchor point defines the child origin, and whether any layout helpers exist for common patterns (vertical stack, grid). Deferred to Phase B (Architecture).

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

**Status: In Progress**

*Phase A complete. Architecture design in progress.*

### Decided

- [x] **Transition topology normalization** — 1→N and N→1 transitions are normalized to primitives the transition system already handles. Two strategies, both available via `childBehavior`:

  **Strategy 1 — Bake (via `childBehavior: 'bake'`):**
  Normalize to **1→1**. Before the transition begins, bake the N side into a single composite texture. The transition runs as a standard 1→1 morph between two quads. At `t = 1.0`, discard the composite and restore live entities at their final positions. Simpler visually — clean morph between two forms.

  **Strategy 2 — Slice (via `childBehavior` iterator):**
  Normalize to **N→N**. The 1 side is baked and split into N virtual slice entities, each carrying a UV sub-region of the baked texture, positioned to tile and reconstruct the original. Paired 1:1 with the N entities on the other side. Each pair runs an independent 1→1 transition. At `t = 1.0`, virtual slices are discarded and live entities are revealed. More visually rich — shattering/assembling effect.

  For `1→N`: virtual slices start at the single entity's geometry and fan out to the N targets.
  For `N→1`: the N source entities animate toward their paired virtual slice of the target. Visually the N pieces converge and assemble the target shape.

- [x] **Slicing strategy (V1):** Equal strips along the dominant axis — horizontal if target entities are arranged vertically, vertical if arranged horizontally. Framework infers dominant axis from target positions. Pairing by index. Exposed as a `slicing` param in the transition config:

  ```typescript
  contentSignal.set([button.id(), list.id()], {
    duration: 300,
    slicing: 'horizontal',  // 'horizontal' | 'vertical' | 'proportional' | 'uniform'
    childBehavior: (child, index, total) => ({ duration: 300, delay: index * 40 })
  });
  ```

  `'uniform'` is a special case — all N virtual entities receive the full source texture mapped to the full source geometry. Visually: N identical copies of the source that peel apart and each morph to a target. Reads as "cloning then diverging" rather than shattering.

  **Post-V1:** custom slicing function — `slicing: (n, sourceGeom, targetGeoms) => SliceConfig[]`.

### To Do

- [ ] Relative positioning and coordinate spaces — how child components position relative to parents, how parent anchor point defines child origin, whether any layout helpers exist for common patterns (vertical stack, grid)
- [ ] Component model — internal ECS representation: entities, component arrays, system definitions
- [ ] Transition state machine — lifecycle states (entering, idle, transitioning, exiting) and system behavior in each
- [ ] Input handling during transitions — hit testing, event routing, mid-transition limited interaction enforcement
- [ ] Bevy ECS integration — confirm bevy_ecs as dependency, define entity/component/system structure
- [ ] Signal system design — how signals are implemented, how they interface with the ECS transition system
- [ ] Render pipeline architecture — single instanced draw call, homogeneous quad buffer, per-frame lerp updates. **3D note:** the pipeline must not foreclose future 3D support (Principle 9). The V1 quad model is intentionally 2D-first, but the coordinate system (x, y, z), the shader design, and the camera/projection model should be designed so that 3D components and spatial transitions can be introduced in a later milestone without requiring a pipeline rewrite.
- [ ] Offscreen texture pipeline — bake: true static composites, childBehavior: 'bake' transition composites
- [ ] WGSL shader design — vertex and fragment shaders for instanced textured quad renderer, corner radius SDF
- [ ] Resource registry — texture lifecycle, reference counting, GPU memory management
- [ ] Web ↔ Rust boundary — what crosses the WASM boundary and how, TypeScript handle/signal API shape
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
- [x] Vision complete (Phase A)
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
- Transition `direction` and `stagger` — superseded by the `childBehavior` iterator pattern. Developers implement these as iterator functions rather than framework primitives. No separate post-V1 work needed.
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
