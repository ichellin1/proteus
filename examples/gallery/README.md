# Gallery

A grid of photos downloaded from picsum.photos. Clicking a photo transforms its tile into a
large view of it (1→1, with a transition channel). Going back splits the large view into a new
grid of photos (1→N, with `split_to`).

Techniques worth copying are described at the top of the source,
[`typescript/src/main.ts`](./typescript/src/main.ts): loading a batch of images at once, giving
the large view its image before it opens, and crossfading from a small image to a large one.

To keep the example short, the layout is worked out once, when it starts, and doesn't follow the
window when it's resized.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### TypeScript

From the repository's root:

```bash
make example-gallery-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/gallery/typescript
npm install
npm run dev
```

`npm run build` type-checks the example and builds it into `dist/`, and `npm run preview` serves
that build.

## Troubleshooting

- **After a change to Proteus's Rust code, the example still runs the old code.** `make
  example-gallery-ts` rebuilds the SDK; restart it. If the change still doesn't appear, Vite is
  holding an old copy: delete `examples/gallery/typescript/node_modules/.vite` and restart.
- **`vite build` fails resolving the WebAssembly, or with an `@swc/core` error.**
  `vite-plugin-top-level-await` and `@swc/core` are pinned to versions that work; see the comment
  in [`vite.config.ts`](./typescript/vite.config.ts).
