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
- **The planning for past releases is archived** in [docs/archive/](./docs/archive/), as
  history: v0.1.0's milestones, M0 to M14, and each release's section from here once it ships.
  Nothing here depends on it.

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
- **Decided (2026-10-20): v0.2.0 skips drawing frames in which nothing changed.** Letting the
  frame loop sleep as well is risky for little gain, so it stays a candidate, to try as a proof
  of concept after the planned releases.
- [x] ROADMAP and PLANNING plan by release rather than milestone.
- [x] Every candidate in ROADMAP is placed in a release, kept as a candidate for later, or
  dropped with a reason.
- [x] The releases are in ROADMAP in order.
- [x] The two high-priority candidates, the shader fast path and rounded corners on baked
  slices, are decided first.

## v0.4.0 — Text

Text that runs over several lines, in several fonts, weights and styles, with styles mixed
within one text, on a text engine chosen for it. The scope is from ROADMAP's draft order, with a
maximum number of lines, measuring text before it's drawn, an optional embedded font, a text
example and the demo's Text panel added. The steps are in the order they're built; each step's
definition of done is written when it's planned.

Today `fontdue` rasterizes text one character at a time into one image per text: one line, in
the app's one font. Nothing shapes it, so there's no kerning, no ligatures and no combining
marks, and each glyph's advance is rounded up to a whole pixel, so text is wider than its font
designs it, by about half a pixel a glyph.

- **Decided (2026-11-16): the text engine is chosen by comparing three, in step 1.** A design
  note compares `fontdue`'s own layout module, the baseline, which adds nothing to the build;
  cosmic-text; and parley. For each: which crate reads fonts, which shapes and which rasterizes,
  and whether each is built on Google's fontations or on the crates before it; the size it adds
  to the wasm; bake time, from a new text benchmark; the same strings rendered by each, against
  today's; variable fonts, fallback, spans, alignment, measuring without rasterizing and inline
  boxes; and how well each is kept up. The embedded font is chosen with it.
- **Decided (2026-11-16): inline content waits for the engine.** An inline box keeps a space in
  a line for a component, which sits there as a child of the text's component, with its own
  transitions, focus and styles: an icon in a sentence, or a link that splits. An inline image
  is the simpler form, its pixels baked into the text's image. Whether either joins v0.4.0 is
  decided with the engine, at the end of step 1, since parley has inline boxes and the others
  don't.
- **Decided (2026-11-16): text wraps at a width set on the text,** not at its component's width,
  since text isn't sized to its component and components change size during transitions. By
  default it doesn't wrap, and a `\n` starts a new line, so existing text is unchanged. Lines are
  broken in logical pixels, so they break in the same places at every display scale. Wrapping
  again when a component changes size is a candidate.
- **Decided (2026-11-16): lines are aligned to the start, center or end of the text,** and the
  anchor places the text on its component, as it does today. A single line looks as it does
  today. Justified text is a candidate.
- **Decided (2026-11-16): line height is a multiple of the size,** as CSS's `line-height: 1.2`
  is, and the font's own line spacing by default.
- **Decided (2026-11-16): a text can have a maximum number of lines,** and says how it ends when
  there's more: with an ellipsis, "…" by default, or "..." if the font has no "…"; with a string
  of the app's own, such as " more"; with spans of the app's own, styled as any span is; or
  clipped, dropping the lines past the last with no mark. The ellipsis replaces as much of the
  last line as it needs, cutting between whole characters (graphemes), never within one. An
  ellipsis wider than the line is clipped, with a warning in debug builds. Text isn't cut
  through the middle of a line by height: that's the component's crop or rounded corners. An
  image in an ellipsis comes with inline content, if that joins.
- **Decided (2026-11-16): an app loads fonts, and each text chooses one.** `load_font` takes a
  font file's bytes, in Rust and TypeScript, and files it under the family name inside it. A
  text names a family, a weight from 100 to 900 and a style, normal or italic, and the nearest
  weight the family has is used, as in CSS. `text.default_font` keeps working, so nothing breaks.
- **Decided (2026-11-16): the embedded font is optional.** In Rust it's a Cargo feature,
  `embedded-font`, on by default, so a first app shows text with no setup; an app with fonts of
  its own turns it off and doesn't carry it. Without it, a text with no font to draw in draws
  nothing, with a warning saying how to fix it. TypeScript's package is built in advance, so
  there the font moves out of the wasm into a file of its own in the package, which `mount`
  fetches only when the app sets no font. The release notes tell TypeScript apps about that
  request, and that hosting the app may need the font file.
- **Decided (2026-11-16): fallback fonts are the app's, never the system's,** so text looks the
  same on every platform and nothing more is fetched on the web. The app gives an ordered list
  of fonts to try for a character the text's font doesn't have; one that none of them has draws
  as the font's missing-glyph box. Color emoji is a candidate.
- **Decided (2026-11-16): one text can mix styles, as spans.** A span is part of the text with
  its own weight, style, size or color, and takes whatever it doesn't set from the text:
  `Text::rich([Span::new("Hello "), Span::new("world").bold()])` in Rust, and
  `text: { spans: [{ content, weight, italic, sizePx, color }] }` in TypeScript. Plain `content`
  keeps working. Span colors are baked into the text's image, and the component's opacity still
  applies; a text without colored spans keeps today's white image tinted by its color.
- **Decided (2026-11-16): text can be measured before it's drawn.** `measure_text` returns a
  text's size at once, rather than after the host has baked it, which the layout system in
  v0.6.0 needs. Laying text out moves from the renderer to where the SDK can reach it, which is
  cheapest while the engine is being replaced.
- **Decided (2026-11-16): the new engine fixes kerning and the rounded advances,** and
  `TextConfig::default_size_px`, which nothing reads, is removed, with an upgrade note.
- **Decided (2026-11-16): each feature is tried by hand in an example as it lands,** and the
  demo shows each new feature. A text example starts with multi-line text, and each later step
  adds its cases to it.

### Step 1 — Comparing text engines

The design note, `docs/design/text.md`, comparing `fontdue`'s layout, cosmic-text and parley as
decided above, with the wasm's size with and without the embedded font, and a text benchmark
of short labels and wrapped paragraphs, run against today's text first. The engine, the
embedded font, and whether inline content joins v0.4.0 are decided at its end.

### Step 2 — The new engine

The chosen engine replaces `fontdue`'s character-by-character rasterizing, with the API
unchanged: single lines draw as before, now kerned and without the rounded advances. The
benchmark's before and after go in BENCHMARKS.

### Step 3 — Multi-line text

Wrapping at a width, `\n`, alignment, line height, and a maximum number of lines with each way
of ending, in Rust and TypeScript. The text example starts here.

### Step 4 — Fonts

`load_font`, families, weights and styles, in Rust and TypeScript, with `text.default_font`
still working; the embedded font made optional, as a Cargo feature and as a file of its own in
the TypeScript package.

### Step 5 — Fallback

The app's ordered list of fallback fonts, and the missing-glyph box for a character none of
them has.

### Step 6 — Inline styles

Spans with their own weight, style, size and color, in Rust and TypeScript, and spans in an
ellipsis. Inline content, if it joins, follows as a step of its own.

### Step 7 — Measuring text

`measure_text`, in Rust and TypeScript, returning the same size the text is baked at.

### Step 8 — The demo's Text panel

The demo's Text panel shows each new feature: wrapped and aligned text, line height, a maximum
number of lines, fonts and weights, fallback, and mixed styles.

### Step 9 — Docs

A Text guide, from the content guide's text section; the configuration guide's text settings;
ARCHITECTURE's text, baking, crates and known limits; and the release notes.

## Standing decisions

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
