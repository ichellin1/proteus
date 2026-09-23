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
M13 Application Platform Architecture
 ↓
M14 Developer Release
```

**Off the critical path** (prerequisites noted, can proceed in parallel once met):

- **M8 — Shader Effects** — can begin after M2
- **M9 — Video** (and its sub-milestones M9.5–M9.8) — can begin after M7
- **M10.5 — Static Component Baking** — can begin after M10
- **M10.6 — Oriented Hit-Test Boxes** — can begin after M10
- **M11.1 — Update Demo, Part 1** — after M11 (complete)
- **M11.2 — Multi-Page Atlas & Bounded Working Set** — after M11.1 (complete)
- **M11.3 — Update Demo, Part 2: Gallery + Examples** — after M11.2 (complete)
- **M11.4 — Host Demo on GitHub Pages** — conceptually after M11.3, no hard dependency
**Cross-shell parity is a standing requirement, not a milestone.** Every milestone's definition of
done is implicitly "works identically on the native and web shells" unless stated otherwise —
this has been true in practice since M9 and is treated as the default going forward rather than a
separate parity pass at the end. The narrower concern of native's own cross-*platform* behavior
(macOS/Linux/Windows via a CI matrix) is checked once, at M14, rather than continuously.

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

Rendering correctness locked in before the complexity of interactivity is introduced. Pixel
snapshots were considered and **deliberately not taken** — they are GPU- and driver-dependent,
non-deterministic across machines, and need a GPU in CI. What shipped asserts on the
`Vec<QuadInstance>` that `collect_instances` produces (`proteus-ui/tests/render_instances.rs`),
which is the complete ground truth for what reaches the screen: pure Rust, deterministic, no GPU.
See PLANNING.md § M6 for the full rationale.

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
for the transition's duration — `one_to_n_setup_system` unconditionally hides the source entity
and shows a baked snapshot instead, regardless of `SplitStrategy`, with no `VideoPlayer`/
`VideoCrossfade` awareness anywhere in `topology.rs`/`bake.rs`. Narrowed from its original broader
scope now that M9.8 covers the simpler 1↔1 case.

Still genuinely unsolved, and no longer hypothetical — this milestone was originally filed with a
note that no demo scene exercised the path. One does now: the reference demo's video-tile↔screen
reverse morph (`start_screen_to_tiles`, M12.5) uses exactly this combination — a `VideoPlayer`+`VideoCrossfade` entity mid-`Slice` split — and does hit the frozen-
snapshot gap this milestone describes. It just reads fine in practice, since the frozen frame is
whatever the video looked like an instant before the ~0.6s crossfade-out started, not a stale or
mismatched one.

## M9.7 — Static Image Support *(off critical path — complete)*

Decode a static image file (PNG/JPEG) on both targets and pack it into `main_atlas` through the
same shelf-packer `FontAtlas` already uses for text. Box-cover art for the reference demo's video
tiles.

## M9.8 — Live Video ↔ Box-Art Crossfade (1↔1 Transitions) *(off critical path — complete)*

Crossfades a single entity's live, still-updating video feed against its own static box-cover art
— the tiles↔screen morph in the reference demo. Built for the plain 1↔1 `TransitionRequest` case,
which is what the demo actually needed; the harder group-transition case remains M9.6.

## M10 — Component Composition & Hierarchy *(complete)*

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

(Previously numbered M5.5 and scoped as a prerequisite for M7; M7 shipped without it, so it was
rescheduled here, immediately before the SDK, where it became a real blocker.)

## M10.5 — Static Component Baking *(off critical path — complete)*

`bake: true` collapses a composite (parent + children) into a single permanent textured quad at
spawn or on-demand, destroying the child entities and freeing the ECS/render cost of the subtree.
Fully designed during Phase A of `PLANNING.md` ("Static baking — resolved") but never attached to
a milestone anywhere — the same kind of gap M11 turned out to be, caught during M10 planning.
Built on M10's hierarchy work (baking a subtree reuses the same children-walk M10 introduced for
the transition-bake crossfade) and `QuadPipeline::bake_instances_to_main_atlas`, which already
existed in `proteus-render`, unused, before this milestone wired it to a real `bake_system`.

**Scope note:** freeing a baked composite's `main_atlas` region is deferred to M11 — there is no
free/deallocate capability for `main_atlas` at all today, for any consumer (text, images, or baked
composites); building one narrowly for this milestone would preempt M11's actual unification work.
See `PLANNING.md`'s M10.5 entry for the full reasoning.

## M10.6 — Oriented Hit-Test Boxes *(off critical path — complete)*

`quad_contains`'s hit-test box was axis-aligned and ignored `QuadState::rotation` for every entity,
root or child — a pre-existing gap (`input.rs` had flagged it since M7: "good enough for M7; full
convex-hull testing can land with M5.5 hierarchy" — M5.5 being this milestone's old number). Now
inverse-rotates the point into the quad's local frame (around its anchor pivot) before testing —
a rotated button or a rotated child is hit-testable within its true rotated footprint, not the
larger axis-aligned box of its unrotated shape. Also fixed `scale` being ignored entirely, the same
underlying gap for the same reason.

## M11 — Resource Management *(complete)*

Real reference counting, eviction, and a texture lifecycle that actually matches what the
architecture specifies — identified by audit, not originally scheduled. Before this milestone
there was no reference counting anywhere, `main_atlas` entries were never freed, and
text/images/video atlases were managed by three disconnected mechanisms instead of one.

Decided: one shared `TextureRegistry` (real `SlotMap` IDs, ref-counted via a new `TextureRef`
component's `ComponentHooks`) sitting above per-kind low-level allocators — a new etagere-backed
`MainAtlasAllocator` for `main_atlas`, unchanged metadata-only tracking for the video slot.
`transition_atlas` stays separately self-managed (already correct, not this milestone's gap).
Eviction-under-pressure is restricted to unreferenced entries only, for correctness (see
PLANNING.md's M11 entry for why evicting referenced content would be unsafe given cached UVs on
components) — referenced-content eviction, a restoration mechanism, and backgrounding-driven
eviction beyond video are Post-V1. See PLANNING.md for the full DoD.

## M11.1 — Update Demo, Part 1 *(off critical path — complete)*

A dogfooding pass on the reference demo — using the framework as a real user rather than its
author, to actually exercise M11's resource management and the transition topologies under a
more demanding workload, and to bring the demo's look and feel toward a professional showcase
rather than a bare topology test harness. Shipped: a full light/dark theme toggle (whole-app
color/corner-radius/asset crossfade), a photo-gallery feature built end to end (network fetch on
both shells, themed loading state, 4×3 grid, the framework's first real use of `NToOneRequest`),
and several real bugs this dogfooding surfaced and fixed (a transition-atlas corner bleed, a
stale-bake corner-radius snap, a stale hover-state bug, and — the reason for M11.2 — a hard
`main_atlas` capacity ceiling under a moderate real workload). Gallery-at-scale and the
"Examples & Tests" section are explicitly deferred to M11.3. See PLANNING.md for the full DoD.

## M11.2 — Multi-Page Atlas & Bounded Working Set *(off critical path — complete)*

Closed the capacity gap M11.1 exposed: one fixed 2048×2048 `main_atlas` (capped there for WebGL2
parity) couldn't hold enough simultaneous content for a real dynamic-content feature layered on
top of everything else the demo already bakes into it. Two complementary fixes: generalized the
shader/pipeline's previously-hardcoded 3-way `atlas_page` branch into a real N-layer texture array
(each layer still WebGL2-safe, so capacity scales with page count, no native/web divergence), and
extended M11's existing LRU eviction to operate across that pool so the resident set stays bounded
to roughly what's visible — the actual mechanism that makes "thousands of images across the app
experience" tractable, not the page count alone. Page size/count are developer-configurable
(`AtlasConfig`, validated at startup against the real device's limits), not hardcoded. See
PLANNING.md for the full DoD, including the documented UHD-on-web limitation and the fragmentation
investigation's findings.

## M11.3 — Update Demo, Part 2: Gallery + Examples *(off critical path — complete)*

Closed the two gaps M11.1 deferred, once M11.2 provided real atlas headroom: the gallery dropped
its aggressive per-image downscale and got a working "Fetch New Images" refetch, and the
"Examples & Tests" nav button (previously an unbuilt placeholder) got real content — six
sub-sections (Effects, Text, Transforms & Animation, Stress Tests, plus two forward-looking
placeholders) reachable from a 3↔6 nav split, each morphing 1↔1 into a shared detail panel. Also
surfaced and fixed several real framework bugs along the way (text-rasterization clipping, an
opacity cascade being ignored during transition bakes, and a general "bake ignores `Visibility`"
pitfall worked around with a reusable coordinator-entity pattern). See PLANNING.md for the full
DoD and what shipped.

## M11.4 — Host Demo on GitHub Pages *(off critical path - complete)*

Building and running the demo locally is fine for a developer already comfortable with the
Rust/wasm toolchain — it shuts out anyone else visiting the project. Goal: a fully working,
publicly reachable, browser-hosted build of the web shell demo, nothing needed but a browser.
Shipped shape (the "separate public repo" this was originally filed with was reconsidered during
design — see PLANNING.md): **this** repo, with the demo's images and videos committed under
`crates/proteus-shell-web/www/`, built and deployed to GitHub Pages by `pages.yml` on every push
to `main`. `www/pkg/` (the wasm-pack output) stays gitignored and is produced by the workflow. A
second workflow, `staging-deploy.yml`, publishes a per-PR preview to `ichellin1/proteus-staging`
— see [RELEASING.md](./RELEASING.md) for the full flow.

## M12 — TypeScript SDK *(critical path — complete)*

A generic app-authoring API — `component()`, `signal()`, `texture()`, and everything Phase A of
PLANNING.md designed — built for the first time, in Rust, then wrapped for TypeScript. Fully typed
(no `any`), documented, publishable to npm. All convenience conversions handled (degrees, hex
colors, top-left coordinate mode). The TypeScript SDK's texture handle is a real wrapper over
M11's reference counting and eviction, not a stub.

**Split into five sub-milestones** (audit done alongside M11.4 handoff): the gap turned out to be
much bigger than "wrap an existing API in TypeScript" — `signal.rs` was still a one-line stub,
`Interactable` a bare marker with no per-state styles or callbacks, and there was no generic
component/signal/texture API in *any* language yet, only ~17,500 lines of hand-duplicated,
demo-specific Rust across `proteus-shell-native`'s and `proteus-shell-web`'s independent
reimplementations of the same reference demo. M12.1–M12.5 close that gap in dependency order and
are complete. A TypeScript example app was briefly opened as an M12 follow-on; its audit surfaced
that the platform shell and the app it runs are welded 1:1, and it grew into its own milestone —
**M13 — Application Platform Architecture** (the TypeScript proof is now M13.8). See PLANNING.md's
M12 entry for the full reasoning, including why the reference demo itself ends up built once in
Rust rather than rebuilt in TypeScript (a scope decision made explicitly, revising this
milestone's original DoD wording).

### M12.1 — Signal Registry & Command Queue *(critical path — complete)*

The `SignalRegistry`/`signal::set()`/`CommandQueue` machinery PLANNING.md's Phase B specified and
M7 explicitly deferred here. Pure `proteus-ui` ECS work — no new crate, no shell changes.

### M12.2 — Interaction States & Handler Events *(critical path — complete)*

The rest of what M7 deferred: per-state style overrides (hover/pressed/focused/disabled) actually
driving `QuadState`, the full handler event set (`onPress`/`onRelease`/`onFocus`/`onBlur`/
`onDrag`), and the `allowInput`/`allowNavigation` transition-config gates. Still pure `proteus-ui`.

### M12.3 — Generic Rust App API *(critical path — complete)*

`proteus-sdk` (new crate): the ergonomic `component()`/`signal()`/`texture()` surface Phase A
designed, built once in Rust on top of M12.1/M12.2's real primitives — a `Handle` type, closure-
based callbacks, `proteus::get(id)` returning `ComponentData`, composite/children declaration,
`bake: true`. Usable directly by native Rust apps; the thing M12.4 wraps 1:1 for JS.

### M12.4 — WASM Bridge & TypeScript SDK Package *(critical path — complete)*

`proteus-sdk-web` (new crate): a thin wasm-bindgen wrapper of `proteus-sdk`. Then the hand-authored
TypeScript layer on top — typed `ComponentData` interfaces, convenience conversions, `requestAnimationFrame`
auto-wiring, the real texture-handle wrapper — plus real npm packaging (package.json, tsconfig,
build → `dist/`, a `tsc --noEmit` CI step). Stops short of `npm publish`.

### M12.5 — Shared Reference-Demo Crate *(critical path — complete)*

The actual "one app, both shells" deliverable: the 9-screen reference demo, built once against
`proteus-sdk`, replacing the hand-duplicated logic in `proteus-shell-native/src/main.rs` and
`proteus-shell-web/src/lib.rs`. Both shells shrink to thin platform glue (window/canvas setup,
asset I/O, event forwarding) linking this one crate. Highest-risk step in M12 — done additively,
old demo code kept working until the new path is verified, cutover an explicit separate step.

## M13 — Application Platform Architecture *(critical path — complete)*

Everything through M12 produced a framework with exactly one app — the reference demo — welded 1:1
to two hand-rolled platform shells (each holding a concrete `demo: Demo` field and ~20
`Demo`-shaped asset methods; a second app means forking a shell). M13 breaks that apart: a
reusable **host** (GPU surface + frame loop + asset services) that runs any app, an **app↔host
contract** an app targets instead of a shell, and a **target map** for browser / native desktop /
mobile / embedded such that a new platform is a new `impl Host`, never a re-architecture. V1
release is deliberately delayed for this; what was M13 (Developer Release) is now **M14**.

Every section is designed and approved in PLANNING.md. Scoping as it landed: **M13.1** (contracts),
**M13.2** (web host), **M13.3** (native host), **M13.4** (assets, resources and video) and
**M13.8** (the TypeScript POC) are built; **M13.5** shipped a scoped build (`ProteusConfig`) on
top of its design; **M13.6** (mobile packaging) and **M13.7** (the post-V1 target map) are design
and seams only, with implementation deferred to V2. The `V1` column below records this per
section.

The **Rust / TypeScript tradeoff is deliberate and permanent**: Rust compiles to native *and*
wasm, so a Rust app is portable by compilation with no bridge; TypeScript always needs a JS
runtime (the wasm bridge everywhere, plus a JS engine wherever it runs). TS is the accessible
default; Rust is the performance path.

| § | Section | V1 |
|---|---|---|
| M13.1 | Core contracts & layering — `Renderer` primitive; `App` / `HostServices` traits; host owns `Proteus` | design + **build** |
| M13.2 | Web host — Rust crate + `ts/` layer: `run()`, canvas, rAF loop, DPI, safe-area, touch, visibility-pause, context-loss. Rust→web and TS→web both front doors. | **build** |
| M13.3 | Native host — `proteus-host-winit`; shell → thin `main()`. Windowing-agnostic (nothing below it names winit) so DRM/KMS, SDL2, mobile slot in later. | design + **build** |
| M13.4 | Asset & resource contract — textures, fonts, runtime loading; video as a host service, codec split hidden per-host | design + **build** |
| M13.5 | Configuration & memory model — `ProteusConfig` through host construction; atlas-sizing for constrained targets | design + scoped **build** |
| M13.6 | Mobile packaging — Capacitor wraps the web bundle (only path for TS logic on iOS); `create-proteus-app` scaffold + template. Write-once web + desktop + mobile. | design + template |
| M13.7 | Post-V1 target map — native mobile, embedded Linux (DRM/KMS), native smart-TV, `proteus-host-jsengine` (browser-engine-free TS-native), XR. Seams, not code. | design |
| M13.8 | TypeScript POC (was the ex-M12.6/M12.7 app) — `.ts`-only, published package only, on the M13.2 host, all three topologies. Doubles as an M14 example. | **build** |

## M14 — Developer Release

Documentation, ≥3 complete examples beyond the reference demo, pluggable interpolation interface
public and documented, CHANGELOG and semantic versioning, contributing guide. An outside developer
can install the SDK, follow the README, and build a working component with a transition. Also the
final checkpoint for the macOS/Linux/Windows CI matrix and a last cross-shell parity audit. V1
ships on the M13 platform architecture.

Carries **both** benchmarks in its DoD: the native performance numbers it already owned, and the
WASM-boundary benchmark vs. a hand-written TS/WebGL2 baseline. The latter was M1's box and was
blocked on the TypeScript SDK until M12 shipped; it has had no owner since. See
[BENCHMARKS.md](./BENCHMARKS.md).

---

## V1 Scope

The following are in scope for V1 and will be complete at M14:

- All three transition topologies (1→1, 1→N, N→1)
- GPU-native rendering via wgpu — WebGL2 primary, WebGPU auto-upgrade
- `bevy_ecs`-based component model with full ECS system schedule
- Single-line rasterized text (M4)
- Shader effects library: drop shadow, glow (M8, M8.6) — blur (M8.5) not yet built
- Video textures, MP4 playback, static images, and live crossfade (M9, M9.5, M9.7, M9.8)
- Component composition & hierarchy (M10)
- Real resource management: reference counting, eviction (M11)
- TypeScript SDK — the primary developer-facing API (M12)
- Application platform: a reusable host (browser + native desktop), an app↔host contract, and a
  TypeScript POC on the web host (M13). The full multi-target map (native mobile, embedded, XR) is
  *designed* at M13 and *built* post-V1.
- Native/web shell parity — a standing requirement across all milestones, plus a
  macOS/Linux/Windows CI matrix for native specifically, checked at M14
- Visual regression CI (M6)
- Developer documentation and examples (M14)

---

## Post-V1

Planned future work, not part of the V1 scope:

- **(HIGH PRIORITY) Rounded corners lose their rounding on the shared/sliced side of a baked
  group-transition crossfade** — every baked-crossfade virtual's container forces `corner_radius`
  to `0.0` on the theory that the baked texture's own alpha already carries the rounding; a
  same-session fix reasserted the real radius on whichever side is one independent shape's own
  geometry (safe — see `one_to_n_setup_system`/`n_to_one_setup_system` in `topology.rs`), but the
  side that's one of `n` equal crops of a single *shared* bake (the big enlarged photo mid-split,
  the loading logo mid-starburst, a nav-button-group mid-merge) still relies entirely on the
  baked texture's own antialiased alpha edge — and our atlases have no mip levels (`quad.wgsl`
  samples LOD 0 always), so that edge visibly aliases toward square under minification once a
  virtual renders much smaller than its bake, which is routine mid-morph. Confirmed still
  reproducing after the same-session fix, across both the video-tile morphs and the photo
  gallery's grid transitions. Demo workaround in the meantime: reduce corner radius so the
  aliasing is less noticeable. Candidate real fix (not yet built): dynamically generate a
  rounded-rect alpha mask on the fly, sized to each shared bake, and composite/sample it
  alongside the bake instead of relying on the bake's own baked-in rounding — would need a new
  mask atlas (or a reusable region within an existing one) and a way to address it per-instance
  in the shader.
- **No configurable render scale.** Both platforms now render at physical/device-pixel
  resolution — M13.2 made the web host DPI-aware (its canvas is sized by `devicePixelRatio`,
  not pinned to `clientWidth`/`clientHeight`), and M13.4 moved the hires-image fetch sizing into
  `DemoApp` so one implementation serves both, fixing the web side's unscaled cap along the way.
  What remains is that rendering at true display resolution is a *choice* nobody can make: on a
  2x/3x display the shader pays 4x/9x the fragment cost, which is invisible for ordinary scenes
  but real during `start_gallery_to_image` — nearly a full window of simultaneously crossfading
  quads, each running the rounded-rect SDF + border + crossfade-blend math per pixel
  (`quad.wgsl`). Sharper on Retina is a legitimate default; it just isn't free, and there's no
  way to trade it off. A capped or configurable render scale (independent of hires-fetch sizing)
  belongs to whoever tunes an app for production rather than in the framework's default — it
  would slot into `ProteusConfig::render` alongside the M13.5 fields.
- **Composite-bake transition strategy.** PLANNING's Phase B designed this as "Strategy 1 —
  Bake": before a group transition starts, flatten the N side into a *single* composite texture,
  run one ordinary 1→1 morph between two quads, then discard the composite and restore the live
  entities at their final positions. It was never built. `SplitStrategy::Bake` — the obvious
  candidate for "the thing that became Strategy 1" — was something else entirely (N independent
  1→1s that bake nothing), and was renamed `PerTarget` at the 2026-09-22 audit because the name
  taught the wrong model.
  The hypothesis is a performance one, and it is untested: on a constrained or low-end device,
  flattening a compound subtree — a grid whose every tile carries images, text and buttons —
  into one quad and morphing that should beat animating each item independently. Two shipped
  things are close but are *not* this: `SplitStrategy::Slice` does flatten the source and its
  whole subtree (`gather_bake_instances` recurses), but then hands out *crops* of that bake to N
  virtuals, so it is still N moving pieces; and `ComponentSpec::bake()` (M10.5) flattens into one
  quad permanently, destroying the children, rather than for a transition's duration.
  M14's benchmarks are what would settle whether it's worth having — nothing else in the project
  measures the per-item cost this is meant to avoid. Filed here so V2 planning can take it or
  drop it deliberately rather than rediscovering the gap.
- **Centered and outer border placement.** `QuadInstance::border_offset` and the TypeScript
  `borderAlignment: 'inner' | 'center' | 'outer'` were designed as a continuous -1.0…1.0 range,
  but only inner (`-1.0`) renders correctly: centered shows just the inner half of the band, and
  outer renders nothing, because the fragment shader discards outside the rect edge so there is
  nowhere for an outer band to land (see `quad.wgsl`'s border block, which documents this). The
  fix is to grow the quad's geometry by the border width, or render the border as its own
  instance — neither is a shader-only change. Surfaced by the 2026-09-22 audit; the limitation
  was real and documented in code but tracked in no milestone until then.
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
- **Backgrounding-driven eviction (M11 follow-up)** — real OS-level triggers (window focus/
  occlusion on native, the Page Visibility API on web) driving the registry's suspend/resume
  path automatically. M11 shipped the callable API (generalized from video's existing
  `suspend_video`/`resume_video`), not the OS-signal wiring itself — no real trigger for this
  exists anywhere in either shell yet, so wiring one is a distinct cross-platform input-handling
  problem.
- **Evicting referenced content under severe pressure, plus a restoration mechanism (M11
  follow-up)** — M11 deliberately restricts eviction to unreferenced entries only, since
  `BakedText`/`BakedImage`/`BakedComposite` cache their UV coordinates directly on the component;
  evicting a still-referenced region and letting a different texture reuse it later would make the
  original component silently render the wrong pixels. A safe version of the harder tier needs a
  restoration queue that regenerates evicted content transparently — explicitly the *owning
  entity/system's* job (re-bake the text, re-fetch the image bytes, then re-register), not the
  resource manager's, since it has no way to reproduce arbitrary content from just a `TextureId`.
- **General atlas repacking/compaction for still-referenced content (M11.2 follow-up)** —
  M11.2 investigated a "repack a partially-empty page to reclaim scattered free space" idea and
  found it blocked on the same hazard as the eviction-restoration item above: `BakedText`/
  `BakedImage`/`BakedComposite` cache `uv_offset`/`uv_scale`/`page` directly on the component, so
  moving a still-*referenced* region's pixels to consolidate space leaves some component pointing
  at stale coordinates with no error, silently rendering the wrong content. Repacking referenced
  content is the eviction-restoration problem wearing a different name — once that restoration
  mechanism exists, strategic (e.g. async, background-event-triggered) repacking becomes buildable
  on top of it, and is worth revisiting then. (Note: repacking a page whose content is *entirely
  unreferenced* is not a real problem — M11.2 confirmed `etagere` already fully reclaims a page's
  space once everything on it frees, with zero measurable fragmentation left over; see
  PLANNING.md's M11.2 section.)
- **UHD (3840×2160+) image support on the web shell (M11.2 follow-up)** — `AtlasConfig`'s
  `page_size` is still bounded by the real device's `max_texture_dimension_2d`, a hard 2048 ceiling
  under WebGL2's `downlevel_webgl2_defaults()` regardless of page count, so a single baked region
  can never exceed 2048×2048 on web today. Sketched (not built) path: tile a source image wider or
  taller than `page_size` into a grid of ≤`page_size` tiles at decode time, register each as its
  own independent atlas region, and render the logical "one image" as a small grid of adjacent
  quads with individually addressed UVs instead of one quad sampling one region. Needs new
  decode-time tiling logic and a new "this `BakedImage` is actually N regions" representation.
  Native is unaffected (8192 under plain `Limits::default()`).
- **`bake_pending_images`/`bake_system` give up after persistent failure (M11.2 follow-up)** —
  both retry every frame on failure (atlas full, decode error) with no backoff or give-up
  threshold, so a persistently-failing entity burns a decode/registration attempt indefinitely
  every single frame rather than failing once and staying failed. Real, but separate from any
  currently-scoped milestone — worth its own small follow-up (e.g. a "give up after N attempts"
  marker per entity) rather than folding into a capacity-focused milestone.
- **`TextureKind::Animated` — GIF/sprite-sheet playback (M11 follow-up)** — the enum variant is
  reserved (M11) but unimplemented. Design: neither `Static` (one fixed region, baked once) nor
  `Video` (one continuously-replaced texture slot, driven by an external decoder every tick) fits —
  GIFs/sprite sheets have small, finite, known-up-front frames, so the efficient shape is to decode
  every frame once at load time, pack each as its own `main_atlas` region exactly like `Static`, and
  cycle *which region's UV* a component samples via a playhead/frame-delay clock, never re-decoding
  on the steady-state path the way `Video` does. Needs one `TextureId` to own a `Vec` of atlas
  regions (freed together as a unit) — a real structural difference from both existing kinds.
**Platform/host targets — designed at M13.7 against the M13.1 `Host` contract, built here:**

- **Native mobile hosts** — iOS and Android on a real native surface (Metal/Vulkan), for Rust
  authors and perf-minded apps. TypeScript authors reach mobile in V1 already, via the M13.6
  Capacitor webview path.
- **Embedded Linux without a compositor** — `proteus-host-drm` (DRM/KMS + GBM + EGL) for Yocto
  kiosk / HMI / signage. Embedded *with* a compositor already runs on the M13.3 winit host.
- **Native smart-TV** — Tizen / webOS / Android TV.
- **`proteus-host-jsengine`** — a browser-engine-free TypeScript-native desktop host (embeds
  Deno/Boa/QuickJS); real native GPU for TS authors without a webview.
- **XR shell** — WebXR / OpenXR.
- **TypeScript apps' declarative parts running with no JS runtime at all** — tractable only if
  component trees / signal wiring end up serializable as data, then interpreted by a native Rust
  host. Distinct from `proteus-host-jsengine` (which runs arbitrary TS callbacks). Not designed
  further than the M13.7 note.

**Other:**

- **Additional language bindings** — Python, Swift, Kotlin, others
- **Benchmark tests** — an ongoing performance suite beyond M1's single WASM-boundary measurement
- **GUI component library** — scrolling lists, grids, forms, and other common patterns, likely
  depending on M10's composition/hierarchy work
- **Dogfooding** — build a personal website using Proteus and publish it on GitHub Pages
