# Choosing a text engine

**Status:** decided in step 1 of v0.4.0, as recommended: parley, rasterized with swash, with a
glyph cache that evicts the least recently used glyphs; variable Inter as the embedded font; and
inline boxes in v0.4.0. See PLANNING. The comparison harness is `checks/text-engines`, a
workspace of its own, deleted when v0.4.0 ships.

Proteus draws text with `fontdue`, one character at a time, into one image per text: one line,
in one font. v0.4.0 needs text that wraps, aligns, mixes weights and styles, and falls back to
another font for characters the first lacks, and v0.6.0's layout system needs text measured
before it's drawn. This note compares three ways to get there: `fontdue`'s own layout module,
cosmic-text, and parley.

## Words used here

- **Glyph:** a shape in a font. Usually one per character, but not always: "fi" can be one
  glyph, and one Hindi syllable can be several.
- **Advance:** how far the pen moves right after drawing a glyph, before drawing the next.
- **Kerning:** adjustments to the advance for particular pairs, such as pulling "A" and "V"
  together. Older fonts keep them in a `kern` table; most modern ones, Inter among them, keep
  them in `GPOS`, which only a shaper reads.
- **Shaping:** turning characters into positioned glyphs, by the font's own rules: kerning,
  ligatures ("fi" as one glyph), accents placed on letters, and the joining that scripts such
  as Hindi and Arabic need. HarfBuzz is the standard shaper; harfrust is its Rust port.
- **Grapheme:** what a reader sees as one character, which can be several Unicode characters,
  such as "e" followed by a combining accent.
- **Variable font:** one file holding a range of designs along **axes**, such as weight from 100
  to 900. Inter's has a weight axis and an optical size axis, which adjusts the design for the
  size it's drawn at.
- **Rasterizing:** turning a glyph's outline into pixels with coverage, how much of each pixel
  the shape covers. **Hinting** nudges outlines to the pixel grid at small sizes; none of these
  engines hints here, as Proteus doesn't today.
- **Subpixel positioning:** placing a glyph at a fraction of a pixel, by rasterizing it with that
  offset, rather than rounding every glyph to a whole pixel.
- **Glyph cache:** keeping each glyph's pixels after rasterizing it, keyed by font, glyph, size
  and offset, to copy rather than rasterize again.
- **Fallback:** drawing a character the text's font lacks in another font that has it.
- **Inline box:** space kept in a line of text for something that isn't text, which flows with
  the text as it wraps. In Proteus, a component would sit there.

## The candidates

| | Reads fonts | Shapes | Breaks lines | Rasterizes | On fontations |
|---|---|---|---|---|---|
| **`fontdue` layout** 0.9.5 | `fontdue`'s own parser | Doesn't: a glyph per character | Its own, at spaces | `fontdue` | No |
| **cosmic-text** 0.19.0 | skrifa (fontations) and fontdb | harfrust 0.5 | `unicode-linebreak` | swash, through its `SwashCache` | Reading and shaping; swash and fontdb are the older stack |
| **parley** 0.13.0 | skrifa (fontations) and fontique | harfrust 0.15 | ICU4X's segmenter | Left to the app: swash here, as parley's examples do | Reading, shaping and fonts; swash is the older stack |

Proteus today uses `fontdue` 0.8's rasterizer, without its layout module.

## How it was measured

`checks/text-engines` lays out and rasterizes the same texts with each engine, into one image of
coverage per text, as Proteus bakes text today, and with today's own `FontAtlas` beside them.
Glyph images are composited the same way for every engine. It runs natively, and in wasm in Node
and a browser. The fonts are today's static Inter Bold; variable Inter and Inter Italic; and
Noto Sans Devanagari, for Hindi, which Inter lacks. All are from Google Fonts' repository, under
the SIL Open Font License.

The native numbers are from a MacBook Air (Apple M4) on macOS 26.5.2, and the wasm numbers from
Node 24.18 on the same machine, on 16 November 2026.

## Output

`out/<case>-<scale>x.png` has a row per engine: today, `fontdue` layout, cosmic-text, parley.
The images are for a person to judge; what can be measured:

| Case | Today | `fontdue` layout | cosmic-text | parley |
|---|---:|---:|---:|---:|
| "AVATAR Wolf Ty. To", 48 px | 494.0 | 494.0 | 463.7 | 462.2 |
| "office waffle -> go", 48 px | 438.5 | 438.5 | 421.9 | 421.9 |
| A 43-character line at 12 px, scale 1 | 281.0 | 281.0 | 261.3 | 261.3 |
| The same line at scale 2 | 270.5 | 270.5 | 261.3 | 261.3 |
| A paragraph wrapped at 360 px | — | 6 lines | 5 lines | 5 lines |

Widths in logical pixels.

- **Today's text is 6 to 8% wider than its font designs it.** Rounding every advance up adds
  about half a pixel a glyph, and Inter's kerning, in `GPOS`, is never applied. The shaping
  engines agree with each other to within 1.5 pixels.
- **Today's text changes width with the display's scale:** the same line is 281 pixels wide at
  a scale of 1 and 270.5 at 2, since the rounding is in display pixels. Wrapped text would
  break in different places on different displays. The shaping engines measure the same at both.
- **`fontdue`'s layout is today's spacing.** It reads only a `kern` table, which Inter doesn't
  have, so it has the same width as today and wraps the paragraph onto a sixth line.
- **Hindi needs shaping.** Without it, `fontdue` draws "नमस्ते" as separate letters with a
  visible virama, the mark that should join them; cosmic-text and parley join them.
- **Weights need variable fonts.** `fontdue` draws every weight of variable Inter as its default,
  Regular; cosmic-text and parley draw Thin to Black.
- **Only parley keeps space for an inline box.** The others draw the placeholder character as
  the font's missing-glyph box.
- **Ligatures:** Inter's "fi" and "ffl" look the same either way, but its `->` becomes an
  arrow only through shaping, in `ligatures-2x.png`.

## Speed

Natively, medians in milliseconds. **Layout** is laying out alone, which `measure_text` would
be. **Rasterizing** is laying out and rasterizing every glyph from scratch. **Warm cache** is the
same with a glyph cache that has seen the glyphs before, as a real implementation would keep.

| Workload | Scale | Engine | Layout | Rasterizing | Warm cache |
|---|---:|---|---:|---:|---:|
| 500 labels | 1 | Today | — | 2.51 | — |
| | | `fontdue` layout | 0.10 | 2.30 | 0.66 |
| | | cosmic-text | 2.01 | 7.40 | 2.77 |
| | | parley + swash | 1.24 | 6.09 | 2.24 |
| 500 labels | 2 | Today | — | 5.63 | — |
| | | `fontdue` layout | 0.10 | 5.01 | 1.83 |
| | | cosmic-text | 2.06 | 11.36 | 4.16 |
| | | parley + swash | 1.26 | 10.36 | 3.48 |
| 20 paragraphs | 2 | `fontdue` layout | 0.08 | 3.07 | 1.04 |
| | | cosmic-text | 1.18 | 7.09 | 2.31 |
| | | parley + swash | 0.62 | 5.95 | 1.85 |

Today's figures are from the native benchmark's `text` mode, in BENCHMARKS: `rasterize_text`
alone, the same work. The harness's whole run, `out/report.md`, has lines and every scale.

- **Shaping costs time:** laying out a label takes parley about 2.5 µs and cosmic-text 4 µs,
  against 0.2 µs for `fontdue`, which doesn't shape.
- **A glyph cache more than pays for it.** With one, both shaping engines bake labels faster
  than today does at a scale of 2, and as fast at 1. Without one, they're two to three times
  slower. Proteus needs a glyph cache with either.
- **parley is faster than cosmic-text** throughout: layout takes about 60% as long, and laying
  out and rasterizing together 80 to 90%.
- **In wasm, in Node,** everything takes 1.3 to 1.5 times as long, in the same order: 500 labels
  at a scale of 2 with a warm cache take 3.98 ms with parley and 4.58 with cosmic-text.
  `out/web/` runs the same in a browser.

Baking 500 labels at once is a whole screen of new text. The native benchmark's frame for it
takes 17.3 ms at a scale of 2 today, of which rasterizing is 5.6; the rest, adding each text to
the atlas and uploading it, doesn't change with the engine.

## Size

Each engine built alone for wasm, laying out and rasterizing one string, optimized as the SDK
is, less the same build with no engine:

| | Added, optimized | Added, gzipped | Crates |
|---|---:|---:|---:|
| `fontdue` layout | 171 KB | 70 KB | 8 |
| cosmic-text | 1,892 KB | 664 KB | 38 |
| parley + swash | 2,288 KB | 827 KB | 55 |

For scale, the SDK's wasm is 2,705 KB, 930 KB gzipped, with today's `fontdue` and Inter Bold in
it; its dependencies come to 136 crates.

- **It's code, not data:** 1.55 MB of cosmic-text's build and 1.92 MB of parley's is code.
- **Both build two versions of fontations' crates:** cosmic-text two of skrifa and read-fonts,
  as its harfrust is older than its own; parley two of each as well, and swash has a font parser
  of its own. Versions that line up, and for parley a rasterizer on skrifa's outlines instead of
  swash, would make both smaller. Worth measuring in step 2 with the chosen engine.
- **The fonts:** today's static Inter Bold is 343 KB, 166 KB gzipped, and moves out of the wasm
  with D7. Variable Inter is 877 KB, 458 KB gzipped, and Inter Italic 907 KB, 499 KB gzipped.

## Support

| | `fontdue` layout | cosmic-text | parley |
|---|---|---|---|
| Kerning and ligatures | No: no shaping, and only a `kern` table | Yes | Yes |
| Variable fonts | No: the default instance | The weight axis only | Any axis, such as Inter's optical size |
| Fallback | No: by hand | An app's list, and lists for each script | A font stack, as in CSS, and lists for each script |
| Mixed styles | Runs, each in one font and size | Spans with attributes | Ranged or nested spans |
| Alignment | Start, center, end | Left, right, center, end, justify | Start, end, left, right, center, justify |
| Ellipsis | No | "…" only, at the start, middle or end | No |
| Inline boxes | No | No | Yes |
| Italic without an italic font | No | A 14° slant, drawn by its rasterizer | Suggests a slant for the app to draw |
| Measuring without rasterizing | Yes | Yes | Yes |
| Line height | A multiple of the font's | Pixels only, set by the app | The font's, the size's, or pixels |

D5's ellipsis can be the app's own text or spans, so Proteus writes its own ellipsis whichever
engine it uses; cosmic-text's covers only the default.

## Upkeep

| | Releases | Minimum Rust | License |
|---|---|---|---|
| `fontdue` | 0.9.5, October 2026; 0.9.0 in 2024 | Not stated | MIT, Apache 2.0 or Zlib |
| cosmic-text | 0.19.0, April 2026; three breaking releases in 2026 | 1.89, Proteus's own | MIT or Apache 2.0 |
| parley | 0.13.0, November 2026; six breaking releases in 2026 | 1.88 | MIT or Apache 2.0 |

All three are licensed as Proteus is. parley moves fastest, with a breaking release every month
or two, so Proteus would pin one and upgrade on purpose, a release at a time.

## Weighing them

**`fontdue`'s layout** is the smallest and fastest, but falls short of v0.4.0's own scope: no
weights from a variable font, so a static file for every weight; no shaping, so no Hindi, and
no Inter kerning; and fallback by hand. Bidirectional text and text input, both candidates,
would need a shaper later anyway. It doesn't fit.

**cosmic-text** is the smaller and steadier of the two shaping engines: 664 KB gzipped, 38
crates, its minimum Rust is Proteus's own, and its releases have slowed. Against it: no inline
boxes; only the weight axis; a line height the app must give in pixels; its ellipsis can't be
the app's own; it's slower than parley at every step; and its `Buffer` is built for editors,
with cursors and scrolling, which Proteus doesn't need for each text.

**parley** has inline boxes, any variable axis, a font stack that is D8's fallback list as it
stands, and is the faster of the two, taking about 60% as long to lay text out. It's built on fontations throughout, apart
from the rasterizer, which is the app's to choose. Against it: it's the largest, 827 KB gzipped
and 55 crates; its API changes every month or two; and Proteus would write its own rasterizing
and ellipsis, both small.

## Recommendation

**parley, rasterized with swash at first,** with:

- **A glyph cache from step 2,** without which either shaping engine is slower than today.
- **Its size worked on in step 2:** the embedded font moving out of the wasm (D7) takes 166 KB
  gzipped back; a rasterizer on skrifa instead of swash, and matching fontations versions, may
  take more. The result goes in BENCHMARKS.
- **parley pinned to one minor version,** upgraded on purpose.
- **Inline boxes in v0.4.0,** as a step after inline styles: parley gives them for little more
  than drawing the component where the box is, and they make an image in an ellipsis possible.

**The embedded font (D7): variable Inter, roman only.** It gives every weight for 458 KB
gzipped, fetched by TypeScript apps only when they set no font. An app that wants italic loads
Inter Italic itself, or gets the slant parley suggests.

## Running it again

From `checks/text-engines`:

```bash
scripts/fetch-fonts.sh
cargo run --release > out/report.md
scripts/wasm.sh sizes
scripts/wasm.sh node
scripts/wasm.sh web
python3 -m http.server -d out/web 8000
```

The native benchmark's `text` mode, from the repository's root:

```bash
cargo run --release -p proteus-bench -- text
```
