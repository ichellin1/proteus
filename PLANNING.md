# Proteus — Planning Document

> This is a living document. It tracks the planning phases for the Proteus project, what has been decided, what is in progress, and what still needs to be worked through. Update it as decisions are made.

---

## Planning Phases

```
Phase A  Vision                  ✅ complete
Phase B  Architecture            ✅ complete
Phase C  Dependencies & Tooling  ✅ complete
Phase D  Project Plan & Roadmap  ✅ complete
Phase E  Build                   In Progress
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

**Status: Complete ✅**

*All architecture decisions made. Phase C can begin.*

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

- [x] **ECS component types** — each Proteus component maps to one Bevy entity with the following component types attached:

  ```
  Transform        x, y, z, width, height, rotation, scale, anchor
  Visual           color (RGBA), opacity, texture_id, corner_radius
  InteractionDef   declared states map (default, hover, pressed, focused, disabled)
  InteractionState current active interaction state
  Visibility       visible: bool — ECS activation flag
  Hierarchy        parent: Option<EntityId>, children: Vec<EntityId>
  Lifecycle        entering | idle | transitioning | exiting
  ActiveTransition base geometry, target geometry, t (0.0–1.0), duration, easing, delay
                   absent when idle — inserted by transition system on morph start
  FocusMap         (optional) up/down/left/right neighbor EntityIds for directional navigation
  ```

- [x] **ECS systems:**

  ```
  transition_system    three internal phases:
                         setup    — analyze topology, create virtual slices if needed,
                                    snapshot geometries, insert ActiveTransition components
                         tick     — advance t each frame, lerp Transform+Visual on all
                                    active transitions
                         complete — detect t >= 1.0, clean up virtual entities, restore
                                    live entities, update Lifecycle, fire completion events
  render_system        reads all visible entities, builds instance buffer, submits draw call
  input_system         pointer hit testing, event dispatch, click-to-focus
  navigation_system    directional focus movement — focus map lookup with spatial algorithm
                       fallback; tab order as linear directional nav; focus trapping
  visibility_system    cascades parent visible:false down the subtree
  opacity_system       computes effective opacity (multiplied down parent chain)
  bake_system          renders bake:true composites to offscreen texture, collapses children
  ```

- [x] **ECS resources (global singletons):**

  ```
  GpuContext       wgpu device, queue, surface
  InstanceBuffer   the GPU buffer used for the instanced draw call
  TextureRegistry  registered textures with reference counts
  SignalRegistry   registered signals and their subscribers
  FocusState       currently focused entity ID (Option<EntityId>) — shared between
                   input_system (pointer focus) and navigation_system (directional focus)
  ```

- [x] **Focus and directional navigation** — `navigation_system` is a peer to `input_system`, not a subsystem of it. The input system handles pointer-based focus changes; the navigation system owns all directional focus movement (arrow keys, D-pad, remote control). Both read and write `FocusState`. Hybrid model: explicit `FocusMap` takes priority, spatial algorithm fallback when no map entry exists for a direction. `tab_index` is handled by the navigation system as linear directional nav, not a separate mechanism.

- [x] **Transition state machine** — four lifecycle states, managed by the `Lifecycle` component:

  ```
                      signal.set() fires
                      (this entity is [to])
           ┌─────────────────────────────────┐
           │                                 ▼
        [entering] ──── t = 1.0 ────► [idle] ──── signal.set() fires ────► [transitioning]
                                        │   ◄──── t = 1.0 ─────────────────────────┘
                                        │
                                   exitTo declared
                                   + destroy()/hide
                                        │
                                        ▼
                                    [exiting] ──── t = 1.0 ────► visible:false (persistent)
                                                                  or destroy()  (ephemeral)
  ```

  **System behavior per state:**

  ```
                entering       idle            transitioning   exiting
  ─────────────────────────────────────────────────────────────────────────
  render        interpolated   declared        interpolated    interpolated
                geometry       geometry        geometry        geometry

  transition    advancing t    nothing         advancing t     advancing t

  input         none           full            limited         none
                (configurable) interaction     (configurable)

  navigation    none           full focus      limited         none
                (configurable) traversal       (configurable)
  ```

  `ActiveTransition` is present in `entering`, `transitioning`, and `exiting`. Absent in `idle`.

  **`from` entity handling:** when `signal.set([to, from])` fires, the `from` entity goes `visible: false` immediately. The morph is the exit — the `to` entity carries the entire visual from the `from` entity's geometry to its own. There is no separate exit animation on the `from` entity. `exiting` is reserved for components being removed from the scene, not for morph sources.

  **Interruption — mid-transition new signal:** default behavior is the new signal is **ignored** if the target entity is already `transitioning`. Opt-in via `interruptible: true` in the transition config — the transition system snapshots the current interpolated geometry as the new `base` and starts the new transition from there. The component changes direction mid-flight.

  ```typescript
  contentSignal.set([detail.id(), list.id()], {
    duration: 300,
    interruptible: true  // new signal mid-flight snapshots current state and redirects
  });
  ```

  **`entering` — opt-in, default is snap:** when `visible` flips from `false` to `true`, the component snaps directly to `idle` with no animation. Animated entry is opt-in via `enterFrom` geometry declared on the component — triggers the `entering` lifecycle state and lerps from that geometry to the declared geometry.

  **`exiting` — opt-in, default is snap:** when `destroy()` or `visible: false` is called, the component snaps out by default. Animated exit is opt-in via `exitTo` geometry declared on the component — triggers the `exiting` lifecycle state, lerps to that geometry, then hides or destroys on completion. Mirrors the `entering` pattern.

- [x] **Input handling during transitions** — three distinct concerns:

  **Hit testing:** when an entity is `transitioning`, the input system hit tests against its interpolated geometry — what the user actually sees. The `render_system` and `input_system` both read from `ActiveTransition.current`.

  **Virtual entity filtering:** virtual slice entities (created for 1→N and N→1 transitions) carry a `Virtual` marker component. The input and navigation systems skip them entirely — they are render-only.

  **Developer-controlled input during transitions:** both interaction events and navigation events are off by default while a component is `transitioning` or `entering`. Opt-in per event class on the component declaration:

  ```typescript
  const listItem = component({
    geometry: { ... },
    transitioning: {
      allowInput: false,       // click, drag, press — default off
      allowNavigation: false,  // directional/tab events — default off
    }
  });
  ```

  Both default off. A scrolling list navigated by D-pad would set `allowNavigation: true` on its items so the user can keep moving through items mid-transition. A cinematic morph leaves both off.

  Navigation-triggered transitions remain snapshot-and-redirect (interruptible) when `allowNavigation: true`. When `allowNavigation: false`, directional events are dropped for the duration of the transition — same as interaction events.

  The `interruptible` flag on signal-driven transition configs is a separate concern from `allowNavigation` — it governs whether a signal.set() call can interrupt a running signal-driven transition.

  **Input delay after focus arrival:** a settle window that starts the moment focus lands on a component, independent of transition state. Declared on the component:

  ```typescript
  const listItem = component({
    geometry: { ... },
    focus: {
      inputDelay: 150  // ms after focus arrives before any input is accepted
    }
  });
  ```

  `FocusState` holds a `focus_accepted_at` timestamp. The input system checks elapsed time on every event routed to the focused entity — events arriving within the delay window are dropped. No new lifecycle state needed.

  A list item with both `allowNavigation: true` and `focus.inputDelay: 150` lets the user scroll through items mid-transition, but prevents accidental triggers before they've settled on a target.

- [x] **Bevy ECS integration** — `bevy_ecs` standalone crate (not full Bevy engine). Full Bevy brings its own renderer, windowing, and asset pipeline — all owned by Proteus. `bevy_ecs` is a well-supported standalone use case. Added to workspace dependencies, used by `proteus-render` and `proteus-ui`.

  **Component types (Rust):**
  ```rust
  #[derive(Component)] struct Transform { x, y, z, width, height, rotation, scale, anchor }
  #[derive(Component)] struct Visual { color: [f32; 4], opacity, texture_id, corner_radius }
  #[derive(Component)] struct InteractionDef { states: HashMap<InteractionState, StateOverride> }
  #[derive(Component)] struct InteractionState { current: StateKind }
  #[derive(Component)] struct Visibility { visible: bool }
  #[derive(Component)] struct Hierarchy { parent: Option<Entity>, children: Vec<Entity> }
  #[derive(Component)] struct Lifecycle { state: LifecycleState }
  #[derive(Component)] struct ActiveTransition { base, target, t, duration, easing, delay }
  #[derive(Component)] struct FocusMap { up, down, left, right: Option<Entity> }
  #[derive(Component)] struct TransitioningConfig { allow_input: bool, allow_navigation: bool }
  #[derive(Component)] struct FocusConfig { input_delay_ms: u32 }
  #[derive(Component)] struct Virtual;   // marker — no fields, skipped by input/nav systems
  ```

  Optional components (`ActiveTransition`, `FocusMap`, `TransitioningConfig`, `FocusConfig`, `Virtual`) are only attached when declared or needed. Bevy queries naturally exclude entities missing a queried component — no extra filtering required.

  **Resources (Rust):**
  ```rust
  #[derive(Resource)] struct GpuContext { device, queue, surface }
  #[derive(Resource)] struct InstanceBuffer { ... }
  #[derive(Resource)] struct TextureRegistry { slots: SlotMap<TextureId, TextureEntry>, ... }
  #[derive(Resource)] struct SignalRegistry { signals: SlotMap<SignalId, Signal> }
  #[derive(Resource)] struct FocusState { focused: Option<Entity>, focus_accepted_at: Instant }
  #[derive(Resource)] struct CommandQueue { pending: Vec<PendingCommand> }
  ```

  **Messages** (bevy_ecs 0.18 uses `Message`/`MessageWriter`/`MessageReader` for scheduled system-to-system communication; `Event`/`Observer` is the separate reactive/immediate trigger pattern):
  ```rust
  #[derive(Message)] struct TransitionRequest { to: Entity, from: Entity, config: TransitionConfig }
  #[derive(Message)] struct TransitionComplete { entity: Entity }
  ```
  Signals write `TransitionRequest` messages. `transition_setup_system` reads them. `TransitionComplete` is written on morph completion — used for transition chaining and restoring live entities after baked transitions.

  **System scheduling order (one frame):**
  ```
  flush_commands_system      drain CommandQueue — apply deferred mutations from last tick
                             (signal.set(), destroy(), addChild() calls from callbacks)
  input_system               process pointer/keyboard events, update InteractionState
  navigation_system          process directional/tab events, update FocusState
  transition_setup_system    read TransitionRequest messages, create virtual slices,
                             snapshot geometries, insert ActiveTransition components
  transition_tick_system     advance t, lerp Transform + Visual on active transitions
  transition_complete_system detect t >= 1.0, remove ActiveTransition, clean up virtual
                             entities, update Lifecycle, write TransitionComplete messages
  visibility_system          cascade Visibility changes down Hierarchy
  opacity_system             compute effective opacity down Hierarchy
  bake_system                handle bake:true composites, offscreen texture rendering
  render_system              read all visible non-Virtual entities, build instance
                             buffer, submit draw call
  ```

  **Frame timing:** transitions advance using Bevy's `Time` resource — delta time between frames. `t += delta_seconds / duration_seconds` each tick. Transitions specified in milliseconds run for exactly that duration regardless of frame rate. Frame-rate independent by default.

- [x] **Signal system design** — signals are thin registered objects. The work happens at `set()` time, not at creation time.

  **Internal representation:**
  ```rust
  struct Signal {
      id: SignalId,
      owner: Option<Entity>,  // entity passed to signal(id) — None for unowned signals
  }
  ```

  **Ownership:**
  - `signal()` — no owner. Lives until explicitly destroyed. Developer is responsible for cleanup.
  - `signal(button.id())` — owned by the button entity. Automatically destroyed when the owner entity is destroyed. Useful for component-scoped signals that should not outlive their component.

  **How `signal.set()` bridges to ECS:**
  ```
  signal.set([list.id(), button.id()], { duration: 300 })
      │
      ▼
  look up signal in SignalRegistry
      │
      ▼
  write TransitionRequest { to: list, from: button, config } into message queue
      │
      ▼  (next frame)
  transition_setup_system reads event, validates entities, begins transition
  ```
  `set()` returns immediately — the write is synchronous, transition begins on the next frame tick. The ECS owns the frame loop; signals are the entry point into it.

  **Multiple signals targeting the same entity in one frame:** processed in order by `transition_setup_system`. If the target is already transitioning and `interruptible: false`, subsequent requests are dropped.

  **`TransitionDropped` message — two-tier opt-in:**

  *Tier 1 — debug builds, automatic:* `#[cfg(debug_assertions)]` — all dropped requests write `TransitionDropped` messages automatically. DevTools listens to them. Zero cost in release builds.

  *Tier 2 — release builds, explicit:* developer registers a handler on the signal:
  ```typescript
  contentSignal.onDropped((request) => {
    // request.to, request.from, request.config
    // request.reason: 'already_transitioning' | 'entity_not_found' | 'entity_not_visible'
  });
  ```
  In release with no handler registered, the event is skipped entirely — no allocation, no cost. `request.reason` lets the developer distinguish between the common "already transitioning" case and error cases like missing or invisible entities.

  **Signal cleanup:**
  ```typescript
  contentSignal.destroy()  // removes from SignalRegistry — further set() calls have no effect
  ```
  Owned signals are destroyed automatically with their owner. Unowned signals must be explicitly destroyed by the developer.

- [x] **Render pipeline architecture** — single instanced draw call, one buffer upload per frame.

  **Base geometry — static, uploaded once at startup:**
  Unit quad centered at origin, four vertices, six indices (two triangles). Never changes.
  ```
  vertices: [(-0.5,-0.5), (0.5,-0.5), (0.5,0.5), (-0.5,0.5)]
  indices:  [0, 1, 2,  0, 2, 3]
  ```

  **Instance buffer — one entry per visible component:**
  ```rust
  #[repr(C)]
  #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
  struct QuadInstance {
      position:      [f32; 3],  // x, y, z — world space (radians internally)
      size:          [f32; 2],  // width, height
      rotation:      f32,       // radians (converted from degrees at WASM boundary)
      scale:         f32,       // uniform scale multiplier
      anchor:        [f32; 2],  // transform origin, 0.0–1.0
      color:         [f32; 4],  // RGBA
      opacity:       f32,       // whole-component multiplier
      corner_radius: f32,       // pixels, for SDF in fragment shader
      uv_offset:     [f32; 2],  // texture sub-region origin (in main_atlas or transition_atlas)
      uv_scale:      [f32; 2],  // texture sub-region size
      atlas_page:    u32,       // 0 = main_atlas, 1 = transition_atlas
  }
  ```
  See below for crossfade and border fields. Full struct is ~124 bytes per instance. 1000 components ≈ 124KB — well within GPU limits.

  **Camera and projection — 3D forward-compatible from day one:**
  The vertex shader computes `clip_position = projection * view * model * vertex_position`.
  View and projection are frame-level uniforms (one upload per frame, same for all instances).

  *V1:* camera fixed at world origin looking down -Z. Orthographic projection. Z on instances controls depth ordering.
  *Future 3D:* swap orthographic for perspective, move camera freely — no pipeline rewrite needed.

  **Coordinate system:**
  - Origin at viewport center, Y-up. Consistent with 3D math conventions.
  - Units: 1 unit = 1 pixel in V1. DPI scale factor applied in the projection matrix — isolated to one place.
  - TypeScript API may offer a top-left convenience mode for front-end developers. The GPU pipeline is always center-origin.

  **Rotation units:**
  - TypeScript API: degrees — human-readable, front-end convention (`rotation: 90`)
  - WASM boundary: degrees → radians via `f32.to_radians()` — conversion happens once on entry
  - ECS `Transform` component, `QuadInstance` buffer, WGSL shaders: radians throughout
  - Rule: degrees are a TypeScript-layer concern and never appear in Rust or WGSL

  **Textures — two-atlas model:**
  All component textures are packed into one of two 2D atlas textures. `uv_offset`/`uv_scale` address the sub-region within the active atlas — the same UV infrastructure used for virtual slice UV mapping. `atlas_page` on `QuadInstance` identifies which atlas: 0 = `main_atlas`, 1 = `transition_atlas`.

  **`main_atlas`** — long-lived component textures: permanent bakes (`bake: true`), loaded images, video frames (M9). Managed via LRU/eternal/reference counting. Sized to the window at init.

  **`transition_atlas`** — ephemeral bakes created at transition start and freed in `transition_complete_system`. No LRU needed — all entries have a known expiry. Sized at 2× window area to hold two concurrent full-screen bakes (the from-state and to-state) simultaneously.

  **Static bake from-state edge case:** when a `bake: true` component is the from-state of a transition, its texture already lives in `main_atlas`. At transition start it is re-baked into `transition_atlas` (a GPU render pass, sub-millisecond one-time cost). This keeps the shader uniform — base UV always addresses `transition_atlas` during a crossfade — and avoids per-frame warp divergence that would result from branching on atlas identity in the fragment shader.

  **`render_system` frame loop:**
  ```
  1. query all Visibility(true), no Virtual marker, with Transform + Visual
  2. sort by z ascending (lower z = further back)
  3. for each entity: pack Transform + Visual → QuadInstance, write to staging buffer
  4. upload staging buffer to GPU (one transfer)
  5. upload view/projection uniform (one transfer)
  6. draw_indexed_instanced(6 indices, instance_count)
  ```
  One buffer upload, one draw call per frame regardless of component count.

- [x] **Offscreen texture pipeline** — two cases sharing the same wgpu render-to-texture mechanism.

  **Underlying mechanism:**
  ```
  1. determine bounding box of composite (parent + all children)
  2. create wgpu texture sized to that bounding box
     (TextureUsages::RENDER_ATTACHMENT | TEXTURE_BINDING)
  3. run render pipeline targeting that texture — composite entities only
  4. register resulting texture in TextureRegistry
  5. update parent Visual.texture_id to reference baked texture
  6. handle children per bake type (destroy vs suppress)
  ```

  **Case 1 — Static `bake: true`:**
  Uses Bevy's `Added<Baked>` query filter — detects any entity where the `Baked` component was just attached, whether at startup or created dynamically at runtime. Runs every frame but does zero work when no new `Baked` entities have appeared. On detection: runs offscreen render, destroys child entities from ECS, parent becomes a permanent leaf quad. Handles startup and dynamic creation uniformly — no explicit flag or manual trigger needed. Baked texture lives for the component's lifetime, freed on `component.destroy()`.

  **Case 2 — Transition `childBehavior: 'bake'` (frame-loop `bake_system`):**
  Triggered in `transition_setup_system` before `ActiveTransition` is inserted.

  *1→N (e.g. button → list):*
  1. Bake `to` entity (list + children) into offscreen texture at its declared geometry
  2. Temporarily suppress children (`Visibility: false` — not destroyed)
  3. Set up `ActiveTransition` on `to` — behaves as single quad for the transition
  4. `from` goes `visible: false` as normal
  5. On `TransitionComplete`: release baked texture, restore children `Visibility: true`

  *N→1 (e.g. list → button):*
  1. Bake `from` entity (list + children) into offscreen texture *before* hiding it
  2. Transition runs: baked list geometry → button geometry, textures crossfade
  3. On `TransitionComplete`: release baked texture, `from` remains `visible: false`

  **Texture transition — crossfade (default for baked transitions):**
  Fragment shader blends base and target textures over the transition duration:
  `final_color = mix(base_texture, target_texture, t)`

  Both texture IDs are stored on `ActiveTransition`:
  ```rust
  struct ActiveTransition {
      base_geometry:  GeometrySnapshot,
      target_geometry: GeometrySnapshot,
      base_texture:   Option<TextureId>,  // texture at t=0
      target_texture: Option<TextureId>,  // texture at t=1
      t:              f32,
      duration:       f32,
      easing:         EasingFn,
      delay:          f32,
  }
  ```
  `base_texture` and `target_texture` are `None` for non-baked transitions — the fragment shader falls back to the entity's declared texture. For baked transitions, the crossfade blends the two textures as geometry morphs. Baked transition textures are freed in `transition_complete_system` once `t = 1.0`.

  **`bake_system` in the frame loop** handles suppression/restoration of children around transition bakes — distinct from the startup bake system. Same offscreen render mechanism, different lifecycle.

- [x] **WGSL shader design** — vertex and fragment shaders for the instanced quad renderer.

  **Binding layout:**
  ```
  Bind group 0:  uniform buffer — view_projection: mat4x4<f32>  (one upload per frame)
  Bind group 1:  main_atlas: texture_2d + transition_atlas: texture_2d + sampler
  Vertex buf 0:  base quad vertices — static, 4 vertices, uploaded once
  Vertex buf 1:  instance buffer — updated each frame
  ```

  **Atlas sizing:** two fixed-size 2D atlas textures are created at init. `main_atlas` defaults to window size; `transition_atlas` defaults to 2× window area to accommodate two concurrent full-screen bakes. Both are bounded by the device's `max_texture_dimension_2d` (queried via wgpu at init). Configurable via `ProteusConfig`:
  ```rust
  struct ProteusConfig {
      max_textures:          u32,                // default: 256
      main_atlas_size:       Option<(u32, u32)>, // None = window size at init
      transition_atlas_size: Option<(u32, u32)>, // None = 2× window size at init
      // ... other config
  }
  ```

  **Vertex shader — instance transform:**
  Model matrix computed in the vertex shader (not CPU-side). Steps per vertex:
  1. Scale base unit quad vertex by `(width * scale, height * scale)`
  2. Shift by anchor offset so rotation happens around anchor point
  3. Rotate by `instance.rotation` (radians)
  4. Translate to world position `instance.position.xyz`
  5. Multiply by `uniforms.view_projection` → clip space
  UV coordinates are transformed by `uv_offset` and `uv_scale` for texture sub-region support.

  **Fragment shader — texture, tint, opacity, SDF corner radius, crossfade, border:**

  *Corner radius SDF:*
  ```wgsl
  fn sdf_rounded_rect(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
      let q = abs(p) - half_size + vec2(r, r);
      return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
  }
  ```
  Negative inside, positive outside, zero at edge. `smoothstep(-1.0, 1.0, dist)` gives a 1-pixel antialiased edge — no extra geometry required. Fragments outside the rounded shape are discarded.

  *Crossfade (baked transitions):*
  When `crossfade_t > 0.0`: sample `transition_atlas` at `base_uv_offset`/`base_uv_scale` (from-state), sample the active atlas (per `atlas_page`) at `uv_offset`/`uv_scale` (to-state), blend:
  `tex_color = mix(base_color, target_color, crossfade_t)`
  When `crossfade_t == 0.0`: skip blend, sample active atlas at `uv_offset`/`uv_scale` only.

  *Color tint + opacity:*
  `tinted = tex_color * instance.color` — RGBA multiply
  `final_alpha = tinted.a * instance.opacity * edge_alpha`

  *Border (SDF-based, zero cost when unused):*
  Uses the SDF distance value already computed for corner radius. Supports inner, center, and
  outer border placement via a single `border_offset` parameter:
  ```wgsl
  // border_offset: -1.0 = inner, 0.0 = center, 1.0 = outer
  let half_w = border_width * 0.5;
  let border_center_dist = border_offset * half_w;
  let border_dist = abs(dist - border_center_dist) - half_w;
  let border_alpha = 1.0 - smoothstep(-1.0, 1.0, border_dist);
  ```
  Continuous range — values between -1.0 and 1.0 place the border anywhere relative to
  the shape edge. Same antialiasing as corner radius. Zero cost when `border_width == 0.0`.
  TypeScript API: `borderAlignment: 'inner' | 'center' | 'outer'` (maps to -1.0, 0.0, 1.0);
  raw float also accepted for fine-grained control. Three fields on `QuadInstance`:
  `border_width: f32`, `border_color: [f32; 4]`, `border_offset: f32` (default 0.0).

  *Note — future shader effects:* blur, glow, drop shadow, distortion, and other multi-pass effects
  belong in the M8 Shader Effects Library. The base fragment shader handles all single-pass
  per-fragment effects. The SDF distance value should be considered an extension point — future
  effects that build on shape-awareness (inner glow, emboss, etc.) can reuse it.

  **Updated `QuadInstance` fields for shader support:**
  ```rust
  // crossfade — base UV always addresses transition_atlas
  base_uv_offset: [f32; 2],   // from-state sub-region origin in transition_atlas
  base_uv_scale:  [f32; 2],   // from-state sub-region size in transition_atlas
  crossfade_t:    f32,        // 0.0 = no crossfade, 1.0 = fully to-state

  // border
  border_width:   f32,        // 0.0 = no border
  border_color:   [f32; 4],   // RGBA
  border_offset:  f32,        // -1.0 inner, 0.0 center, 1.0 outer — default 0.0
  ```

- [x] **Resource registry** — texture lifecycle, reference counting, GPU memory management.

  **Storage — `slotmap` crate (not HashMap):**
  TextureIds are small integers in a fixed range (0 to `max_textures`). Direct slot access beats HashMap — no hashing, contiguous memory, cache-friendly, O(1) with a much smaller constant. The `slotmap` crate adds generation counters: each slot carries a generation number that increments on free/reuse. A stale `TextureId` is detectable immediately — error in debug builds rather than silently sampling the wrong texture. `SignalRegistry` uses `slotmap` for the same reasons.

  ```rust
  struct TextureRegistry {
      slots:    SlotMap<TextureId, TextureEntry>,
      capacity: u32,  // from ProteusConfig.max_textures (default 256)
      // atlas packing state managed internally — strategy TBD at implementation
  }
  ```

  **`TextureEntry` structure:**
  ```rust
  struct AtlasRegion {
      x:      u32,
      y:      u32,
      width:  u32,
      height: u32,
      page:   u8,  // 0 = main_atlas, 1 = transition_atlas
  }

  struct TextureEntry {
      atlas_region: AtlasRegion,          // location within the atlas
      ref_count:    u32,
      size:         (u32, u32),
      format:       wgpu::TextureFormat,  // V1: RGBA8Unorm
      state:        TextureState,         // Loading | Ready | Failed
      kind:         TextureKind,          // Static (V1) | Video (M9)
  }
  ```

  `TextureEntry` no longer owns a `wgpu::Texture` — the atlas textures are owned by `GpuContext` and shared across all entries. `atlas_region` describes where the entry lives within its atlas page. Freeing an entry returns its region to the atlas packer.

  **Reference counting — two tiers:**
  Framework auto-tracks refs when components reference textures. Ref count increments when a component is created or updated with a texture reference; decrements when `free()` is called, `component.freeResources()` is called, `component.destroy()` is called, or a component's texture property changes away from this texture. When ref count reaches zero: atlas region returned to the atlas packer.

  **Loading state:**
  While `TextureState::Loading`: render system substitutes a 1×1 transparent fallback — component renders its `color` only. No crash, no stall.

  **Baked texture lifecycle:**
  - *Static bake:* owned by the baked component, no developer handle. Freed on `component.destroy()`.
  - *Transition bake:* internal to transition system. Created by `transition_setup_system`, freed by `transition_complete_system`. Never surfaced to the developer.

  **Forward compatibility — video (M9):**
  `TextureKind::Video` declared now so registry needs no structural changes at M9. No V1 implementation.

  **Developer-facing texture handle:**
  ```typescript
  const heroImage = proteus.texture({ src: '/images/hero.png' });
  heroImage.id()        // TextureId
  heroImage.free()      // explicit release — decrements ref count
  heroImage.state()     // 'loading' | 'ready' | 'evicted' | 'failed'
  heroImage.onReady(fn)    // fires when texture is uploaded and available
  heroImage.onEvicted(fn)  // fires when texture is evicted from GPU
  heroImage.onRestored(fn) // fires when texture is back on GPU after eviction
  heroImage.restore()      // explicitly push to front of restoration queue
  ```

  **Capacity strategy — fixed, no dynamic growth:**
  The texture budget is declared at init via `ProteusConfig.max_textures`. No dynamic array resizing. Application designers own their texture budget. Assuming memory is always available is not safe — fixed capacity forces explicit resource responsibility.

  **Eternal vs ephemeral:**
  ```typescript
  const navIcon   = proteus.texture({ src: '/icons/nav.png', eternal: true }); // never evicted
  const heroImage = proteus.texture({ src: '/images/hero.png' });              // ephemeral (default)
  ```
  Eternal textures occupy their slot permanently. Navigation icons, persistent UI chrome, and any texture that must always be available should be declared eternal.

  **LRU eviction — order when a slot is needed:**
  1. Unreferenced ephemeral textures (`ref_count = 0`) — evict oldest `last_used` first
  2. Referenced ephemeral textures (`ref_count > 0`) — evict oldest `last_used`
  3. Eternal textures — never candidates for eviction

  `last_used` is a timestamp updated by the render system each frame a texture is drawn. LRU candidate is always the oldest among eligible textures. Added to `TextureEntry`:
  ```rust
  eternal:   bool,     // true = never evicted — default false
  last_used: Instant,  // updated each frame the texture is rendered
  ```

  `TextureState` gains `Evicted`:
  ```rust
  enum TextureState { Loading, Ready, Evicted, Failed }
  ```

  **Fallback when texture unavailable:**
  Black geometry — component's color still renders, texture contribution replaced with solid black. Never crashes. Configurable via `ProteusConfig.missing_texture_color`, black is the default.

  **Restoration queue:**
  When an evicted texture is needed for rendering:
  1. Render black immediately (fallback)
  2. Queue a restoration request for that texture
  3. When a slot opens (texture freed), process queue — re-upload from source, resume normal rendering
  `heroImage.restore()` pushes the texture to the front of the restoration queue explicitly.

  **Backgrounding — platform GPU release:**
  When the app is backgrounded and the platform requires GPU memory to be freed:
  1. All ephemeral textures marked `Evicted`, GPU memory released
  2. Eternal textures freed if platform requires full GPU release — flagged for immediate restoration on foreground
  3. On foreground: eternal textures re-uploaded immediately, ephemeral textures restored lazily as they're needed

  ```typescript
  proteus.onBackground(() => { /* app-level cleanup */ });
  proteus.onForeground(() => { /* notified before ephemeral restoration begins */ });
  ```
  The framework owns the GPU lifecycle. Developer hooks are for app-level concerns on top.

- [x] **Web ↔ Rust boundary** — thin WASM surface, rich Rust interior.

  **Principle:** everything stays in Rust — ECS, rendering, transitions, texture management. The WASM boundary is a clean declarative surface. TypeScript tells Rust what to do; Rust does all the work.

  **Two TypeScript layers:**
  - *Raw wasm-bindgen output* — internal layer, mechanical, not consumed directly by developers
  - *TypeScript SDK* — the public API. Handles degrees→radians, hex color→RGBA, top-left convenience coordinates, idiomatic TypeScript types. Developers import from the SDK only.

  **Exposed WASM surface — three handle classes + Proteus entry point:**
  ```rust
  #[wasm_bindgen] pub struct Proteus
    async fn init(canvas_id, config: JsValue) -> Proteus
    fn component(&self, config: JsValue) -> Component
    fn signal(&self, owner_id: Option<String>) -> Signal
    fn texture(&self, config: JsValue) -> Texture
    fn get(&self, id: &str) -> JsValue          // registry read
    fn tick(&self)                               // advance one frame
    fn on_background(&self, cb: Function)
    fn on_foreground(&self, cb: Function)

  #[wasm_bindgen] pub struct Component
    fn id(&self) -> String
    fn on_click/on_hover_enter/on_hover_exit/on_press/on_release/
       on_focus/on_blur/on_drag(&self, cb: Function)
    fn add_child/remove_child(&self, child, destroy: bool)
    fn destroy/free_resources(&self)

  #[wasm_bindgen] pub struct Signal
    fn id(&self) -> String
    fn set(&self, to_id, from_id, config: JsValue)
    fn on_dropped(&self, cb: Function)
    fn destroy(&self)

  #[wasm_bindgen] pub struct Texture
    fn id(&self) -> String
    fn free/restore(&self)
    fn state(&self) -> String
    fn on_ready/on_evicted/on_restored(&self, cb: Function)
  ```

  **Config as `JsValue` + `serde_wasm_bindgen`:** component config, transition config, and other complex objects cross as plain JS objects and are deserialized in Rust. Developers write plain object literals — no complex typed boundary structs.

  **Callbacks:** JS functions stored as `js_sys::Function` in the ECS `InteractionDef`. Invoked synchronously by the input system when events fire. The only Rust→JS crossing during normal operation — all other data flows JS→Rust.

  **Re-entrancy safety — command queue:** if a callback calls back into the framework (`signal.set()`, `destroy()`, `addChild()`, etc.) while systems hold a mutable borrow on the ECS world, a direct mutation would panic. All WASM handle methods that mutate the ECS therefore defer their work: they push a `PendingCommand` onto the `CommandQueue` resource rather than touching the world directly. The queue is drained by `flush_commands_system` at the start of the next `tick()`. Mutations from callbacks in tick N take effect at the start of tick N+1. This is deterministic and panic-free regardless of call origin — inside a callback, in a `setTimeout`, or in developer code between frames.

  **Entity IDs as strings at the boundary:** internally Bevy `Entity` is a typed u64; at the boundary it becomes a string. Strings are safe, serializable, inspectable in DevTools, and easy for AI agents to reference.

  **Frame loop — `tick()` owned by the TypeScript SDK:** the SDK drives `requestAnimationFrame` and calls `proteus.tick()` each frame. Developers can integrate into an existing render loop or control timing manually. SDK default wires `tick()` to `rAF` automatically.

  **`proteus.get(id)` returns a typed object:** the SDK wraps the raw `JsValue` registry read into a properly typed `ComponentData` interface. Developers get typed access to geometry, state, visibility, children, and transition data — not a raw JS object.

- [x] **Relative positioning and coordinate spaces**

  **Two coordinate spaces:**
  ```
  World space:  center-origin, Y-up   — what the GPU sees, used for root components
  Local space:  top-left,      Y-down — what developers declare for children
  ```
  Child `(0, 0)` is the parent's top-left corner. Y increases downward. Natural for web developers and consistent with the Phase A code examples. The TypeScript SDK can expose a `coordinateMode: 'center' | 'top-left'` option for root-level component declarations as well.

  **Parent anchor does not affect child origin.** The parent's `anchor` property controls its own rotation/scale pivot only. Child `(0, 0)` is always the parent's top-left bounding box corner regardless of anchor. Separation of concerns — anchor is a transform property, not a layout property.

  **Two-component transform model:**
  ```rust
  #[derive(Component)]
  struct LocalTransform {
      x, y, z,           // parent-relative, top-left origin, Y-down
      width, height,
      rotation, scale, anchor
  }

  #[derive(Component)]
  struct WorldTransform {
      x, y, z,           // world-space, center-origin, Y-up — computed each frame
      width, height,
      rotation, scale
  }
  ```
  A `transform_system` runs each frame and computes `WorldTransform` from `LocalTransform` + parent's `WorldTransform`. The render system reads `WorldTransform` only. Bevy `Changed<LocalTransform>` change detection means only dirty entities are recomputed. Root components (no parent) map `LocalTransform` directly to `WorldTransform`.

  **Z depth — relative to parent:**
  `world_z = parent_world_z + local_z`. Children are always rendered within their parent's depth layer. Developers manage scene layering at the root level; composites manage internal ordering via child z offsets.

  **Layout — V1 default is `Free` (manual positioning):**
  Developers declare `x, y` on each child directly. No layout system in V1. SDK utilities (`VStack`, `HStack`, `Grid`) may be provided as pure TypeScript position calculators for convenience, but they are static — they compute positions once and return values.

  **Layout in ECS — planned post-V1 milestone:**
  `VStack`, `HStack`, and `Grid` as ECS layout components with a `layout_system`. When children are added/removed or sizes change, the layout system recomputes `LocalTransform` for affected children. The transform system detects those changes. The transition system automatically animates position deltas — items in a list glide to new positions when the list grows or shrinks, with no manual transition calls from the developer. This requires the core transition system to be proven first; layout is a natural next layer on top.

### To Do

- [x] Relative positioning and coordinate spaces ✅
- [x] Component model / Bevy ECS integration ✅
- [x] Transition state machine ✅
- [x] Input handling during transitions ✅
- [x] Signal system design ✅
- [x] Render pipeline architecture ✅
- [x] Offscreen texture pipeline ✅
- [x] WGSL shader design ✅
- [x] Resource registry ✅
- [x] Web ↔ Rust boundary ✅
- [x] Native shell architecture ✅

---

## Phase C — Dependencies & Tooling

**Status: Complete ✅**

*Prereqs: Phase B complete*

### Decided

- [x] **Dependency audit — existing crates confirmed:**
  `wgpu` ✅, `bytemuck` ✅, `glam` ✅, `pollster` ✅ (native-only), `serde` ✅, `serde_json` ✅,
  `log` ✅, `env_logger` ✅ (native-only), `wasm-bindgen` ✅, `web-sys` ✅, `js-sys` ✅,
  `wasm-bindgen-futures` ✅, `winit` ✅, `thiserror` ✅, `anyhow` ✅.
  Removed: `futures` (covered by `pollster` on native and `wasm-bindgen-futures` on wasm — no remaining use).

- [x] **Dependency gaps filled:**
  - `bevy_ecs = "0.18"` — standalone ECS runtime (does not pull in full Bevy engine). MIT/Apache 2.0.
    Note: bevy_ecs 0.18 uses `Message`/`MessageWriter`/`MessageReader` for scheduled system-to-system
    communication; `Event`/`Observer` is the separate reactive/immediate trigger pattern. Architecture
    docs updated accordingly (`TransitionRequest`, `TransitionComplete`, `TransitionDropped` → `Message`).
  - `slotmap = "1"` — O(1) stable-key storage with generation safety. Used by `TextureRegistry` and
    `SignalRegistry`. Already a transitive dep of bevy_ecs; declared explicitly since we use it directly.
  - `serde-wasm-bindgen = "0.6"` — `JsValue` ↔ Rust deserialization at the WASM boundary. MIT.
  - `etagere = "0.3"` — shelf-based 2D atlas packing for `TextureRegistry`. MIT/Apache 2.0. Written by
    the author of Firefox's WebRender; designed for dynamic atlas allocation/deallocation.
  - `wasm-logger = "0.2"` — routes `log::` macros to `console.log` on wasm. Pairs with `env_logger` on native.
  - `console_error_panic_hook = "0.1"` — routes Rust panics to `console.error` on wasm. Apache 2.0/MIT.

- [x] **License — MIT OR Apache-2.0.** Standard dual-license for Rust ecosystem libraries. All runtime
  dependencies are MIT and/or Apache 2.0 compatible. `LICENSE-MIT` and `LICENSE-APACHE` created.
  `Cargo.toml` and `README.md` updated. No GPL, LGPL, or other copyleft in the dependency tree.

- [x] **Owned vs. borrowed boundary:**
  - *Owned outright:* rendering pipeline (WGSL shaders, instance buffer management, atlas management),
    ECS component types and system schedule, signal model, WASM boundary surface, TypeScript SDK.
  - *Delegated to dependencies:* GPU abstraction (`wgpu`), math (`glam`), ECS runtime (`bevy_ecs`),
    GPU buffer casting (`bytemuck`), atlas packing (`etagere`), stable-key storage (`slotmap`),
    WASM bindings (`wasm-bindgen`, `web-sys`, `js-sys`), native windowing (`winit`),
    async bridging (`pollster`, `wasm-bindgen-futures`), serialization (`serde`, `serde_json`,
    `serde-wasm-bindgen`), logging (`log`, `env_logger`, `wasm-logger`, `console_error_panic_hook`),
    error handling (`thiserror`, `anyhow`).
  - The rendering model, transition system, and developer API are 100% Proteus-owned. Dependencies
    handle infrastructure. No dependency owns any part of the public-facing API surface.

- [x] **Developer tooling decisions:**
  - *Build — web:* `wasm-pack` for npm library distribution; `trunk` for the reference demo app.
  - *Build — native:* `cargo build` / `cargo run` via standard Cargo workflow.
  - *Testing:* `cargo test` for unit and integration tests; visual regression tests against a headless
    wgpu render target (introduced at M6).
  - *CI:* GitHub Actions — `cargo test`, `cargo clippy`, `wasm-pack build` on every push.
  - *Hot reload:* deferred — not in scope until M5 (reference demo) or later. Trunk provides basic
    browser live-reload for the demo during development.
  - *Formatting:* `rustfmt` with default settings. `cargo fmt --check` in CI.

---

## Phase D — Project Plan & Roadmap

**Status: Complete ✅**

*Prereqs: Phase C complete*

### Decided

- [x] **Critical path:** M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7 → M10 → M12.
  Each step is a hard prerequisite for the next. Nothing on this path can be parallelized without
  the prior milestone being solid — rendering before transitions, transitions before topologies,
  topologies before demo, regression testing before interactivity, interactivity before SDK,
  SDK before developer release.

- [x] **Off-critical-path milestones (can run in parallel once their prerequisites are met):**
  - M8 (Shader Effects) — can begin after M2 (rendering pipeline stable). Does not block M12.
  - M9 (Video) — can begin after M7 (interactivity needed for meaningful video UX). Does not block M12.
  - M11 (Native Parity) — can begin after M5 (shared core proven). Does not block M12 directly,
    but must complete before M12 since native parity is a V1 requirement.

- [x] **M6 before M7:** visual regression testing is locked in before interactivity is introduced.
  This ensures any rendering regressions introduced during M7 work are caught immediately.
  Changing this order would mean working without a safety net on the most complex milestone.

- [x] **TypeScript SDK (M10) is V1 and required before developer release (M12).** The primary
  developer-facing API is TypeScript. A developer release without a polished TS SDK would only
  serve Rust consumers — too narrow for a public release. M10 stays on the critical path.

- [x] **M0 updated to include CI as an exit criterion.** GitHub Actions (cargo test, cargo clippy,
  cargo fmt --check, wasm-pack build) must be green before M0 is considered complete. CI configured
  late means regressions accumulate; CI from M0 keeps the bar clean from the start.

- [x] **Definition of done added to each milestone.** See the Milestones section below. Each
  milestone now has concrete, testable exit criteria — not just a description of intent.

- [x] **ROADMAP.md created** as the stable external-facing document. PLANNING.md remains the
  working document with full context, decisions, and DoD. ROADMAP.md is what an outside developer
  reads to understand where the project is going.

---

## Milestones

Finalized during Phase D. Each milestone has a definition of done — concrete, testable exit criteria.
M0 is not complete until Phases A–D are done. See ROADMAP.md for the external-facing summary.

---

### M0 — Foundation *(in progress)*

The project foundation. Nothing in M1 or beyond starts until this is complete.

**Definition of done:**
- [x] Repository initialized, Cargo workspace, all 5 crates (`proteus-gpu`, `proteus-render`,
  `proteus-ui`, `proteus-shell-web`, `proteus-shell-native`) present and compiling as stubs
- [x] Vision document (VISION.md) — Phase A complete
- [x] Architecture design (PLANNING.md) — Phase B complete
- [x] Dependencies & tooling decisions (Cargo.toml, LICENSE files) — Phase C complete
- [x] Project plan and milestones finalized (ROADMAP.md, this section) — Phase D complete
- [ ] CI: GitHub Actions running `cargo test`, `cargo clippy`, `cargo fmt --check`,
  and `wasm-pack build` on every push — all green

---

### M1 — First Pixel

wgpu device initializes, a textured quad renders. The instanced draw call is proven end to end.

**Definition of done:**
- [ ] wgpu device initializes on WebGL2 (browser via WASM) and native (macOS minimum)
- [ ] A single textured quad renders correctly on both targets — correct position, size, UV mapping
- [ ] Instance buffer works: 1000 quads render correctly in a single instanced draw call
- [ ] Unit tests: vertex layout, transform math (position, scale, rotation, anchor)
- [ ] Integration test: headless render produces expected pixel output (reference image)
- [ ] Benchmark: WASM+instanced rendering vs equivalent pure TypeScript/WebGL2 — result documented
  in a `BENCHMARKS.md` file. Must demonstrate O(1) or near-O(1) boundary crossing behavior.

---

### M2 — First Transition

The lerp transition system works end to end. One quad morphs into another.

**Definition of done:**
- [ ] `bevy_ecs` world initializes with the full system schedule running in the correct order
  (see system scheduling in Phase B)
- [ ] A 1→1 lerp transition animates correctly: position, size, color all interpolate over the
  declared duration
- [ ] Frame-rate independent: the same transition at 30fps and 60fps takes the same wall-clock
  duration (delta-time based `t` advancement via Bevy `Time` resource)
- [ ] `TransitionComplete` message fires correctly at `t = 1.0`; `Lifecycle` updates to `idle`
- [ ] Easing function is pluggable: a custom `t → t` function can be passed at the call site
- [ ] Unit tests: lerp math, `t` advancement, easing substitution, `TransitionComplete` timing

---

### M3 — All Three Topologies

All transition topologies working. A button splits into a list; the list collapses back.

**Definition of done:**
- [ ] **1→N:** a single quad splits into N target quads (N=5 minimum). Both `childBehavior: 'bake'`
  (normalize to 1→1) and the slice strategy (normalize to N→N via virtual entities) work correctly
- [ ] **N→1:** N source quads converge into one target. Virtual slice entities are created, rendered
  during the transition, and cleaned up by `transition_complete_system`
- [ ] `childBehavior` iterator works — per-child duration, delay, and easing config each applied
  independently
- [ ] Virtual entity `Virtual` marker: input and navigation systems skip virtual entities entirely
- [ ] Integration test: button → list → button round trip. ECS state (entity count, Lifecycle,
  Visibility) is correct after each transition. No entity leaks.

---

### M4 — Text Phase 1

Single-line SDF text. Components can carry readable labels.

**Definition of done:**
- [ ] Single-line SDF text renders on a component with a declared text property
- [ ] Font atlas is generated and managed through `TextureRegistry` (`TextureKind::Static`)
- [ ] Text is readable without aliasing artifacts at sizes from 12px to 48px
- [ ] Text renders correctly on WebGL2 (browser) and native
- [ ] Text content is treated as a texture in the rendering pipeline — the transition system
  handles text-bearing components identically to any other textured quad

---

### M5 — Reference Demo

The paradigm demo: button → list → detail view. Scripted, labeled, 60fps in a browser.

**Definition of done:**
- [ ] button → list → detail view transition sequence runs correctly in a browser (WebGL2)
- [ ] All components carry SDF text labels (from M4)
- [ ] Demo runs at a stable 60fps on a mid-range device throughout all transitions — no frame drops
- [ ] Demo is viewable without a local build: either hosted or a pre-built WASM artifact committed
  to the repository
- [ ] Screen recording captured and committed to `docs/` for documentation purposes
- [ ] No hardcoded timing hacks — all transitions use the declared `duration`/`easing` mechanism

---

### M6 — Visual Regression Testing

Instance-buffer regression tests. Correctness locked before interactivity.

**Approach:** instead of pixel snapshots (GPU-dependent, non-deterministic across drivers,
requires GPU in CI), tests verify the `Vec<QuadInstance>` produced by `collect_instances`.
That buffer is the complete ground truth for what appears on screen — if instance data is
correct, rendered output is correct. Tests are pure Rust, deterministic, and run in CI with
no GPU required.

**Definition of done:**
- [x] `proteus_ui::collect::collect_instances` extracted from shells into a single, tested,
  public function — shells now call it instead of duplicating the logic
- [x] `QuadInstance` derives `PartialEq` so tests can assert on exact values
- [x] `proteus-ui/tests/render_instances.rs` covers:
  - Static quad → correct position, size, color, UV in the instance buffer
  - Hidden entity (`Visibility::HIDDEN`) → excluded from buffer
  - Entity with no `Visibility` component → defaults to visible (Virtual entity case)
  - Text entity → two instances: background (WHITE_PIXEL_UV) + overlay (BakedText UV)
  - `Text::color` → applied to overlay layer, not background layer
  - 1→1 linear transition at t=0.5 → position and size are midpoints of from/to
- [x] All tests pass with `cargo test -p proteus-ui`
- [x] CI runs these tests on every push

---

### M7 — Interactivity

User input drives transitions. The reference demo becomes interactive.

**Definition of done (M7 minimal set — click + hover only):**
- [x] Hit testing: pointer events route correctly to the topmost visible, non-Virtual `Interactable`
  entity at the pointer position (AABB, accounts for `anchor`)
- [x] `clicked`, `hover_entered`, `hover_exited` events produced by `hit_test_system` each frame
- [x] `PointerInput` resource written by the shell before `update()`; one-shot flags cleared after
- [x] The reference demo is fully click-driven — idle phases wait for user click; transition phases
  block input by convention (clicks ignored during in-flight transitions)
- [x] Virtual entities are not hit-testable
- [x] Hidden entities (`Visibility::HIDDEN`) are not hit-testable
- [x] Native shell: `StagedPointer` accumulates winit `CursorMoved`/`MouseInput` events between
  frames; flushed to `PointerInput` at the start of each `render()` call
- [x] Web shell: `on_mouse_move(x, y)`, `on_mouse_down()`, `on_mouse_up()`, `on_mouse_leave()`
  wasm-bindgen methods accumulate JS pointer events; flushed to `PointerInput` at the start of
  each `tick()` call
- [x] Regression tests: `hit_test.rs` covers hidden/virtual opt-out, AABB hit, hover enter/exit
- [x] Existing visual regression tests (M6) still pass after M7 work

**Deferred to M10 (full handler API):**
- [ ] All handlers: `onPress`, `onRelease`, `onFocus`, `onBlur`, `onDrag`
- [ ] `allowInput`/`allowNavigation` transition config flags
- [ ] `signal.set()` from an `onClick` handler triggering a transition
- [ ] `CommandQueue`/`flush_commands_system` for mutation deferral from callbacks

---

### M8 — Drop Shadow *(off critical path — can begin after M2)*

SDF-based drop shadow rendered entirely in the existing fragment shader pass.
No offscreen render targets required; works on WebGL2 and native with zero
architecture change.

**Approach:** Upgrade the quad fragment shader to compute a rounded-rectangle
Signed Distance Function (SDF). The SDF gives every fragment its exact distance
to the shape boundary, enabling soft drop shadows (computed per-fragment in the
same draw call from the distance field, no separate shadow quad or render pass)
and sharper anti-aliasing as a free side-effect.

**Definition of done:**
- [x] Fragment shader upgraded to SDF-based rounded-rectangle rendering
- [x] `DropShadow` component: `offset: Vec2`, `color: Vec4`, `softness: f32`, `spread: f32`
- [x] Shadow rendered correctly for quads with and without corner radius
- [x] Works on both WebGL2 and native (no extensions required)
- [x] Anti-aliasing quality visibly improved over the previous step-function approach
- [x] Instance buffer extended to carry shadow parameters (loc 13: shadow_params, loc 14: shadow_color; +32 bytes → 156 total)
- [x] Regression tests: shadow instance data verified via `collect_instances` (3 tests in render_instances.rs)

---

### M8.5 — Blur *(off critical path — skeleton exists, NOT scheduled)*

Gaussian blur via an offscreen bake pass. Establishes the bake-to-atlas
infrastructure used by both blur and glow.

**Status:** An early skeleton (`blur.rs`, `shaders/blur.wgsl`) was created but
is intentionally **not compiled** (`mod blur` is absent from `lib.rs`).  The
skeleton is incomplete and the shader uses `@group(1) @binding(3)`, which now
collides with `video_atlas` (M9).  Before M8.5 begins, the binding collision
must be resolved and the skeleton properly completed.  Glow (M8.6) landed
without the bake infrastructure by reusing the shadow SDF path, so M8.5 does
not gate any other shipped milestone.

**Approach:** Components with a `Blur` effect render to a small offscreen texture
first (reusing the bake concept from M4 text rendering). A two-pass separable
Gaussian blur shader runs on that texture (horizontal then vertical), and the
result is written into the main atlas. The main render pass then samples the
blurred texture from the atlas.

**Definition of done:**
- [ ] Resolve `@group(1) @binding(3)` collision with `video_atlas`
- [ ] `Blur` component: `radius: f32`
- [ ] Offscreen bake pass: component renders to intermediate texture
- [ ] Separable Gaussian blur: horizontal + vertical passes
- [ ] Result composited into main atlas
- [ ] Works on WebGL2 and native
- [ ] Regression tests

---

### M8.6 — Glow *(off critical path — complete)*

Soft radial halo/glow that emanates outward from the component boundary.

**Approach (revised):** Glow reuses the existing `shadow_params`/`shadow_color`
instance slots (introduced in M8) with a zero offset, causing the SDF shadow
branch in the fragment shader to produce a symmetric halo instead of a
directional shadow.  No new vertex attributes, pipeline changes, shader changes,
or bake passes are required.  This is zero-cost when the component is absent
(`shadow_color.a == 0` → shader branch skipped).

`DropShadow` and `Glow` share the same instance slots and are mutually exclusive:
`DropShadow` takes precedence if both are attached to the same entity.

**Definition of done:**
- [x] `Glow` component: `radius: f32`, `color: Vec4`, `intensity: f32`
- [x] Reuses M8 shadow instance slots (zero-offset → symmetric halo); no GPU changes
- [x] `DropShadow` takes precedence over `Glow` when both present (documented)
- [x] `Glow` re-exported from `proteus_ui` crate root
- [x] Regression tests: 3 new tests in `render_instances.rs` (`glow_params_populate_instance`, `no_glow_by_default`, `shadow_wins_over_glow`)
- [x] Demo: `Glow` attached to the Elephant list item in `proteus-shell-native`

---

### M9 — Video *(off critical path — complete)*

Per-frame video texture streaming to the GPU. A component can morph into a playing video.

**Approach:** A new `video_atlas` wgpu texture is allocated at pipeline init (1×1 black
placeholder) and registered in a new `TextureRegistry` with `TextureKind::Video`.
`QuadPipeline::init_video(device, w, h)` reallocates at the requested resolution and
returns a `TextureId`; `upload_video_frame(queue, rgba)` streams RGBA pixel data each
frame without rebuild. The WGSL shader gains a third atlas binding (`@group(1) @binding(3)`);
`atlas_page = 2` routes fragments there. A `VideoPlayer` ECS marker component on an entity
sets `atlas_page = 2` and full-UV mapping in `collect_instances`. Backgrounding is handled
by `suspend_video` (swaps to 1×1 placeholder, rebuilds bind group) and `resume_video`.

**Definition of done:**
- [x] Per-frame video texture uploads to the GPU at the native video frame rate
- [x] A list item transitions into a playing video — the reference demo is extended to show this
- [x] Video frames managed through `TextureRegistry` using `TextureKind::Video`
- [x] Correct behavior on backgrounding: GPU memory released via `suspend_video`; `resume_video`
  re-allocates on foreground (`resumed()` now calls it when state already exists)
- [x] Latest-frame-wins delivery: producer blocks on `sync_channel(2)`, consumer drains all
  buffered frames per tick and uploads only the most recent (stale frames discarded).
  *Note: "no frame drops" in the strict sense is not guaranteed — the producer may skip frames
  under load; the guarantee is that the displayed frame is always the freshest available.*

**Web verification status:** Complete. The wgpu 22→29 upgrade resolved the
`maxInterStageShaderComponents` Chrome rejection bug that made the web shell non-functional during
M9 development; M9 infrastructure is now verified on both native and web (see M9.5 below).

---

### M9.5 — MP4 Playback *(off critical path — complete)*

Real MP4 file decoding feeding the M9 GPU streaming pipeline, on both web and native targets.
Both implementations are reference examples of "bring your own player": `proteus-render`/
`proteus-ui` know nothing about MP4, ffmpeg, or the browser's `<video>` element — they only see
[`VideoFrameSender`]/`upload_video_frame`, a plain RGBA-bytes interface. Swapping in an HLS
player, a different codec, or a hardware decoder means writing a different producer with the same
shape, not touching the framework.

**Approach (as built):**

*Web:* An HTML `<video>` element is the decoder (`crates/proteus-shell-web/www/index.html`).
Since Rust owns hit-testing (there's no per-tile DOM element for JS to attach a click listener
to), `ProteusApp` exposes `take_video_start_tile()`/`take_video_stop()` — polled once per
`tick()` — so JS knows *when* to drive the `<video>` element. Frames are pulled via
`requestVideoFrameCallback` (falling back to `requestAnimationFrame` polling on browsers without
it) onto an offscreen `<canvas>`, read back as RGBA, and pushed straight to
`QuadPipeline::upload_video_frame` via a new `push_video_frame()` entry point — no channel, no
thread, since wasm32 has neither.

*Native:* Originally attempted as a from-scratch pure-Rust decoder (`mp4` demuxer + `rust_h264`
H.264 decoder). That path hit real correctness issues — visible judder that persisted after
verifying decode-thread pacing, render-loop frame delivery, and GPU presentation timing were all
individually correct, most likely `pic_order_cnt` display-reordering edge cases in a young,
lesser-vetted decoder crate. Replaced with `crates/proteus-shell-native/src/mp4_player.rs`
shelling out to `ffmpeg`/`ffprobe` (must be on `PATH`) on a background thread: `ffmpeg -re -i
<file> -f rawvideo -pix_fmt rgba -vf scale=W:H -` streams RGBA frames from stdout, with decoding,
container demuxing, B-frame reordering, and real-time pacing all delegated to ffmpeg itself. Feeds
the existing `VideoFrameSender`/`sync_channel(2)` pipeline unchanged. `PlaybackHandle::stop` kills
the child process directly for immediate teardown.

**Definition of done:**
- [x] Web: a real `.mp4` file plays back visibly in the browser via the M9 GPU pipeline — frames
  decode correctly and display without tearing
- [x] Native: a real `.mp4` file plays back via a background decoder thread (ffmpeg subprocess)
  feeding the existing `sync_channel` pipeline, at the correct frame rate
- [x] End-to-end demo: clicking a video tile starts playback immediately (visible underneath the
  tile→screen morph) on both web and native; clicking the screen stops it
- [x] No main-thread stall on either target — decoding is fully off the render thread (native:
  background thread; web: browser's own decode pipeline, frames pushed from a `<video>` callback)
- [x] Codec support: native depends on whatever codecs the system's `ffmpeg` build supports
  (practically all common ones); web depends on the browser's built-in decoders (H.264, VP8, VP9,
  AV1 where supported) — documented here rather than a separate `CODECS.md`, since it's fully
  determined by the two "bring your own player" implementations above, not a framework choice

---

### M9.6 — Live Video Crossfade During Transitions *(off critical path — begins after M9.5)*

**Captured as a known gap, not yet implemented.** Currently, when a `VideoPlayer` entity (e.g.
the Whale list item) is swept into a bake or slice transition (`childBehavior: 'bake'`, or the
N→1/1→N slice strategy), the `bake_system` snapshots its texture into `transition_atlas` once at
transition start. For a static component this is correct. For a `VideoPlayer` component it is
not: the snapshot freezes the video at whatever frame was current when the bake fired, and the
frozen frame is what crossfades against the target for the full transition duration — playback
does not resume until the transition completes and the entity returns to live rendering.

**Desired behavior:** the video should keep streaming live throughout the transition. The morph
should crossfade the *live, still-updating* video surface into the target's background — matching
the same `mix(base_color, tex_color, crossfade_t)` blend the fragment shader already does for
static bakes (`quad.wgsl`, `atlas_page == 2` already routes to `video_atlas`) — rather than
freezing a snapshot. This is the effect that visually differentiates Proteus's morphing transitions
from a conventional crossfade-and-cut; getting it right for video specifically (the one case where
the source content is animating independently of the transition itself) is important to demo well.

**Why this is non-trivial:** the bake pipeline's whole point is to collapse a component (and its
children) into one static GPU snapshot so the transition system can treat 1→N/N→1 as ordinary
1→1 morphs. A live video source breaks that assumption — it needs `base_texture` (or whichever
side of the crossfade it occupies) to keep pointing at `video_atlas` and re-sample every frame
instead of a frozen `transition_atlas` region, while still participating in the same slice/bake
geometry math. Likely needs a per-entity flag (e.g. `Baked::live_video` or a check for
`VideoPlayer` at bake time) that skips the snapshot-into-`transition_atlas` step for that specific
sub-region and leaves its `atlas_page`/UV pointed at `video_atlas` for the duration of the
transition, while everything else around it still bakes normally.

**Definition of done:**
- [ ] A `VideoPlayer` entity that is a source or target of a bake/slice transition keeps rendering
  live video frames throughout the transition — no freeze
- [ ] The video surface crossfades into (or out of) the transition target's background using the
  existing `crossfade_t` blend, not a static snapshot
- [ ] Reference demo: the Whale item's N→1 Slice back into the button (and any future 1↔1 video
  transition) demonstrates this visibly
- [ ] Regression test verifying the baked entity's instance data still points at `atlas_page == 2`
  (not a `transition_atlas` snapshot) for the live-video sub-region during an active transition

---

### M9.7 — Static Image Support *(off critical path — moved up, needed for the reference demo)*

No image-loading pipeline exists yet — the only ways to get pixels into `main_atlas` today are the
1×1 white sentinel, baked SDF text glyphs, offscreen bake render targets, and streamed video
frames (`video_atlas`). The reference demo's video tiles currently use solid-color placeholders
standing in for real box-cover art. This milestone closes that gap: decode a static image file
(PNG/JPEG) on both targets and upload it into `main_atlas` through the same atlas-region mechanism
`FontAtlas`/`BakedText` already use for text, so a component can reference it exactly like any
other texture.

**Approach:** Add the `image` crate (pure Rust, works on both native and wasm32) as a dependency.
A new `proteus_render::static_texture` module decodes RGBA8 pixels from bytes and hands them to
the existing shelf/atlas-packing path (`etagere`, already a dependency per Phase C) — the same
`write_to_main_atlas` upload used by text baking, just with decoded image pixels instead of
rasterized glyph coverage. A new `Image` ECS component (analogous to `Text`/`BakedText`) declares
`src`/`bytes` on an entity; the shell's per-frame bake-pending pass uploads it once and inserts a
`BakedImage { uv_offset, uv_scale, pixel_size }` component, mirroring `BakedText` exactly —
including the `pixel_size`-driven quad-sizing fix from the text overlay work, so images aren't
stretched to fill an unrelated parent size either.

**Definition of done:**
- [ ] `image` crate added as a workspace dependency; decodes PNG and JPEG on native and wasm32
- [ ] `Image` component (`src: bytes` or a resolved path/URL, resolved by the shell) triggers a
  bake-and-upload pass identical in shape to the `Text` → `BakedText` flow
- [ ] `BakedImage { uv_offset, uv_scale, pixel_size }` — same shape as `BakedText`, read by
  `collect_instances` to size and UV-map the entity's quad
- [ ] Decoded images are packed into `main_atlas` via the existing atlas region allocator — no new
  atlas page required
- [ ] Reference demo: the three video tiles use real box-cover images instead of solid-color
  placeholders
- [ ] Regression tests: decode → atlas upload → instance UV/size correctness, mirroring the
  existing `BakedText` test coverage in `render_instances.rs`
- [ ] Works on both native and web (wasm32) shells

---

### M10 — TypeScript SDK *(critical path)*

A developer builds the full interactive reference demo in TypeScript without touching Rust.

**Definition of done:**
- [ ] TypeScript SDK is publishable to npm (package.json, build output, types)
- [ ] All public types fully declared — no `any`, complete IntelliSense in VS Code
- [ ] The full interactive reference demo (from M7) is rebuilt end-to-end in TypeScript using only
  the SDK — no raw wasm-bindgen output consumed directly
- [ ] All public APIs documented with usage examples (inline JSDoc minimum)
- [ ] Convenience conversions handled by the SDK: degrees→radians, hex/named colors→RGBA,
  top-left coordinate mode option for root components
- [ ] `proteus.get(id)` returns a fully typed `ComponentData` interface (no raw `JsValue`)
- [ ] The SDK wires `requestAnimationFrame` → `proteus.tick()` automatically by default;
  manual tick control is available for custom render loop integration

---

### M11 — Native Parity *(off critical path — can begin after M5)*

The full interactive reference demo runs identically on macOS, Linux, and Windows.

**Definition of done:**
- [ ] The full interactive reference demo (from M7) runs correctly on macOS, Linux, and Windows
  via the native shell (`proteus-shell-native` with `winit`)
- [ ] CI runs the test suite on all three platforms (GitHub Actions matrix)
- [ ] Visual regression tests (M6) pass on all three native platforms
- [ ] Performance benchmarks on native documented in `BENCHMARKS.md`
- [ ] No platform-specific behavioral differences in transitions, input handling, or text rendering

---

### M12 — Developer Release

Documentation, examples, and enough polish for an outside developer to pick up Proteus and build.

**Definition of done:**
- [ ] Public documentation: README covers installation, quickstart, and links to full docs;
  a `docs/` directory with API reference and at least a getting-started guide
- [ ] ≥3 complete examples in an `examples/` directory, beyond the reference demo — each
  demonstrating a distinct use case or transition pattern
- [ ] Pluggable interpolation interface is public, stable, and documented with an example
  custom easing function
- [ ] `CHANGELOG.md` exists; project is on semantic versioning (v0.1.0 minimum)
- [ ] `CONTRIBUTING.md` covers: how to build, how to run tests, PR process
- [ ] An outside developer with no prior codebase knowledge can follow the README,
  install the SDK, and produce a working component with a transition

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
- ~~How does a developer declare a transition — what is the minimum they need to provide?~~ ✅ Resolved — declared at the `signal.set()` call site: `[to, from]` plus optional `duration`, `easing`, `delay`
- ~~What triggers a transition — is it always user-initiated, or can application logic drive it?~~ ✅ Resolved — anything that calls `signal.set()`: user input handlers or application logic; cross-component triggering is first-class
- ~~What is the developer-facing API for defining a component's interaction definition?~~ ✅ Resolved — `on`-prefixed handle methods (`onClick`, `onHoverEnter`, `onDrag`, …) — see Phase A
- ~~What does the TypeScript API look like end to end for a simple component with one transition?~~ ✅ Resolved — see the Phase A developer experience example (button → list → detail)
- How do parent and child transitions coordinate — who has priority when both are triggered simultaneously?
- ~~Open from review: how do JS callbacks that re-enter the framework (`signal.set()`, `destroy()` mid-dispatch) interact with ECS system execution?~~ ✅ Resolved — command queue: all WASM handle mutations push `PendingCommand` to `CommandQueue`; `flush_commands_system` drains at the start of each `tick()`.
- ~~Open from review: differently-sized per-component textures cannot share one `texture_2d_array` (layers must be uniform size, no bindless in WebGL2) — atlas, size-classes, or padded layers?~~ ✅ Resolved — two-atlas model: `main_atlas` (long-lived, window-sized) + `transition_atlas` (ephemeral, 2× window area), UV-addressed via `uv_offset`/`uv_scale` and `base_uv_offset`/`base_uv_scale` on `QuadInstance`.

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
