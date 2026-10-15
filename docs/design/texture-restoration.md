# Restoring evicted textures

**Status:** a V2 candidate.

The main atlas evicts only textures no component uses (see
[ARCHITECTURE.md](../../ARCHITECTURE.md#texture-lifetime)). Three related features need more:

- **Evicting textures still in use** when the atlas is under severe pressure.
- **Repacking a page** whose free space is scattered between regions still in use.
- **Releasing textures when the app is backgrounded,** on window occlusion natively or the Page
  Visibility API on the web. Only the video texture can be released today
  (`QuadPipeline::suspend_video` and `resume_video`), and no host calls them.

## Why they wait

`BakedText`, `BakedImage` and `BakedComposite` keep their region's texture coordinates and page
on the component. Freeing a region still in use, or moving its pixels, leaves the component
pointing at the old coordinates with no error, and once another texture reuses the region, the
component draws the wrong image.

So each needs a way to restore content: when a component's texture is evicted or moved, its owner
bakes it again (rasterizes the text, decodes the image bytes again) and the component gets the
new coordinates. That is the owning system's job, not the registry's: the registry can't
reproduce content from a `TextureId`. Repacking and releasing in the background are then
buildable on top of it.

Repacking a page whose content is entirely unused isn't needed: `etagere` already recovers all
of an empty page's space.

## Where the pixels come from

The same question as [recovering a lost GPU context](./context-loss.md): text and images on
components can be baked again from what their entities keep, while textures from
`bake_texture` and `load_texture`, and components made with `ComponentSpec::bake`, aren't kept.
Answer it once for both.
