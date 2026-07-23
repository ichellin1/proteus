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
M10 Component Composition & Hierarchy
 ↓
M11 Resource Management
 ↓
M12 TypeScript SDK
 ↓
M13 Developer Release
```

**Off the critical path** (prerequisites noted, can proceed in parallel once met):

- **M8 — Shader Effects** — can begin after M2
- **M9 — Video** (and its sub-milestones M9.5–M9.8) — can begin after M7
- **M10.5 — Static Component Baking** — can begin after M10
- **M10.6 — Oriented Hit-Test Boxes** — can begin after M10

**Cross-shell parity is a standing requirement, not a milestone.** Every milestone's definition of
done is implicitly "works identically on the native and web shells" unless stated otherwise —
this has been true in practice since M9 and is treated as the default going forward rather than a
separate parity pass at the end. The narrower concern of native's own cross-*platform* behavior
(macOS/Linux/Windows via a CI matrix) is checked once, at M13, rather than continuously.

---

## M0 — Foundation *(complete)*

Repository, workspace, crate scaffolding, CI. Vision, architecture, and tooling decisions locked.
Nothing in M1 or beyond starts until this is complete.

## M1 — First Pixel *(complete)*

wgpu device initializes on WebGL2 (browser) and native. A textured quad renders. The single
instanced draw call is proven end to end. (The WASM-boundary-cost benchmark specifically is still
open — methodology is written in `BENCHMARKS.md`, results are not yet recorded.)

## M2 — First Transition *(complete)*

The first 1→1 lerp transition. Two quads morph — position, size, color all interpolating smoothly
over a declared duration. `bevy_ecs` running. Frame-rate independent `t` advancement proven.
The pluggable easing interface is established here.

## M3 — All Three Topologies *(complete)*

All transition shapes working: 1→1, 1→N, N→1. A quad splits into N and converges back. Virtual
slice entities created and cleaned up correctly. The `childBehavior` iterator proven.

## M4 — Text Phase 1 *(complete)*

Single-line rasterized text (fontdue-based anti-aliased coverage, not a true SDF — that's future
work if ever needed) on components. Components can carry readable labels. Font-atlas *lifecycle*
management (reference counting, eviction) is **not** part of this milestone — it's a
resource-management concern, tracked at M11.

## M5 — Reference Demo *(complete)*

The paradigm demo: a button expands into multiple tiles, one of which expands further into a
detail/screen view, and collapses back. Interactive, running in both native and browser shells.
Video playback specifically is not part of this milestone's scope — that's M9/M9.5/M9.8's job;
this milestone is about the transition-topology structure standing on its own.

**M5 known shortcut — Text-on-entity:** In M5, a labeled component is a single entity carrying
both a `QuadState` (background geometry) and a `Text` component (label). This is a pragmatic
shortcut: the entity transitions as one unit, and text is rendered as an overlay on the same
quad. This collapses "container + label" into a single ECS entity because M5 has no
parent/child hierarchy or relative layout.

The intended model — and the one developers will actually use — is composition: `Text` is a
leaf entity, a `Quad` is a container, and you build a button by parenting a `Text` entity
inside a `Quad` entity. The child's position is declared relative to the parent; the parent
and child can each have their own transition behavior. This requires the hierarchy
infrastructure that doesn't exist until M10.

**The `Text` component as it exists in M5 is temporary API.** It will be superseded by proper
entity composition at M10. The `with_text()` style API goes away entirely at that point.

## M6 — Visual Regression Testing *(complete)*

Headless render target, reference image capture, per-frame pixel diffing, CI integration.
Rendering correctness locked in before the complexity of interactivity is introduced. Failing
diffs surface in CI with before/after image artifacts.

## M7 — Interactivity *(complete — minimal set)*

User input drives transitions. Hit testing, click/hover events, signal-triggered transitions
from callbacks. The reference demo becomes interactive. The full metamorphic paradigm is live end
to end for the first time. The full handler API (`onPress`/`onRelease`/`onFocus`/`onBlur`/`onDrag`,
`CommandQueue`) is deferred to M12 (TypeScript SDK).

## M8 — Drop Shadow *(off critical path — complete)*

SDF-based drop shadow rendered in the existing fragment shader pass — no offscreen render targets,
no architecture change, works on WebGL2 and native identically.

## M8.5 — Blur *(off critical path — not started)*

Gaussian blur via an offscreen bake pass. An early skeleton existed but was removed during a later
cleanup pass; this milestone starts from nothing.

## M8.6 — Glow *(off critical path — complete)*

Soft radial halo/glow, implemented by reusing M8's shadow instance slots with a zero offset — no
new GPU state, no bake pass.

## M9 — Video *(off critical path — complete)*

Per-frame video texture streaming to the GPU via a generic RGBA-bytes channel — the framework
knows nothing about codecs or players, only how to display frames it's handed. `TextureKind::Video`
in the registry.

## M9.5 — MP4 Playback *(off critical path — complete)*

Real MP4 decoding feeding M9's pipeline, on both targets, each a reference "bring your own player"
example: native shells out to `ffmpeg` on a background thread; web uses the browser's own
`<video>` element and `requestVideoFrameCallback`.

## M9.6 — Live Video Crossfade During Bake/Slice Group Transitions *(off critical path — not started)*

The harder half of the original live-crossfade problem: a `VideoPlayer` entity swept into a group
transition (`OneToNRequest`/`NToOneRequest`) still gets its texture frozen into a static snapshot
for the transition's duration. Narrowed from its original broader scope now that M9.8 covers the
simpler 1↔1 case. No demo scene currently exercises this path.

## M9.7 — Static Image Support *(off critical path — complete)*

Decode a static image file (PNG/JPEG) on both targets and pack it into `main_atlas` through the
same shelf-packer `FontAtlas` already uses for text. Box-cover art for the reference demo's video
tiles.

## M9.8 — Live Video ↔ Box-Art Crossfade (1↔1 Transitions) *(off critical path — complete)*

Crossfades a single entity's live, still-updating video feed against its own static box-cover art
— the tiles↔screen morph in the reference demo. Built for the plain 1↔1 `TransitionRequest` case,
which is what the demo actually needed; the harder group-transition case remains M9.6.

## M10 — Component Composition & Hierarchy *(not started)*

Parent/child entity relationships, relative-coordinate `QuadState` (position, rotation, *and*
scale all compose down the parent chain — not position alone), and cascading visibility/opacity.
This is the milestone where:

- `Text` becomes a true leaf entity with its own identity and `QuadState`
- A labeled button is composed as a `Quad` parent containing a `Text` child
- The child's position, rotation, and scale are declared relative to the parent, not in screen
  coordinates — a rotated or scaled parent correctly rotates/scales its children too
- Parent transitions carry children with them by default; children can also transition
  independently (e.g., cross-fade the label while the container morphs)
- `stub_visibility_system` and `stub_opacity_system` in `schedule.rs` are replaced with real
  cascade implementations
- The M5 `Text`-on-entity shortcut is removed
- `Interactable` children hit-test correctly against their resolved world position (previously
  every entity was flat, so this never came up)

(Previously numbered M5.5 and scoped as a prerequisite for M7; M7 shipped without it, so it's
rescheduled here, immediately before the SDK, where it becomes a real blocker.)

## M10.5 — Static Component Baking *(off critical path — not started)*

`bake: true` collapses a composite (parent + children) into a single permanent textured quad at
spawn or on-demand, destroying the child entities and freeing the ECS/render cost of the subtree.
Fully designed during Phase A of `PLANNING.md` ("Static baking — resolved") but never attached to
a milestone anywhere — the same kind of gap M11 turned out to be, caught during M10 planning.
Builds directly on M10's hierarchy work (baking a subtree needs the same children-walk M10
introduces for the transition-bake crossfade), and `QuadPipeline::bake_instances_to_main_atlas`
already exists in `proteus-render`, unused — the primitive is there, just never wired to an ECS
component/system.

## M10.6 — Oriented Hit-Test Boxes *(off critical path — not started)*

`quad_contains`'s hit-test box is axis-aligned and ignores `QuadState::rotation` for every entity,
root or child — a pre-existing gap (`input.rs` has flagged it since M7: "good enough for M7; full
convex-hull testing can land with M5.5 hierarchy" — M5.5 being this milestone's old number). Small,
but easy to lose track of once the hierarchy work lands, so it gets its own explicit slot: a
rotated button or a rotated child should be hit-testable within its true rotated footprint, not the
larger axis-aligned box of its unrotated shape.

## M11 — Resource Management *(not started)*

Real reference counting, eviction, and a texture lifecycle that actually matches what the
architecture specifies — identified by audit, not originally scheduled. Today there's no
reference counting anywhere, `main_atlas` entries are never freed, and text/images/video atlases
are managed by three disconnected mechanisms instead of one. This milestone closes that gap (or
explicitly documents why the three-way split should stay).

## M12 — TypeScript SDK *(critical path)*

A developer builds the full interactive reference demo in TypeScript without touching Rust.
Fully typed (no `any`), documented, publishable to npm. All convenience conversions handled
(degrees, hex colors, top-left coordinate mode). This is the primary developer-facing API. The
SDK's texture handle is a real wrapper over M11's reference counting and eviction, not a stub.

## M13 — Developer Release

Documentation, ≥3 complete examples beyond the reference demo, pluggable interpolation interface
public and documented, CHANGELOG and semantic versioning, contributing guide. An outside developer
can install the SDK, follow the README, and build a working component with a transition. Also the
final checkpoint for the macOS/Linux/Windows CI matrix and a last cross-shell parity audit.

---

## V1 Scope

The following are in scope for V1 and will be complete at M13:

- All three transition topologies (1→1, 1→N, N→1)
- GPU-native rendering via wgpu — WebGL2 primary, WebGPU auto-upgrade
- `bevy_ecs`-based component model with full ECS system schedule
- Single-line rasterized text (M4)
- Shader effects library: drop shadow, glow (M8, M8.6) — blur (M8.5) not yet built
- Video textures, MP4 playback, static images, and live crossfade (M9, M9.5, M9.7, M9.8)
- Component composition & hierarchy (M10)
- Real resource management: reference counting, eviction (M11)
- TypeScript SDK — the primary developer-facing API (M12)
- Native/web shell parity — a standing requirement across all milestones, plus a
  macOS/Linux/Windows CI matrix for native specifically, checked at M13
- Visual regression CI (M6)
- Developer documentation and examples (M13)

---

## Post-V1

Planned future work, not part of the V1 scope:

- **Text Phase 2** — multi-line layout (line breaking, alignment, line height)
- **Text Phase 3** — bidirectional text (LTR/RTL, Unicode bidi algorithm)
- **Text Phase 4** — inline styles (mixed bold, italic, size, color within a text run)
- **True SDF text** — resolution-independent glyph rendering, if the M4 rasterized approach ever
  proves insufficient
- **Live video crossfade during group transitions (M9.6)** — if not completed as part of V1
- **ECS layout system** — `VStack`, `HStack`, `Grid` with automatic transition of position
  changes (items glide when the list grows or shrinks — no manual transition calls). Also where
  declarative/relative child positioning belongs — e.g. a child declaring its position as "center"
  or as a percentage of its parent's current geometry — and the responsive re-layout that implies
  when a parent's geometry changes, including mid-transition. M10's world-position resolution is
  recomputed fresh from current parent+child state every frame specifically so this can slot in
  later without changing the resolution/render/bake pipeline it establishes.
- **Advanced transition effects** — non-linear easing library, particle dissolution,
  fluid deformation
- **Custom shader authoring** — formal support for developer-written WGSL effects
- **Additional geometry types** — beyond textured quads; geometry atlasing or multi-buffer model
- **XR shell** — WebXR / OpenXR
- **Additional language bindings** — Python, Swift, Kotlin, others
- **Benchmark tests** — an ongoing performance suite beyond M1's single WASM-boundary measurement
- **GUI component library** — scrolling lists, grids, forms, and other common patterns, likely
  depending on M10's composition/hierarchy work
- **Embedded systems demo** — native shell on Android TV / Raspberry Pi 4
- **Dogfooding** — build a personal website using Proteus and publish it on GitHub Pages
