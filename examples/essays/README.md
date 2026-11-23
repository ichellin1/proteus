# Essays Example

A reading app with four public-domain essays. Three are from their Project Gutenberg editions:
Mary Wollstonecraft's "A Vindication of the Rights of Woman" (1792), Henry David Thoreau's
"Walking" (1862) and Robert Louis Stevenson's "An Apology for Idlers" (1877). The fourth is from
Hindi Wikisource, in Hindi: Bharatendu Harishchandra's "भारतवर्ष की उन्नति कैसे हो सकती है?"
("How can India progress?", 1884).

None of the essays' fonts has Hindi's script, Devanagari, so the app names one fallback font,
Noto Serif Devanagari, with `set_fallback_fonts`. A character a text's font lacks is looked for
there, then in the default font. Only that one font is listed, so every character the app
draws is known to be covered.

Each essay has a card. The year is aligned to the end of the card's column. The title wraps onto
at most two lines and ends in "…". The opening is spaced at a line height of 1.45 and stops
after five lines, ending in an ellipsis of its own. The source is clipped after two lines, and
the titles in it are italic. The second card's ellipsis is bold, in the accent color, with an
arrow from Material Icons after it. The third card's ellipsis is longer than a line, so it takes
a line of its own.

A card opens into its essay (1→1). The essay has a two-line byline split with `\n`, with the
author in a heavier weight and the year smaller and muted. It has an opening that wraps at the
column's width, a centered pull quote ending in a superscript dagger, and the source in full,
starting with the same dagger. While an essay is open, a toolbar at the top sets its body's
size, alignment, bold and italic, line height and font, as a text editor's does. The toolbar's
icons are Material Icons, a font whose ligatures turn an icon's name into the icon. A chosen
button stays in the accent color under the pointer, since `set_style` removes its hover style.

The essays are set in Lora, a serif loaded from [`fonts/`](./fonts), with its italic for the
pull quotes. Titles are the built-in Inter at weight 800, and years are at weight 500. Bylines
are Inter slanted, since Inter's italic isn't loaded. Buttons and sources are Inter with no
weight set, so they're bold.

The TypeScript version downloads Lora and the icons, and texts in them wait for the fonts. The
Rust version builds them in. The toolbar's last button sets the body in Crimson Pro. Both
versions download it only when it's first chosen. Until it arrives, its button and the body show
the default font, then swap to it. Crimson Pro has no italic file, so its italic is slanted.

Lora and Crimson Pro are from Google Fonts, under the SIL Open Font License. The license texts
are in [`fonts/OFL-Lora.txt`](./fonts/OFL-Lora.txt) and
[`fonts/OFL-CrimsonPro.txt`](./fonts/OFL-CrimsonPro.txt). Material Icons is from Google, under
the Apache License 2.0. Its license text is in
[`fonts/LICENSE-MaterialIcons.txt`](./fonts/LICENSE-MaterialIcons.txt). To watch a font arrive,
throttle the network in the browser's developer tools.

Noto Serif Devanagari is from Google Fonts, under the SIL Open Font License. The license text is
in [`fonts/OFL-NotoSerifDevanagari.txt`](./fonts/OFL-NotoSerifDevanagari.txt).

Not shown yet: a weight a font doesn't have, a character no font has (which is drawn as a box),
measuring text, and components within a line of text.

Techniques worth copying are described at the top of each version's source:
[`rust/src/lib.rs`](./rust/src/lib.rs) and [`typescript/src/main.ts`](./typescript/src/main.ts).
They include placing text by its component's anchor, and hiding a card's texts while it changes
shape.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### Rust

In a desktop window, from the repository's root:

```bash
cargo run -p essays
```

In a browser:

```bash
make example-essays-web
```

Then open <http://localhost:8080/examples/essays/rust/>. Without make, build it for the web,
then serve the repository from its root:

```bash
wasm-pack build examples/essays/rust --target web --release
python3 -m http.server 8080
```

### TypeScript

From the repository's root:

```bash
make example-essays-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/essays/typescript
npm install
npm run dev
```
