# Proteus — Planning

The decisions and definitions of done for the work underway. [ROADMAP.md](./ROADMAP.md) gives
the order of the releases and the candidates for what comes next.

## How this document works

- **It records decisions and definitions of done.** A decision says what was decided and when,
  in a paragraph at most. A definition of done is a checklist, ticked as the work lands.
- **Reasoning longer than a paragraph goes in a design note,** in
  [docs/design/](./docs/design/README.md), linked from here: what was found or measured, the
  approaches weighed, and how to tell whether a change worked.
- **The design v0.1.0 settled is described in [ARCHITECTURE.md](./ARCHITECTURE.md),** with its
  key decisions. Changing one of them is a decision, recorded here.
- **The planning for v0.1.0, milestones M0 to M14, is archived** in
  [docs/archive/](./docs/archive/), as history. Nothing here depends on it.

## Release planning

Decides the releases: which of ROADMAP's candidates each takes, and in what order. Until it's
done, the candidates are unordered.

- **Decided (2026-10-15): releases, not milestones.** Now that Proteus is released, work is
  planned by version, each release with a theme and features of its own. Milestone numbers end
  at M14. "V2" in older documents means everything after v0.1.0, not a release.
- **Decided (2026-10-15): a release's scope is fixed.** It ships when its features are done,
  not on a date. Small fixes ride along in the next feature release, or, if found after a
  release ships, go out in a patch release.
- **Decided (2026-10-15): candidates are weighed in this order:** fixing known limits and known
  issues; what outside developers need to build real apps; effects, such as blur; then other
  platforms.
- **Decided (2026-10-15): the origin stays at the viewport's center, with y up.** Instead of
  moving it, Rust gets equivalents of TypeScript's `topLeftToWorld` and `worldToTopLeft`, and
  the guides make the origin and the conversion clear in both languages. Nothing breaks.
- **Decided (2026-10-15): dogfooding is off the roadmap.** A personal website built with Proteus
  happens outside it, and what it finds comes back to ROADMAP as candidates.
- **Decided (2026-10-19): work independent of a release is tracked in ROADMAP too,** in a
  section of its own. Rewriting the history to shrink the repository is the first such task,
  done early.
- **Decided (2026-10-19): a draft order of releases,** v0.2.0 to v0.8.0, is in ROADMAP. Each
  release is planned in detail when it comes up, and its scope can change then. Only the next
  release has a section here.
- [x] ROADMAP and PLANNING plan by release rather than milestone.
- [x] Every candidate in ROADMAP is placed in a release, kept as a candidate for later, or
  dropped with a reason.
- [x] The releases are in ROADMAP in order.
- [ ] The two high-priority candidates, the shader fast path and rounded corners on baked
  slices, are decided first.

## v0.2.0 — Rendering limits

**Draft (2026-10-19).** The scope from ROADMAP's draft order, not yet planned in detail; this
section is revised when it is. Sharper, faster and steadier rendering: the known limits in how
Proteus draws.

- [ ] **The shader fast path:** plain components, with no image, border, shadow or glow, skip
  the shader's full per-pixel work, and pixels a later opaque component covers aren't shaded.
  The benchmarks are run again and BENCHMARKS updated, checking whether frame times even out
  and whether Safari's tab still crashes at 100,000 large components.
  [Design note](./docs/design/shader-fast-path.md)
- [ ] **Rounded corners on baked slices** stay round while a slice is drawn smaller than its
  bake. [Design note](./docs/design/shared-bake-corners.md)
- [ ] `quad.wgsl` is split into named parts with its output unchanged, before the fast path
  changes it.
- [ ] The test that checks the shader's atlas-page layout against Rust's survives the shader
  being reformatted.
- [ ] Text and baked components are baked at the display's resolution and drawn at their
  logical size. [Design note](./docs/design/bake-resolution.md)
- [ ] Each frame collects, sorts and uploads only the components that changed.
  [Design note](./docs/design/per-frame-work.md)
- [ ] Benchmarks run regularly and catch regressions.
- [ ] Rust has equivalents of TypeScript's `topLeftToWorld` and `worldToTopLeft`, and the
  guides make the origin and the conversion clear in both languages.
- [ ] ARCHITECTURE's known limits no longer list what this release fixes.
- [ ] Released through the process in [RELEASING.md](./RELEASING.md).

## Standing decisions

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
