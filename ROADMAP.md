# Proteus — Roadmap

Where Proteus is going, release by release. [PLANNING.md](./PLANNING.md) records the decisions
and definitions of done for the work underway, and the [design notes](./docs/design/README.md)
hold the reasoning behind planned work.

## How releases are planned

- **Features ship by version.** Each release, such as v0.2.0 or v0.3.0, has a theme and a fixed
  scope, and ships when that scope is done, not on a date.
- **Small fixes ride along** in the next feature release. One found after a release ships can
  go out in a patch release, such as v0.2.1.
- **Candidates are weighed in this order:** fixing known limits and known issues; what outside
  developers need to build real apps; effects, such as blur; then other platforms.
- **Work that isn't part of any release,** such as upkeep of the repository, is listed under
  [Independent of a release](#independent-of-a-release), and done when it's ready.

## Where things stand

v0.1.0, Proteus's first developer release, shipped on 2026-10-15: the component model and its
three transition shapes, GPU rendering natively and on the web, Rust and TypeScript SDKs, a
native and a web host, guides, examples and benchmarks. [ARCHITECTURE.md](./ARCHITECTURE.md)
describes how it is built, and lists its known limits. It was built in milestones M0 to M14,
whose roadmap and planning are kept in [docs/archive](./docs/archive/) as history. "V1" in those
documents means v0.1.0, and "V2" means everything after it.

## Releases

A draft order, from release planning. Each release is planned in detail, with a definition of
done in PLANNING, when it comes up, and its scope can change then.

### v0.2.0 — Rendering limits

Sharper, faster and steadier rendering: the known limits in how Proteus draws. In the order
they're built; PLANNING has the steps.

- The browser benchmark published beside the demo, and in each pull request's staging preview,
  so it can be run on other people's machines.
- Skip drawing a frame when nothing changed: no running transition, input, change to a
  component, pending bake or new video frame. Both hosts draw every frame today, even on a still
  screen, which may be why fans turn on with Proteus on low-end devices. A coarse check comes
  first, and per-frame work's tracking refines it.
- Only redo per-frame work that changed: collect, sort and upload only the components that
  changed. [Design note](./docs/design/per-frame-work.md)
- Split `quad.wgsl` into named parts, keeping its output identical, before custom shaders need
  reusable pieces. Done before the fast path, which changes the same shader.
- The test that checks the shader's atlas-page layout against Rust's breaks when the shader is
  reformatted.
- **High priority: a fast path for plain components in the shader.** Per-pixel work, not the
  number of components, limits large, overlapping scenes; it also makes frame times uneven. Also
  covers skipping pixels a later opaque component covers.
  [Design note](./docs/design/shader-fast-path.md)
- Bake text and components at the display's resolution, so they are as sharp as shapes the
  shader draws. [Design note](./docs/design/bake-resolution.md)
- **High priority: rounded corners on the slices of a baked split or merge** alias toward
  square while a slice is drawn smaller than its bake.
  [Design note](./docs/design/shared-bake-corners.md)
- Top-left coordinates in Rust: Rust equivalents of TypeScript's `topLeftToWorld` and
  `worldToTopLeft`, and guides that make the origin, the viewport's center with y up, and the
  conversion clear in both languages. The origin itself stays where it is. Independent of the
  rest, so done at any point.

### v0.3.0 — Keyboard and focus

- Keyboard input: both hosts read key events and report them to the engine, and apps can
  respond to keys.
- Focus navigation: the schedule's navigation stage, a placeholder today, moves focus between
  components with the arrow keys and Tab, and the focused style follows it.
- Every new kind of input skips non-interactive and disabled components, as the pointer does;
  see the decision in PLANNING.
- The reference demo uses interaction styles, rather than its own hover code.

### v0.4.0 — Text

- Multi-line text and layout: line breaking, alignment, line height.
- Multiple fonts: a font per text, several weights of one family, and falling back to another
  font for characters the first doesn't have.
- Inline styles: bold, italic, size and color mixed within one text.

### v0.5.0 — Video

- Copying frames from a `<video>` element straight to the GPU, instead of reading them back
  through a canvas.
  [Design note](./docs/design/video.md#copying-frames-straight-to-the-gpu-on-the-web)
- Punch-through video, as an alternative to drawing video on a component: the platform shows
  the video itself, in a region the app leaves clear, with no frames copied at all. For full
  screen video on low-end devices, and for protected (DRM) video, which can't be drawn into a
  texture. It gives up most transitions and effects on the video. The web first, with a
  `<video>` element; native hosts after.
  [Design note](./docs/design/video.md#punch-through-video)
- A video handle with playback controls (play, pause, seek, loop, volume, an "ended" event),
  designed around bringing your own player.
  [Design note](./docs/design/video.md#a-video-handle-with-playback-controls)
- Several videos at once, each with its own texture. A new video replaces the old one today.
  [Design note](./docs/design/video.md#several-videos-at-once)
- Video that keeps playing through splits and merges, which today show a frozen frame.
  [Design note](./docs/design/video.md#live-video-during-splits-and-merges)

### v0.6.0 — Building apps

What outside developers need to build real apps.

- A layout system (`VStack`, `HStack`, `Grid`), with positions relative to the parent, and items
  that glide to their new places when the layout changes.
- Opacity in transitions, so a component can fade as it moves. Today opacity changes at once,
  and isn't part of the geometry a transition interpolates.
- Which wins when a parent and its child are both given transitions at once.
- Start many transitions in one call, from TypeScript.
  [Design note](./docs/design/many-transitions.md)
- Image fit: the crop follows the component's size as it changes, like CSS
  `object-fit: cover`, so a tile widening into a full view widens its crop with it.
- Borders that sit inside, centered on, or outside a component's geometry, and a border offset
  that moves the border a set distance in from or out past the edge, like CSS's
  `outline-offset`. Only inner borders draw today. The instance already carries a placement, in
  a field confusingly named `border_offset`, but nothing outside the shape is drawn, so a border
  outside it needs the quad grown to make room, as it already is for a shadow.
- An `on_destroy` callback, for components and channels destroyed as a side effect, with their
  parent or their owner.

### v0.7.0 — Effects

- Blur, through an offscreen bake pass.
- Custom shader authoring: formal support for developer-written WGSL effects.
- Masking: shape a component's content with a shape or another image's alpha, separately from
  cropping.
- A configurable render scale, so an app can trade sharpness on high-density displays for less
  fragment work.

### v0.8.0 — Platforms

- Mobile packaging for TypeScript apps, through a Capacitor webview.
- TV remotes: the D-pad, OK and Back buttons. On web-based TVs they arrive as key events, so they
  build on keyboard input and focus navigation; native TV platforms need their hosts to report
  them.
- Game controllers: buttons and sticks, read through the browser's Gamepad API on the web and
  the platform's controller support natively, driving focus navigation and reaching apps.
- Native mobile hosts: iOS and Android on a native surface.
- Embedded Linux without a compositor (`proteus-host-drm`), and a demo on constrained hardware
  such as a Raspberry Pi.
- Recovering a lost GPU context on the web. [Design note](./docs/design/context-loss.md)


## Independent of a release

Tied to no version; each is done when it's ready.

- **The README on GitHub, npm and crates.io:** check that it renders correctly on all three,
  and that its images load off GitHub. Left over from v0.1.0.

## Candidates

Not yet placed in a release. Unordered within each area.

### Rendering and performance

- Let the frame loop sleep while nothing changes, waking for input, a resize or the app asking
  for a frame. Needs a new API for apps that animate in `update`, and gains little over
  skipping drawing, so it's tried as a proof of concept after the planned releases.
- 3D, and geometry other than rectangles. [Design note](./docs/design/3d.md)

### Text

- Bidirectional text: left-to-right and right-to-left, with the Unicode bidi algorithm.
- Resolution-independent text, with signed distance fields, if rasterized text proves
  insufficient.

### Images, textures and memory

- Content larger than an atlas page: let developers choose to scale it, clip it, or tile it
  across several regions so it shows in full at full resolution, including images over 2048
  pixels on the web.
- Loading that manages itself: decode off the main thread, a per-frame budget for baking that
  carries the rest over, and what is visible first. Also, an image that doesn't fit a full atlas
  is decoded again, with a warning, every frame until space frees.
- Evicting, moving and releasing textures still in use, with a way to restore them.
  [Design note](./docs/design/texture-restoration.md)
- Animated images, such as GIFs and sprite sheets, with every frame packed into the atlas.
- Mipmaps for the atlases, so text, images and bakes drawn smaller than their texture don't
  shimmer or alias. Costs a third more atlas memory, a pass after each bake, and padding so
  atlas regions don't bleed into each other at the smaller levels.

### Transitions and components

- A composite-bake transition strategy: flatten a subtree into one quad for the length of a
  transition. [Design note](./docs/design/composite-bake.md)
- Advanced transition effects, such as particle dissolution and fluid deformation.
- Signals: reactive state that notifies its observers when its value changes.
- A component library: scrolling lists, grids, forms and other common patterns.
- A style system: styles set declaratively, outside the app's code, such as in a style file, and
  applied to components by name. Colors, corner radii, borders, shadows, text and interaction
  styles could then change, or follow a theme such as light and dark, without changing code.

### Platforms and hosts

- Native smart-TV: Tizen, webOS and Android TV.
- `proteus-host-jsengine`: TypeScript apps on a native GPU, without a browser.
- An XR host, with WebXR and OpenXR. Depends on 3D.
- TypeScript apps' declarative parts running natively, with no JavaScript runtime.
- Other language bindings, such as Python, Swift and Kotlin.

### Tests and tooling

- Benchmarks that run in CI and catch regressions. Shared runners' timings are too noisy to
  compare with stored results, so each change would be compared against its base branch on the
  same runner.
- Tests that run in a browser, for the web host itself.
- Real runs of the native host on Linux and Windows, which CI tests but nobody has run by hand.

