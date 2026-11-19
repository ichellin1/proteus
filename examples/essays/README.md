# Essays Example

A reading app with four public-domain essays: Mary Wollstonecraft's "A Vindication of the
Rights of Woman" (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis Stevenson's "An
Apology for Idlers" (1877), from their Project Gutenberg editions, and Bharatendu
Harishchandra's "भारतवर्ष की उन्नति कैसे हो सकती है?" ("How can India progress?", 1884), in
Hindi, from Hindi Wikisource.

None of the essays' fonts has Hindi's script, Devanagari, so the app names one fallback font,
Noto Serif Devanagari, with `set_fallback_fonts`: a character a text's font lacks is looked for
there, then in the default font. Only that one is listed, so every character the app draws is
known to be covered.

Each essay has a card. Its year is aligned to the end of the card's column, its title wraps onto
at most two lines and ends in "…", its opening is spaced at a line height of 1.45 and stops
after five lines, ending in an ellipsis of its own, and its source is clipped after two lines. The
third card's ellipsis is longer than a line, so it takes a line of its own. A card opens into its
essay (1→1): a two-line byline split with `\n`, an opening that wraps at the column's width, a
centered pull quote, and the source in full. While an essay is open, a toolbar at the top sets
its body's size, alignment, bold and italic, line height and font, as a text editor's does. Its
icons are Material Icons, a font whose ligatures turn an icon's name into the icon, and a chosen
button stays in the accent color under the pointer, since its hover style is removed with
`set_style`.

The essays are set in Lora, a serif loaded from [`fonts/`](./fonts), with its italic for the
pull quotes. Titles and years are the built-in Inter at weights 800 and 500, bylines are Inter
slanted, since Inter's italic isn't loaded, and buttons and sources are Inter with no weight
set, bold. The TypeScript version downloads Lora and the icons, and their texts wait for them;
the Rust version builds them in. The toolbar's last button sets the body in Crimson Pro, which
both versions download only when it's first chosen: until it arrives, its button and the body
show the default font, then swap to it. Crimson Pro has no italic file, so its italic is
slanted.

Lora and Crimson Pro are from Google Fonts, under the SIL Open Font License, in
[`fonts/OFL-Lora.txt`](./fonts/OFL-Lora.txt) and
[`fonts/OFL-CrimsonPro.txt`](./fonts/OFL-CrimsonPro.txt). Material Icons is from Google, under
the Apache License 2.0, in [`fonts/LICENSE-MaterialIcons.txt`](./fonts/LICENSE-MaterialIcons.txt).
To watch a font arrive, throttle the network in the browser's developer tools.

Noto Serif Devanagari is from Google Fonts, under the SIL Open Font License, in
[`fonts/OFL-NotoSerifDevanagari.txt`](./fonts/OFL-NotoSerifDevanagari.txt).

Not shown yet: a weight a font doesn't have, a character no font has, drawn as a box, mixed
styles within a text, measuring text, and components within a line of text.

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
