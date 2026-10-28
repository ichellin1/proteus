# Bake at the display's resolution

**Status:** done in v0.2.0, capped at a scale of 2 by `ResourceConfig::max_bake_scale`; see
step 7 of [v0.2.0's planning](../archive/PLANNING-v0.2.0.md), ARCHITECTURE's
[Baking](../../ARCHITECTURE.md#baking), and the
[configuration guide](../guides/configuration.md#sharpness-on-high-density-displays). A baked
component isn't baked again when the scale changes.

Nothing that draws into a texture reads the display's scale factor. Text is rasterized at
`size_px` logical pixels, and baked components (`ComponentSpec::bake`) and the images a split or
merge bakes get atlas regions of their logical size. On a 2× display each texel covers 2 × 2
screen pixels, so text and baked content are slightly soft next to shapes the shader draws
directly. Confirmed on a Retina display: subtle, but text is visibly softer than macOS's own.

## Approach

- Rasterize text at `size_px × scale_factor`, and size component and transition bakes the same
  way; keep drawing them at their logical size, so layout doesn't change.
- When the scale factor changes, as when a window moves to another display, bake text and
  components again.
- It costs up to 4× the atlas space for that content on a 2× display, which matters most on the
  web's 2048-pixel pages. Decide whether a setting caps the bake scale for apps short of memory.

## Test

On a scale-2 viewport, a text bake's pixel size is twice its logical size, and it is drawn at
its logical size.

## Related

- [A configurable render scale](../../ROADMAP.md#rendering-and-performance), which trades the
  surface's own resolution for fragment cost.
- Resolution-independent (SDF) text, which would make text's resolution a non-question.
