# Proteus — Roadmap

> For detailed definitions of done, architecture decisions, and dependency rationale, see [PLANNING.md](./PLANNING.md). This document is the external-facing summary of where Proteus is going and in what order.

---

## Critical Path

```
M0  Foundation
 ↓
M1  First Pixel
 ↓
M2  First Transition
 ↓
M3  All Three Topologies
 ↓
M4  Text Phase 1
 ↓
M5  Reference Demo
 ↓
M6  Visual Regression Testing
 ↓
M7  Interactivity
 ↓
M10 TypeScript SDK
 ↓
M12 Developer Release
```

**Off the critical path** (prerequisites noted, can proceed in parallel once met):

- **M8 — Shader Effects** — can begin after M2
- **M9 — Video** — can begin after M7
- **M11 — Native Parity** — can begin after M5; must complete before M12

---

## M0 — Foundation *(in progress)*

Repository, workspace, crate scaffolding, CI. Vision, architecture, and tooling decisions locked.
Nothing in M1 or beyond starts until this is complete.

## M1 — First Pixel

wgpu device initializes on WebGL2 (browser) and native. A textured quad renders. The single
instanced draw call is proven end to end with a benchmark documenting the WASM boundary cost —
the core architectural bet of the framework.

## M2 — First Transition

The first 1→1 lerp transition. Two quads morph — position, size, color all interpolating smoothly
over a declared duration. `bevy_ecs` running. Frame-rate independent `t` advancement proven.
The pluggable easing interface is established here.

## M3 — All Three Topologies

All transition shapes working: 1→1, 1→N, N→1. A quad splits into N and converges back. Virtual
slice entities created and cleaned up correctly. The `childBehavior` iterator proven.

## M4 — Text Phase 1

Single-line SDF text on components. Font atlas managed through `TextureRegistry`. Components can
carry readable labels — required before the reference demo.

## M5 — Reference Demo

The paradigm demo: button → list → detail view. Scripted (not yet interactive), all components
labeled, running at 60fps in a browser. This is the thing you show someone to explain what
Proteus is.

**M5 known shortcut — Text-on-entity:** In M5, a labeled component is a single entity carrying
both a `QuadState` (background geometry) and a `Text` component (label). This is a pragmatic
shortcut: the entity transitions as one unit, and text is rendered as an overlay on the same
quad. This collapses "container + label" into a single ECS entity because M5 has no
parent/child hierarchy or relative layout.

The intended model — and the one developers will actually use — is composition: `Text` is a
leaf entity, a `Quad` is a container, and you build a button by parenting a `Text` entity
inside a `Quad` entity. The child's position is declared relative to the parent; the parent
and child can each have their own transition behavior. This requires the hierarchy
infrastructure that doesn't exist until a future milestone.

**The `Text` component as it exists in M5 is temporary API.** It will be superseded by proper
entity composition. The `with_text()` style API goes away entirely at that point.

## M5.5 — Component Composition & Hierarchy

*Prerequisite for TypeScript SDK (M10) and Interactivity (M7).*

Parent/child entity relationships, relative-coordinate `QuadState`, and cascading
visibility/opacity. This is the milestone where:

- `Text` becomes a true leaf entity with its own identity and `QuadState`
- A labeled button is composed as a `Quad` parent containing a `Text` child
- The child's position is declared relative to the parent, not in screen coordinates
- Parent transitions carry children with them by default; children can also transition
  independently (e.g., cross-fade the label while the container morphs)
- `stub_visibility_system` and `stub_opacity_system` in `schedule.rs` are replaced with real
  cascade implementations
- The M5 `Text`-on-entity shortcut is removed

## M6 — Visual Regression Testing

Headless render target, reference image capture, per-frame pixel diffing, CI integration.
Rendering correctness locked in before the complexity of interactivity is introduced. Failing
diffs surface in CI with before/after image artifacts.

## M7 — Interactivity

User input drives transitions. Hit testing, all interaction handlers, signal-triggered transitions
from callbacks. The reference demo becomes interactive. The full metamorphic paradigm is live end
to end for the first time.

## M8 — Shader Effects Library *(off critical path)*

Built-in WGSL shader effects: blur, glow, drop shadow. Composable. Each documented with an
example. A reference custom effect demonstrates the extension point for developers writing their
own WGSL effects.

## M9 — Video *(off critical path)*

Per-frame video texture streaming to the GPU. A list item transitions into a playing video. The
reference demo is extended to show this. `TextureKind::Video` in the registry.

## M10 — TypeScript SDK

A developer builds the full interactive reference demo in TypeScript without touching Rust.
Fully typed (no `any`), documented, publishable to npm. All convenience conversions handled
(degrees, hex colors, top-left coordinate mode). This is the primary developer-facing API.

## M11 — Native Parity *(off critical path)*

The full interactive reference demo runs identically on macOS, Linux, and Windows via the native
shell. CI matrix across all three platforms. Performance benchmarks documented.

## M12 — Developer Release

Documentation, ≥3 complete examples beyond the reference demo, pluggable interpolation interface
public and documented, CHANGELOG and semantic versioning, contributing guide. An outside developer
can install the SDK, follow the README, and build a working component with a transition.

---

## V1 Scope

The following are in scope for V1 and will be complete at M12:

- All three transition topologies (1→1, 1→N, N→1)
- GPU-native rendering via wgpu — WebGL2 primary, WebGPU auto-upgrade
- `bevy_ecs`-based component model with full ECS system schedule
- Single-line SDF text (M4)
- Shader effects library: blur, glow, drop shadow (M8)
- Video textures (M9)
- TypeScript SDK — the primary developer-facing API (M10)
- Native desktop parity: macOS, Linux, Windows (M11)
- Visual regression CI (M6)
- Developer documentation and examples (M12)

---

## Post-V1

Planned future work, not part of the V1 scope:

- **Text Phase 2** — multi-line layout (line breaking, alignment, line height)
- **Text Phase 3** — bidirectional text (LTR/RTL, Unicode bidi algorithm)
- **Text Phase 4** — inline styles (mixed bold, italic, size, color within a text run)
- **ECS layout system** — `VStack`, `HStack`, `Grid` with automatic transition of position
  changes (items glide when the list grows or shrinks — no manual transition calls)
- **Advanced transition effects** — non-linear easing library, particle dissolution,
  fluid deformation
- **Custom shader authoring** — formal support for developer-written WGSL effects
- **Additional geometry types** — beyond textured quads; geometry atlasing or multi-buffer model
- **XR shell** — WebXR / OpenXR
- **Additional language bindings** — Python, Swift, Kotlin, others
