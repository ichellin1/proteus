# Menu

A theme picker. Clicking the button splits it into a column of menu items (1→N, with
`splitTo`). Picking a theme merges the items back into the button (N→1, with `mergeFrom`), which
now names the new theme, and the preview card above fades to its color. Clicking anywhere else
while the menu is open merges it back unchanged.

Nothing is opened or closed: the button becomes the menu, and the menu becomes the button. The
items grow on hover and shrink while pressed, and one, Gold, is disabled.

Techniques worth copying are described at the top of each version's source:
[`rust/src/lib.rs`](./rust/src/lib.rs) and [`typescript/src/main.ts`](./typescript/src/main.ts).
They include reusing hidden menu items, changing the button while it's hidden, and tracking
whether the menu is opening, open, closing or closed.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### Rust

In a desktop window, from the repository's root:

```bash
cargo run -p menu
```

In a browser:

```bash
make example-menu-web
```

Then open <http://localhost:8080/examples/menu/rust/>. Without make, build it for the web, then
serve the repository from its root:

```bash
wasm-pack build examples/menu/rust --target web --release
python3 -m http.server 8080
```

### TypeScript

From the repository's root:

```bash
make example-menu-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/menu/typescript
npm install
npm run dev
```
