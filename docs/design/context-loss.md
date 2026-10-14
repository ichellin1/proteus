# Recovering a lost GPU context on the web

**Status:** a V2 candidate. A known shortcoming, listed in
[ARCHITECTURE.md](../../ARCHITECTURE.md#known-limits).

A browser can take a page's GPU context away: when it reclaims a background tab's memory, most
often on phones, and after a GPU driver resets. The web host listens for `webglcontextlost` and
`webglcontextrestored`. It logs the loss and reconfigures the surface when the context is
restored, but rebuilds nothing, so the canvas stays blank until the page reloads.

Recovery would recreate the renderer and its pipeline, and bake every resident texture again.

## Keep the regions

The texture registry and its allocators are CPU memory and survive the loss. Upload each texture
into the region it had, and components' texture coordinates stay valid, with nothing in the
component model changed.

## Where the pixels come from

| Content | After a loss |
|---|---|
| Text | `Text` stays on its entity after baking, so it can be rasterized again |
| Images on components | `Image` keeps its file's bytes after baking, so it can be decoded again |
| Video | The player's next frame restores it |
| A split or merge in progress | Finish it at once, or bake both ends again |
| Textures from `bake_texture` and `load_texture` | **Not kept:** the pixels go straight to the GPU |
| Components made with `ComponentSpec::bake` | **Gone:** baking destroyed their children |

For the last two:

- **Keep a CPU copy** of those textures. Simple, but costs memory, up to the main atlas's size
  (64 MiB by default).
- **Ask the app to supply them again,** through an `on_context_restored` callback.
- **For a baked component, keep its data** rather than destroying its children, so it can be
  baked again.

A mix is likely: keep CPU copies of baked components, which are usually small, and ask the app
for its own textures.

## Check first

On WebGPU, a lost device is replaced by requesting a new one. On WebGL2, it isn't known whether
wgpu can set up again on the same canvas after the browser restores the context; if not, swap in
a new `<canvas>` element. A short experiment settles it before any design work.

## Related

[Restoring evicted textures](./texture-restoration.md) needs the same answer to where the pixels
come from.
