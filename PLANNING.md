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
maximum number of lines, measuring text before it's drawn, an optional embedded font, inline
boxes, a text example and the demo's Text panel added, and one image feature, images given as
raw pixels. The steps are in the order they're built; each step's
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
  don't. Decided in step 1: inline boxes join.
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
  ellipsis wider than the line wraps onto lines of its own, which the text gives up; one too
  long to fit in the maximum on its own is clipped, with a warning suggesting more lines, a
  wider wrap width or a shorter ellipsis (decided in step 3). Text isn't cut
  through the middle of a line by height: that's the component's crop or rounded corners. An
  image in an ellipsis is a component in an inline box.
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
- **Decided (2026-11-18): images given as raw pixels join v0.4.0,** though they aren't text. A
  map app built on Proteus draws SVG icons, trails and boundaries on a browser canvas, and
  today has to encode each one as a PNG for Proteus to decode again. Small, and outside the
  text work, so it's a step of its own.

### Step 1 — Comparing text engines

The design note, `docs/design/text.md`, comparing `fontdue`'s layout, cosmic-text and parley as
decided above, with the wasm's size with and without the embedded font, and a text benchmark
of short labels and wrapped paragraphs, run against today's text first. The engine, the
embedded font, and whether inline content joins v0.4.0 are decided at its end.

- **Decided (2026-11-16): the comparison harness is a workspace of its own,**
  `checks/text-engines`, so its engines stay out of Proteus's `Cargo.lock`, builds and CI, and
  the note's numbers can be measured again. It's deleted when v0.4.0 ships. Its fonts are
  fetched by a script, with their checksums, rather than kept in git.
- **Decided (2026-11-16): the wasm timings run in Node as well as a browser,** since Node runs
  wasm on Chrome's engine, V8, without a person at the browser; the page in `out/web/` runs the
  same in any browser.
- [x] The native benchmark's `text` mode, timing rasterizing alone and whole frames that bake
  every text again, with today's results in BENCHMARKS.
- [x] `checks/text-engines`: the same texts through today's text, `fontdue`'s layout,
  cosmic-text and parley, as images, sizes, timings with and without a glyph cache, and
  support; the wasm size of each; and the timings in wasm.
- [x] `docs/design/text.md`, with a recommendation.
- **Decided (2026-11-16): the engine is parley, rasterized with swash.** It has inline boxes,
  any variable axis, and a font stack that is the app's fallback list, and lays text out in
  about 60% of cosmic-text's time. It adds the most to the wasm, 827 KB gzipped, and changes its
  API every month or two, so Proteus pins one minor version and upgrades on purpose. With a
  glyph cache, it bakes labels faster than today's text. See the
  [design note](./docs/design/text.md).
- **Decided (2026-11-16): the embedded font is variable Inter, roman only,** every weight in one
  file of 458 KB gzipped, with 700 still the default weight, so existing text keeps its weight.
  An app that wants italic loads Inter Italic, or any italic, itself.
- **Decided (2026-11-16): inline boxes join v0.4.0,** as a step of their own after measuring
  text, which moves layout to where the SDK can place a component in a box. An image in a line,
  or in an ellipsis, is a component showing an image in an inline box; inline images baked into
  the text's pixels aren't built.
- [x] The engine, the embedded font, and whether inline content joins v0.4.0, decided.

### Step 2 — The new engine

parley, rasterized with swash, replaces `fontdue`'s character-by-character rasterizing, with the
API unchanged: single lines draw as before, now kerned and without the rounded advances. A glyph
cache keeps each glyph's pixels to use again, without which parley is slower than today's text.
The benchmark's before and after, and the wasm's size, go in BENCHMARKS.

- **Decided (2026-11-16): the glyph cache evicts the least recently used glyphs by size.** It
  keeps each glyph's coverage by font, glyph, size in pixels, variation, and a quarter-pixel
  horizontal offset, so glyphs are placed to within an eighth of a pixel. When its glyphs pass a
  budget in bytes, set in `ResourceConfig` beside the atlas's settings, those used longest ago
  are dropped, and rasterized again if they're needed. A glyph's pixels are copied into each
  text's image as it's baked, so the cache can drop any glyph at any time; unlike the atlas,
  nothing on screen uses it. It's cleared when the bake scale changes, as baked text is.
- **Decided (2026-11-16): an app can empty the glyph cache,** with `clear_glyph_cache` in Rust
  and `clearGlyphCache` in TypeScript, to free its memory, such as when it's hidden. Text
  already baked keeps its pixels. Proteus doesn't tell an app when it's hidden: a TypeScript
  app listens for the page's `visibilitychange`, and lifecycle callbacks are a candidate in
  ROADMAP.
- [x] parley, rasterized with swash, replaces `fontdue` in `FontAtlas`, with its API unchanged:
  kerned, placed to a quarter pixel, as wide at every scale, and a `\n` starting a new line.
- [x] The glyph cache, with its budget in `ResourceConfig` and the TypeScript config, emptied
  when the bake scale changes and by `clear_glyph_cache`.
- [x] Tests: kerning, a line's width at scales 1 and 2, descenders, the cache's budget and
  eviction order, cached and uncached pixels the same, and both ways of emptying it.
- [x] The benchmark's before and after, and the web host's wasm size, in BENCHMARKS; the
  configuration guide, ARCHITECTURE and the release notes.

### Step 3 — Multi-line text

Wrapping at a width, `\n`, alignment, line height, and a maximum number of lines with each way
of ending, in Rust and TypeScript. The text example starts here.

- **Decided (2026-11-16): a text's box is its wrap width,** when it has one, and otherwise its
  widest line. Lines align within the box, and the anchor places the box on the component, so a
  text with a wrap width doesn't change size when its words do, and centered text stays put.
  `baked_text_size` returns the box.
- **Decided (2026-11-16): a word wider than the wrap width breaks between characters,** as CSS's
  `overflow-wrap: anywhere` does, rather than running out of the box.
- **Decided (2026-11-16): the ellipsis is found by a binary search** over the text's cluster
  boundaries, each try laying out the text up to the cut and the ellipsis after it, and keeping
  the longest that fits in the maximum number of lines. It covers an ellipsis beside the text
  and one wider than a line alike.
- **Decided (2026-11-16): the options are `Text`'s fields, set with `with_` methods,** as its
  color and letter spacing are: `with_wrap_width`, `with_align`, `with_line_height`,
  `with_max_lines`, `with_ellipsis` and `with_overflow`. `proteus-render` has a `TextLayout` of
  its own, since it can't see `Text`, and `FontAtlas::rasterize` lays one out at a scale;
  `rasterize_text` stays, and calls it.
- **Decided (2026-11-16): the example is a reading app, `essays`,** whose cards' texts show
  each option once, opening into an essay whose alignment and line height can be changed. Later
  steps add their cases to it. **Decided (2026-11-19), after the user's review:** the essays are
  real, public-domain ones from Project Gutenberg, each cited; the settings bar shows only while
  an essay is open, since it changes only the essay, on the title's line at the window's right
  margin; a chosen setting's button is disabled, so it stays in the accent color under the
  pointer; the cards have no photo placeholders; and a card's source is clipped after two
  lines.
- [x] Wrapping, alignment, line height, a maximum number of lines, and the ellipsis and clipping,
  in `proteus-render`, `Text`, and TypeScript's `TextSpec`.
- [x] Tests: the wrap width and the box, the same breaks at three scales, a long word, each
  alignment, line height, clipping, an ellipsis beside the text, wrapping onto its own lines,
  and too long to fit, never splitting a character, the "..." fallback, and the TypeScript
  fields and their bad values.
- [x] `examples/essays`, in Rust and TypeScript, wired into the workspace, CI, the Makefile, the
  pre-push hook and the examples table.
- [x] The content guide, ARCHITECTURE's known limits and the release notes.

### Step 4 — Fonts

`load_font`, families, weights and styles, in Rust and TypeScript, with `text.default_font`
still working; the embedded font, now variable Inter, made optional, as a Cargo feature and as a
file of its own in the TypeScript package.

- **Decided (2026-11-19): a text in a font that's still loading waits, not drawn,** by default:
  a text that changes font once drawn looks like a mistake. An app says a family is coming with
  `expect_font`, and how its texts are drawn meanwhile: `WhileLoading::Hide`, the default, or
  `WhileLoading::DefaultFont`, in the default font and again in their own once it loads, like
  CSS's `font-display`. If the load fails, `font_failed` draws them in the default font. A family
  never expected or loaded is drawn in the default font, with a warning, since it's most likely
  misspelled. TypeScript's `loadFont` does all three from a promise, with the family named. An
  expected family that never loads or fails keeps its texts hidden; there's no timeout.
- **Decided (2026-11-19): fonts are queued, and the renderer adds them,** since the renderer
  lays text out until step 7 moves fonts to where the SDK can reach them. `load_font` checks the
  bytes and returns the family name at once.
- **Decided (2026-11-19): a text that sets no weight is bold in the embedded Inter,** as the
  static Inter Bold was, and in the font's own weight in an app's own default font, so neither
  changes. A weight a family lacks uses its nearest, and a missing italic or bold face is
  synthesized, slanted 14° or thickened by a thirtieth of the size, as parley suggests.
- **Decided (2026-11-19): the TypeScript package's font is copied in by its `prebuild`,** from
  `proteus-render`'s assets, rather than kept twice in the repository, and every workflow builds
  the package's host wasm with `--no-default-features`.
- **Decided (2026-11-19): `TextConfig::default_size_px` is removed here,** as decided for
  v0.4.0; step 2 missed it.
- [x] `load_font`, `expect_font`, `font_failed` and `font_status` in Rust; `loadFont`, with bytes
  or a promise, in TypeScript; `family`, `weight` and `italic` on texts in both.
- [x] Variable Inter embedded, behind `embedded-font`, on by default in `proteus-runtime` and the
  hosts; the TypeScript package fetching it from `fonts/` when no font is set.
- [x] Tests: loading and family names, a loaded family, weights, the nearest weight, a real and
  a synthesized italic, an atlas with no font, waiting for an expected font, the default font
  meanwhile, a failed or unknown family, bad bytes, and the TypeScript fields.
- [x] The essays example in Lora, with Inter's weights and a synthesized italic; TypeScript
  downloading Lora with the promise form.
- **Decided (2026-11-19), after the user's review: the essays example gets a text-editor
  toolbar,** of icon buttons with no labels, setting the body's size, alignment, bold and
  italic, line height and font. Its icons are Material Icons, a font, drawn through its
  ligatures. The second font, Crimson Pro, is downloaded only when it's first chosen, expected
  with `WhileLoading::DefaultFont`, in Rust through the host's `fetch_async`, so both ways of
  waiting are shown.
- **Decided (2026-11-19): a component's interaction styles can change while the app runs,**
  with `Handle::set_style` and `setStyle`, so a chosen or toggled-on button drops its hover
  style and stays as it is under the pointer. Disabling a chosen button did that before, but a
  toggle must stay clickable, and a disabled button lost keyboard focus. A style changed while
  it shows is applied again.
- [x] `set_style` in Rust and TypeScript, with a test that changes a style while it shows; the
  interaction guide.
- [x] The essays toolbar, in Rust and TypeScript, with tests for the body's settings, chosen
  buttons under the pointer, and Crimson Pro loading once chosen.
- [x] The content guide's Fonts section, the configuration guide, ARCHITECTURE and the release
  notes.

### Step 5 — Fallback

The app's ordered list of fallback fonts, and the missing-glyph box for a character none of
them has.

### Step 6 — Inline styles

Spans with their own weight, style, size and color, in Rust and TypeScript, and spans in an
ellipsis.

### Step 7 — Measuring text

`measure_text`, in Rust and TypeScript, returning the same size the text is baked at.

### Step 8 — Inline boxes

A space kept in a line of text for a component, which sits there as a child of the text's
component, flowing with the text as it wraps, in Rust and TypeScript; and a component in an
ellipsis.

### Step 9 — Images as raw pixels

An image given as RGBA pixels and their size, rather than as a PNG or JPEG file, so an app that
draws its own pixels, such as on a browser canvas, skips encoding a file for Proteus to decode
again. Everything after decoding is unchanged: `max_side`, scaling down to fit an atlas page,
and premultiplying on upload.

- **Decided (2026-11-18): the pixels are straight alpha, sRGB, 4 bytes each,** row by row from
  the top, which is what decoding a file gives today and what a canvas's `getImageData` returns,
  so neither side converts them.
- **Decided (2026-11-18): `Image::from_rgba(width, height, pixels)` sits beside `Image::new`,**
  and `Image` holds either a file's bytes or the pixels with their size. In TypeScript,
  `ImageSpec` is either `{ bytes, maxSide }` or `{ pixels, width, height, maxSide }`, where
  `pixels` is a `Uint8Array` or a `Uint8ClampedArray`, so an `ImageData` is passed as it is.
- **Decided (2026-11-18): pixels of the wrong length are dropped with a warning,** as a broken
  file is today: anything other than `width × height × 4` bytes, or a width or height of 0.
- **Decided (2026-11-18): it isn't zero-copy.** The pixels are copied out of the canvas and into
  the wasm's memory; what it saves is the encode and the decode. Uploading an `ImageBitmap` to
  the GPU directly is a candidate.
- [ ] `Image::from_rgba`, `Handle::set_image` taking either form, and TypeScript's `ImageSpec`.
- [ ] Tests: pixels baked the same as the same image decoded from a PNG, `max_side` and an
  image larger than an atlas page, each wrong length and a zero size, and the TypeScript
  fields and their bad values.
- [ ] The gallery example shows a tile from raw pixels the app makes itself: drawn on a canvas
  in TypeScript, computed in Rust.
- [ ] The content guide's images section, a canvas recipe for TypeScript, ARCHITECTURE's image
  baking, and the release notes.

### Step 10 — The demo's Text panel

The demo's Text panel shows each new feature: wrapped and aligned text, line height, a maximum
number of lines, fonts and weights, fallback, mixed styles and inline boxes.

### Step 11 — Docs

A Text guide, from the content guide's text section; the configuration guide's text settings;
ARCHITECTURE's text, baking, crates and known limits; and the release notes.

## Standing decisions

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
