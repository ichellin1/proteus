# Essays Example

A reading app with three public-domain essays: Mary Wollstonecraft's "A Vindication of the
Rights of Woman" (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis Stevenson's "An
Apology for Idlers" (1877), from their Project Gutenberg editions.

Each essay has a card. Its year is aligned to the end of the card's column, its title wraps onto
at most two lines and ends in "…", its opening is spaced at a line height of 1.45 and stops
after five lines, ending in an ellipsis of its own, and its source is clipped after two lines. The
third card's ellipsis is longer than a line, so it takes a line of its own. A card opens into its
essay (1→1): a two-line byline split with `\n`, an opening that wraps at the column's width, a
centered pull quote, and the source in full. While an essay is open, the buttons at the top
change its alignment and line height.

Not shown yet: fonts and weights, other scripts, mixed styles within a text, measuring text,
and components within a line of text.

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
