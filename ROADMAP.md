# Proteus — Roadmap

Where Proteus is going. [PLANNING.md](./PLANNING.md) records the decisions and definitions of
done for the work underway, and the [design notes](./docs/design/README.md) hold the reasoning
behind planned work.

## Where things stand

**V1** is Proteus's first developer release, v0.1.0: the component model and its three
transition shapes, GPU rendering natively and on the web, Rust and TypeScript SDKs, a native and
a web host, guides, examples and benchmarks. [ARCHITECTURE.md](./ARCHITECTURE.md) describes how
it is built, and lists its known limits. It was built in milestones M0 to M14, whose roadmap
and planning are kept in [docs/archive](./docs/archive/) as history.

M14, the developer release, finishes with the README and publishing v0.1.0; see PLANNING.

## Next: V2 Planning

The first V2 milestone. It decides what V2 is: which candidates below it takes, in what order,
grouped into milestones, each with a definition of done in PLANNING. Until it runs, the
candidates are unordered.

## V2 candidates

Unordered within each area. **High priority** marks findings for V2 Planning to weigh first.

### Rendering and performance

- **High priority: a fast path for plain components in the shader.** Per-pixel work, not the
  number of components, limits large, overlapping scenes; it also makes frame times uneven. Also
  covers skipping pixels a later opaque component covers.
  [Design note](./docs/design/shader-fast-path.md)
- **High priority: rounded corners on the slices of a baked split or merge** alias toward
  square while a slice is drawn smaller than its bake.
  [Design note](./docs/design/shared-bake-corners.md)
- Only redo per-frame work that changed: collect, sort and upload only the components that
  changed. [Design note](./docs/design/per-frame-work.md)
- Bake text and components at the display's resolution, so they are as sharp as shapes the
  shader draws. [Design note](./docs/design/bake-resolution.md)
- A configurable render scale, so an app can trade sharpness on high-density displays for less
  fragment work.
- Split `quad.wgsl` into named parts, keeping its output identical, before custom shaders need
  reusable pieces.
- Custom shader authoring: formal support for developer-written WGSL effects.
- Blur, through an offscreen bake pass.
- Recovering a lost GPU context on the web. [Design note](./docs/design/context-loss.md)
- 3D, and geometry other than rectangles. [Design note](./docs/design/3d.md)

### Text

- Multiple fonts: a font per text, several weights of one family, and falling back to another
  font for characters the first doesn't have.
- Multi-line text and layout: line breaking, alignment, line height.
- Bidirectional text: left-to-right and right-to-left, with the Unicode bidi algorithm.
- Inline styles: bold, italic, size and color mixed within one text.
- Resolution-independent text, with signed distance fields, if rasterized text proves
  insufficient.

### Images, textures and memory

- Image fit: the crop follows the component's size as it changes, like CSS
  `object-fit: cover`, so a tile widening into a full view widens its crop with it.
- Content larger than an atlas page: let developers choose to scale it, clip it, or tile it
  across several regions so it shows in full at full resolution, including images over 2048
  pixels on the web.
- Masking: shape a component's content with a shape or another image's alpha, separately from
  cropping.
- Loading that manages itself: decode off the main thread, a per-frame budget for baking that
  carries the rest over, and what is visible first. Also, an image that doesn't fit a full atlas
  is decoded again, with a warning, every frame until space frees.
- Evicting, moving and releasing textures still in use, with a way to restore them.
  [Design note](./docs/design/texture-restoration.md)
- Animated images, such as GIFs and sprite sheets, with every frame packed into the atlas.

### Video

- A video handle with playback controls (play, pause, seek, loop, volume, an "ended" event),
  designed around bringing your own player.
  [Design note](./docs/design/video.md#a-video-handle-with-playback-controls)
- Several videos at once, each with its own texture. A new video replaces the old one today.
  [Design note](./docs/design/video.md#several-videos-at-once)
- Copying frames from a `<video>` element straight to the GPU, instead of reading them back
  through a canvas.
  [Design note](./docs/design/video.md#copying-frames-straight-to-the-gpu-on-the-web)
- Video that keeps playing through splits and merges, which today show a frozen frame.
  [Design note](./docs/design/video.md#live-video-during-splits-and-merges)

### Transitions and components

- Start many transitions in one call, from TypeScript.
  [Design note](./docs/design/many-transitions.md)
- A composite-bake transition strategy: flatten a subtree into one quad for the length of a
  transition. [Design note](./docs/design/composite-bake.md)
- Which wins when a parent and its child are both given transitions at once.
- Opacity in transitions, so a component can fade as it moves. Today opacity changes at once,
  and isn't part of the geometry a transition interpolates.
- Advanced transition effects, such as particle dissolution and fluid deformation.
- A layout system (`VStack`, `HStack`, `Grid`), with positions relative to the parent, and items
  that glide to their new places when the layout changes.
- Where (0, 0) is: the top-left with y down, as many web developers expect, or positions as they
  are, with layouts placing components.
- Borders that sit inside, centered on, or outside a component's geometry, and a border offset
  that moves the border a set distance in from or out past the edge, like CSS's
  `outline-offset`. Only inner borders draw today. The instance already carries a placement, in
  a field confusingly named `border_offset`, but nothing outside the shape is drawn, so a border
  outside it needs the quad grown to make room, as it already is for a shadow.
- An `on_destroy` callback, for components and channels destroyed as a side effect, with their
  parent or their owner.
- Signals: reactive state that notifies its observers when its value changes.
- A component library: scrolling lists, grids, forms and other common patterns.
- A style system: styles set declaratively, outside the app's code, such as in a style file, and
  applied to components by name. Colors, corner radii, borders, shadows, text and interaction
  styles could then change, or follow a theme such as light and dark, without changing code.

### Input

- Keyboard input: both hosts read key events and report them to the engine, and apps can
  respond to keys.
- Focus navigation: the schedule's navigation stage, a placeholder today, moves focus between
  components with the arrow keys and Tab, and the focused style follows it.
- Every new kind of input skips non-interactive and disabled components, as the pointer does;
  see the decision in PLANNING.
- Game controllers: buttons and sticks, read through the browser's Gamepad API on the web and
  the platform's controller support natively, driving focus navigation and reaching apps.
- TV remotes: the D-pad, OK and Back buttons. On web-based TVs they arrive as key events, so they
  build on keyboard input and focus navigation; native TV platforms need their hosts to report
  them.

### Platforms and hosts

- Native mobile hosts: iOS and Android on a native surface.
- Mobile packaging for TypeScript apps, through a Capacitor webview.
- Embedded Linux without a compositor (`proteus-host-drm`), and a demo on constrained hardware
  such as a Raspberry Pi.
- Native smart-TV: Tizen, webOS and Android TV.
- `proteus-host-jsengine`: TypeScript apps on a native GPU, without a browser.
- An XR host, with WebXR and OpenXR. Depends on 3D.
- TypeScript apps' declarative parts running natively, with no JavaScript runtime.
- Other language bindings, such as Python, Swift and Kotlin.
- Real runs of the native host on Linux and Windows, which CI tests but nobody has run by hand.

### Tests, tooling and the repository

- Tests that run in a browser, for the web host itself.
- Benchmarks that run regularly and catch regressions.
- The reference demo uses interaction styles, rather than its own hover code.
- The test that checks the shader's atlas-page layout against Rust's breaks when the shader is
  reformatted.
- Repository size: the history still carries assets removed from the tree.
- Dogfooding: a personal website built with Proteus.
