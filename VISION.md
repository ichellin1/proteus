# Proteus — Vision Document

> *"He could take the form of any creature he chose."*
> — Homer, the Odyssey

---

## What Is Proteus?

Proteus is a new UX paradigm and the GPU-native rendering framework that makes it possible.

The central thesis: **user interfaces should not have a fixed shape.** They should be capable of continuously adapting — their layout, density, visual language, and interactive surface — in response to who is using them, what the user is trying to accomplish, and the context they are operating in.

This is not theming. This is not responsive design. It is something more fundamental: an interface that **shapeshifts** as a first-class design primitive, powered by the same GPU hardware that drives games, simulations, and scientific visualization.

---

## The Problem With Static UIs

Modern UI frameworks — whether on the web, mobile, or desktop — treat an interface as a document or a scene graph with fixed component roles. A button is always a button. A sidebar is always a sidebar. The visual grammar is decided at design time and applied uniformly to everyone who uses the product.

This rigidity creates real costs:

- **Cognitive friction** for users whose context doesn't match the designer's assumptions
- **Inaccessibility** that requires manual configuration rather than being emergent and automatic
- **Wasted GPU capability** — the most powerful rendering hardware ever put in consumer devices, used almost entirely for 2D rectangles and drop shadows
- **Brittle adaptation** — "responsive design" changes layout at breakpoints, but doesn't reason about intent or role

The Proteus framework rejects these constraints at a foundational level.

---

## The Proteus Paradigm

### 1. Adaptive Structure

Every layout in a Proteus application is a **living structure** — a graph of composable regions whose topology can change. Components declare their *semantic role* rather than their fixed visual form. The framework resolves the visual expression at runtime based on context, inferred intent, and user-declared preferences.

A "navigation" component might manifest as a sidebar, a command palette, a bottom nav bar, a voice-accessible menu, or a spatial ring in XR — not based on a hard breakpoint, but based on a continuous evaluation of who is using the interface and what they are doing right now.

### 2. Fluid GPU Rendering

State transitions in Proteus are not CSS animations or pre-baked keyframe sequences. They are **physical simulations on the GPU** — mesh morphs, particle dissolves, field-driven deformation — computed in real time using WebGPU compute shaders on the web, and Vulkan/Metal/DirectX compute on native platforms.

This means:
- Transitions are continuous and interruptible, not discrete
- The "feel" of an interface can encode meaning — a component that sheds its form to become something else communicates the nature of that change through the motion itself
- Visual richness has no CPU cost penalty — it lives entirely in parallel compute

### 3. Role and Context Awareness

A Proteus application is aware of a **user context model**: who the user is (role, permissions, expertise level), what they are trying to accomplish (current task, inferred goal state), and what environment they are in (device, screen, ambient light, interaction modality). This model drives the adaptive structure and influences rendering decisions without the user needing to configure anything manually.

The context model is designed to be AI-augmented — a lightweight inference layer can continuously update the context from implicit signals, driving more precise adaptation over time.

---

## The Framework Architecture

### Layer 0 — GPU Abstraction (`proteus-gpu`)

A thin, safe abstraction over:
- **WebGPU** (web target, via WASM)
- **wgpu** (native Rust, targeting Vulkan, Metal, DX12, and OpenGL ES)

Responsibilities: device initialization, swap chains, command encoding, buffer management, texture handling, compute pipeline management.

This layer has no opinion about UI. It is a general-purpose GPU runtime.

### Layer 1 — Render Pipeline (`proteus-render`)

A retained-mode scene graph and draw call batcher built on top of `proteus-gpu`. Manages:
- A **mesh registry** for UI geometry (quads, paths, glyphs, arbitrary meshes)
- A **material system** with hot-reloadable WGSL shaders
- A **compute pipeline** for physics-driven transitions (spring systems, fluid fields, morphing)
- A signed distance field (SDF) renderer for resolution-independent vector shapes and typography

### Layer 2 — Component Model (`proteus-ui`)

The semantic component system. Components in this layer:
- Declare their **semantic role** (navigation, action, content, data, status, etc.)
- Define multiple **visual forms** — the set of renderings the component can take
- Participate in a **context bus** that informs which form is active
- Expose a **declarative transition graph** — rules for how the component moves between forms

This layer is framework-agnostic. It can be driven from Rust, bound to JavaScript/TypeScript via WASM, or consumed from any language with C FFI.

### Layer 3 — Context Engine (`proteus-context`)

The runtime that maintains the user context model and drives adaptation:
- **Role registry** — user-declared or inferred roles
- **Intent inference** — heuristic and optionally AI-augmented task modeling
- **Environment probe** — device capabilities, input modalities, viewport, ambient context
- **Adaptation rules** — a declarative rule language for mapping context → visual form choices

The context engine is designed with a clean separation between the deterministic rule layer (always present, no external dependencies) and an optional AI inference adapter that can plug in a local or remote model.

### Layer 4 — Application Shell (`proteus-shell`)

The host environment for a Proteus application:
- **Web shell**: a WASM module + WebGPU canvas, with a thin JS bridge
- **Native shell**: a windowing layer (via `winit`) with a native GPU context
- **XR shell**: (future) a WebXR or OpenXR session host

---

## Target Platforms

| Platform | Rendering Backend | Shell |
|---|---|---|
| Web (primary) | WebGPU via WASM | `proteus-shell-web` |
| macOS / Linux / Windows | wgpu → Metal / Vulkan / DX12 | `proteus-shell-native` |
| AR/VR headsets | WebXR / OpenXR | `proteus-shell-xr` *(future)* |

---

## Technology Stack

| Concern | Choice | Rationale |
|---|---|---|
| Framework core | **Rust** | Memory safety, zero-cost abstractions, WASM compilation, growing GPU ecosystem |
| GPU abstraction | **wgpu** | Single Rust API over WebGPU, Vulkan, Metal, DX12, OpenGL ES |
| Shader language | **WGSL** | First-class in WebGPU; wgpu also accepts SPIR-V for native paths |
| Web bindings | **wasm-bindgen + web-sys** | Idiomatic Rust → WASM → JS interop |
| JS/TS API layer | **TypeScript** | Developer-facing API for web consumers of the framework |
| Build system | **Cargo + trunk** (web) | Cargo for Rust workspace; trunk for WASM bundling |
| AI inference (optional) | **ONNX Runtime / Candle** | Local model inference for context engine; no cloud dependency required |

---

## What Proteus Is Not

- It is **not a game engine** — though it shares infrastructure with one
- It is **not a design tool** — though it enables a new kind of design workflow
- It is **not an AI chatbot interface** — though AI is one of several context sources
- It is **not another React/Vue competitor** — it operates at a lower level and is designed to be bound to existing component systems, not replace them

---

## Design Principles

**1. Shape is not identity.** A component's role persists across transformations. Users orient to semantic purpose, not visual form.

**2. Transitions are communication.** How an interface changes is as meaningful as what it changes to. Motion should convey, not decorate.

**3. The GPU is not a luxury.** Every modern device capable of running a browser has a GPU. Using it for UI is not extravagant — it is responsible use of available hardware.

**4. Context over configuration.** The interface should require no manual customization to serve a user well. Adaptation should emerge from observation, not settings panels.

**5. Open by default.** The framework is open source. The context engine is locally runnable. No adaptation behavior should require a cloud service.

---

## Project Roadmap

### Phase 0 — Foundation *(current)*
- [ ] Repository structure and toolchain setup
- [ ] `proteus-gpu`: wgpu device init, swap chain, basic command encoder
- [ ] `proteus-render`: static quad renderer, SDF text
- [ ] Vision document and architectural spec

### Phase 1 — Render Core
- [ ] Full mesh renderer with material system
- [ ] WGSL shader hot-reload pipeline
- [ ] Compute-driven spring physics for transitions
- [ ] WebGPU/WASM build and browser demo

### Phase 2 — Component Model
- [ ] Semantic component declarations
- [ ] Multi-form component with explicit transition graph
- [ ] Context bus implementation
- [ ] First real UI widget: adaptive navigation component

### Phase 3 — Context Engine
- [ ] Environment probe
- [ ] Deterministic adaptation rule engine
- [ ] AI inference adapter (optional, pluggable)

### Phase 4 — Shell + Integration
- [ ] Web shell with TypeScript bindings
- [ ] Native shell (macOS first)
- [ ] Developer tooling: Proteus DevTools for inspecting context and forms

### Phase 5 — XR + Ecosystem
- [ ] WebXR shell prototype
- [ ] Plugin/extension API for custom forms and context sources
- [ ] Reference application

---

## Name

Proteus was the ancient Greek sea god known for shapeshifting — he could take any form, but only revealed truth to those who could hold him through every transformation. The name captures the essential quality of the project: **identity that persists through continuous change**.

---

*This document is the living foundation of the Proteus project. It will evolve as the work does.*
