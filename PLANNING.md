# Proteus — Planning

The decisions and definitions of done for the work underway. [ROADMAP.md](./ROADMAP.md) gives
the order and the candidates for what comes next.

## How this document works

- **It records decisions and definitions of done.** A decision says what was decided and when,
  in a paragraph at most. A definition of done is a checklist, ticked as the work lands.
- **Reasoning longer than a paragraph goes in a design note,** in
  [docs/design/](./docs/design/README.md), linked from here: what was found or measured, the
  approaches weighed, and how to tell whether a change worked.
- **The design V1 settled is described in [ARCHITECTURE.md](./ARCHITECTURE.md),** with its key
  decisions. Changing one of them is a decision, recorded here.
- **V1's planning is archived** in [docs/archive/](./docs/archive/), as history. Nothing here
  depends on it.

## M14 — Developer Release (finishing)

Makes Proteus ready for outside developers, and ends by publishing v0.1.0 to npm and crates.io.
Steps 1 to 12 are done; their checklists are in the archived PLANNING. These two remain.

### Step 13 — README

The README is what everyone sees first, on GitHub, npm and crates.io. Done last, so it can point
at finished docs, examples and benchmarks.

- [x] The Proteus logo at the top, switching between the light and dark lockups to match the
  reader's theme (`brand/logo/assets/`).
- [x] In order: what Proteus is, a picture or short clip of the demo, links to the
  getting-started guides for Rust and TypeScript, links to the docs, examples, demo and
  architecture, project status (0.x is an early release), and license.
- **Decided (2026-10-14):** the README links to the getting-started guides for the quickstart
  and install instructions rather than repeating their code, so there is one copy to keep up to
  date, and it is already checked in CI.
- [ ] Renders correctly on GitHub, npm and crates.io; images use URLs that work off GitHub.
- [x] The README's links resolve, checked by `proteus-docs`'s link test.

### Release — v0.1.0

- [ ] The release process in [RELEASING.md](./RELEASING.md) runs for real: tag, changelog,
  GitHub Release, npm and crates.io. Publishing cannot be undone, so it goes ahead only on
  explicit approval.

## V2 Planning

The first V2 milestone: it decides what V2 is. Until it runs, ROADMAP's candidates are
unordered.

- [ ] Every candidate in ROADMAP is placed in a V2 milestone, kept as a candidate for later, or
  dropped with a reason.
- [ ] V2's milestones are in ROADMAP in order, each with a definition of done here.
- [ ] The two high-priority candidates, the shader fast path and rounded corners on baked
  slices, are decided first.

## Decisions carried into V2

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
