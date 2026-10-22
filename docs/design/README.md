# Design notes

Reasoning behind planned work that is too long for [PLANNING.md](../../PLANNING.md), which
records only decisions and definitions of done. Each note gathers what is known about one
problem: what was found or measured, the approaches, and how to tell whether a change worked.
[ROADMAP.md](../../ROADMAP.md) links to the notes from its candidates.

These are working notes for contributors, not guides; for using Proteus, see the
[guides](../README.md).

| Note | About |
|---|---|
| [shader-fast-path](./shader-fast-path.md) | Less work per pixel in the shader, for plain components and covered pixels |
| [per-frame-work](./per-frame-work.md) | Collecting and uploading only the components that changed |
| [bake-resolution](./bake-resolution.md) | Baking text and components at the display's resolution |
| [shared-bake-corners](./shared-bake-corners.md) | Rounded corners on the slices of a baked split or merge |
| [context-loss](./context-loss.md) | Recovering a lost GPU context on the web |
| [3d](./3d.md) | 3D, and geometry other than rectangles |
| [texture-restoration](./texture-restoration.md) | Evicting, moving and releasing textures still in use |
| [video](./video.md) | Copying frames to the GPU, punch-through video, playback controls, several videos, and video in splits and merges |
| [many-transitions](./many-transitions.md) | Starting many transitions in one call from TypeScript |
| [composite-bake](./composite-bake.md) | Transitioning a flattened subtree as one quad |
