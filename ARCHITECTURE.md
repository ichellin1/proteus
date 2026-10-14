# Architecture

How Proteus is built: its crates, what happens in a frame, how components are drawn and
transitioned, and the main design decisions behind it. It is for anyone changing Proteus, or
wanting to know how it works before using it. To use Proteus, start with the
[guides](./docs/README.md); for the ideas behind it, see [VISION.md](./VISION.md).

## Crates and layers

Each crate depends only on the ones below it:

```text
proteus-host-winit    proteus-host-web          the hosts: window or canvas, loop, input
        │                    │      │
        │                    │      └── proteus-sdk-web   the TypeScript SDK's bridge
        ▼                    ▼                │
proteus-runtime ◄────────────┘                │           App, Engine, Renderer, HostServices
        │                                     │
        ├── proteus-gpu                       │           device, queue and surface setup
        ▼                                     ▼
proteus-sdk ◄─────────────────────────────────┘           the API apps are written against
        │
        ▼
proteus-ui                                                components, transitions, input
        │
        ▼
proteus-render                                            the pipeline, the shader, the atlases
```

| Crate | What it holds |
|---|---|
| `proteus-render` | The instanced pipeline, its shader `quad.wgsl`, the texture atlases and their registry, text rasterizing and image decoding. Knows nothing about components. |
| `proteus-ui` | The component model on `bevy_ecs`: geometry, the hierarchy, transitions in all three shapes, input and hit testing, interaction styles, component baking, and turning the world into instances to draw. |
| `proteus-sdk` | The API an app uses: `Proteus`, which holds the app's components, and the handles it returns. It draws nothing itself. |
| `proteus-gpu` | Creates a wgpu device, queue and configured surface from a platform's surface, the same way on every platform. |
| `proteus-runtime` | The contract between an app and a host: `App`, `Frame`, `HostServices` and `ProteusConfig`, and the `Engine` and `Renderer` that run a frame. |
| `proteus-host-winit` | The native host: a winit window, its event loop, pointer input, and assets read from a directory. |
| `proteus-host-web` | The web host: a canvas, the `requestAnimationFrame` loop, Pointer Events, and fetched assets. Runs a Rust `App`, or a TypeScript app through `mount`. |
| `proteus-sdk-web` | The wasm bridge from TypeScript to `proteus-sdk`, and the TypeScript SDK itself in `ts/`, published to npm. |

These are published. The rest of the workspace isn't: `proteus-demo`, the reference demo,
written once as an `App`; `proteus-shell-native` and `proteus-shell-web`, which hand the demo
to a host; the examples under `examples/`; the benchmarks under `benches/`; and
`proteus-docs`, which tests the guides' code and links.

## Components

"Component" has two meanings here. On its own, it means a UI element, as in the public API's
`component()`. An **ECS component** is `bevy_ecs`'s meaning: a piece of data attached to an
entity, which systems query. Each UI component is an entity carrying ECS components. The public
API keeps the ECS out of sight, apart from `Proteus::world_mut` for code that needs it, so app
code normally only meets the first meaning.

A component is an entity in a `bevy_ecs` world, owned by `Proteus`. Its geometry is a
`QuadState`: position, size, rotation, scale, anchor, color and corner radius. Everything
else it can have is a separate ECS component on the same entity: `Text`, `Image`, `Border`,
`Glow`, `DropShadow`, `Opacity`, `Visibility`, interaction styles, and the state of a
transition in progress.

A component has two geometries. Its **declared geometry** is where it rests, set when it is
created and changed by `set_declared_geometry` or `animate_to`. Its `QuadState` is where it is
now, which differs from the declared geometry during a transition or an interaction style.
Transitions into a component end at its declared geometry.

**Coordinates.** Proteus is 2D. World units are logical pixels, with the origin at the center of
the viewport and y up. A position's `z` only decides which component is drawn on top; there is
no perspective, and `rotation` turns a component in the screen's plane. A top-level component's
position is in world units; a child's is relative to its parent. A child's world geometry is
worked out when something needs it, by drawing or baking, by combining position, rotation and
scale down the chain of parents, so it is never a tick out of date.

**The hierarchy** is `bevy_ecs`'s own `ChildOf` and `Children`, so destroying a component
destroys its children. A hidden component hides its descendants, and opacity multiplies down
the hierarchy. These two are the only values stored per entity after combining with the
parents': `visibility_system` and `opacity_system` write `EffectiveVisibility` and
`EffectiveOpacity`, and only when they change.

**Drawing order.** Top-level components are drawn by `z`, lowest first, then in the order they
were created. Each is followed by its children, ordered among themselves the same way, so a
child is always over its parent and never over a different top-level component drawn later.
Hit testing follows the same order, so a click goes to the component on top.

**Handles.** An app holds `Handle`s, `TransitionChannel`s and `TextureHandle`s: small `Copy`
IDs. Their methods take the `Proteus` they came from as an argument, as in
`button.animate_to(&mut app, to, config)`. Proteus has no global state: `Frame` passes
`&mut Proteus` to the app each frame, and the app's own data is kept separately.

## A frame

A host calls `Engine::frame` once per frame:

```text
Proteus::tick(dt)          run the schedule below, then the app's callbacks
App::update(frame, dt)     the app reacts to this frame's events
Proteus::refresh_cascades  recompute visibility and opacity after update's changes
Renderer::render(target)   bake pending text and images, collect instances, draw
```

`dt` is clamped first, so a frame after a long pause doesn't jump. A TypeScript app runs the
same sequence, in `proteus-host-web`'s `mount`, with its JavaScript `update` in place of
`App::update`.

**The tick** runs a `bevy_ecs` schedule of stages, each finishing before the next starts:

```text
flush_commands       apply changes queued during the last tick
input                hit-test the pointer and record this tick's input events
interaction_style    start style transitions for hover, press, focus and disabled
navigation           keyboard focus movement (a placeholder; does nothing yet)
channel_dispatch     turn channel requests into transition requests
transition_setup     start requested transitions, splits and merges
transition_tick      advance transitions and interpolate geometry
transition_complete  finish transitions that reached the end
group_complete       finish splits and merges whose pieces have all arrived
visibility, opacity  combine each entity's values with its parents'
bake                 bake components marked for baking
```

Then `Proteus::tick` calls the app's callbacks for this tick's events: clicks, hovers,
presses, releases, focus, drags, completed transitions and dropped requests. Callbacks run in
the order they were registered.

**When changes take effect.** A transition requested during a tick or in `update`, by
`animate_to`, a channel or a split, starts on the next tick: requests are recorded and picked
up by the setup stages. Visibility and opacity changes made in `update` are drawn the same
frame, which is why the engine recomputes them after `update`.

## Rendering

Each frame the renderer collects every visible component's data into a list of
`QuadInstance`s, copies the list to the GPU in one upload, and draws them all with **one
instanced draw call**: a unit quad, drawn once per instance. However many components there
are, the CPU issues one upload and one draw.

### One draw call, one shader

One draw call means one pipeline, so one shader, `quad.wgsl`, draws every component and every
feature. A component's features are switched on or off by its instance data, not by choosing
a different shader:

- **The shape** is a rounded rectangle drawn with a signed distance field, antialiased over
  one pixel.
- **Content** is a texture sample. A component with no image or text samples a white block in
  the main atlas, so its color alone decides how it looks, with no branch in the shader.
- **A border** draws when its width isn't zero.
- **A drop shadow or glow** draws when its color isn't transparent; the vertex shader grows
  the quad to make room for it only then. A glow is a shadow with no offset.
- **A crossfade** between two textures, used by splits, merges and video, blends two samples
  by `crossfade_t`.
- **Text** is a second instance drawn over the component's background, with the text's
  baked image as its texture.

Drawing order is the order of the instances: `collect_instances` sorts them, so later ones
are on top, and no depth buffer is used. At most `ProteusConfig.memory.max_instances` are
drawn; beyond that, the topmost are dropped with a warning.

**What this costs.** The CPU's work per frame grows with the number of components only in
collecting and uploading their data, which is what makes Proteus fast with many components;
[BENCHMARKS.md](./BENCHMARKS.md) measures it against one draw call per component. The cost is
on the GPU: every pixel of every component runs the whole shader, including the texture
sample and the checks for a border and a crossfade, even for a plain shape. With large
components overlapping many times, this per-pixel work, not the CPU, is what limits the frame
rate.

### The instance and its limits

A `QuadInstance` is 160 bytes, so 10,000 components upload about 1.6 MB a frame. Its fields are
packed into **all 16 vertex attribute slots**, the most wgpu's default limits allow, natively
and in WebGL2: two for the unit quad and fourteen for the instance. New per-component data has
to be packed into existing fields, as `atlas_page` already packs the atlas selector and the page
number into one integer. The layout is listed in `QuadInstance::buffer_layout`, and `quad.wgsl`
must match it.

A second pipeline, with a different shader for some components, is possible, but it costs a
draw call for each run of consecutive components that use it, since drawing order must hold
across the two. It suits a feature drawn on its own layer better than one mixed through the
scene. Such components must still be bakeable, so the second pipeline would also need a
version that draws into the atlases, as `atlas_pipeline` does for the main shader; see
[Baking](#baking).

### Atlases

Every texture a component shows is a region of one of three textures, bound once for the
whole draw:

| Atlas | Holds | Size |
|---|---|---|
| Main atlas | Baked text, images, baked components, and the white block | An array texture: 4 pages of 2048 × 2048 by default, 64 MiB, allocated up front |
| Transition atlas | Images of the components at both ends of a split or merge in progress | One 2048 × 2048 texture by default |
| Video | The current video frame | The video's size |

An instance's `atlas_page` says which atlas and, for the main atlas, which page it samples.
Pages are fixed in size and number when the renderer starts, since an array texture can't grow
without being recreated, and no texture can be larger than a page. WebGL2 limits a page to
2048 pixels, and the web host holds every config to WebGL2's limits, even in a browser with
WebGPU, so an app behaves the same in every browser. All three atlases hold premultiplied
alpha, which keeps bilinear filtering right at antialiased edges.

### Baking

To bake is to render something into an atlas region, once, so it can be drawn as a texture.
Proteus bakes three kinds of thing:

- **Text and images**, by `Renderer::render` before collecting instances. Text is rasterized
  on the CPU with `fontdue`, one line to one image, white with the glyphs' coverage in alpha,
  so it can take any color. Images are decoded with the `image` crate. Both go into the main
  atlas. With `resources.lazy_load`, a component's text or image waits until it is visible.
- **A component marked with `ComponentSpec::bake`**, by `bake_system` during the tick. It and
  its descendants are drawn into a main-atlas region; then the children are destroyed and the
  component becomes a plain quad showing the image. It waits until every text and image in it
  has been baked.
- **The two ends of a split or merge**, when it starts, into the transition atlas; see
  [Splits and merges](#splits-and-merges).

A bake draws with the same shader as the frame, through a second pipeline, `atlas_pipeline`,
because a wgpu pipeline is tied to one target format: the atlases are `Rgba8Unorm`, while the
surface's format depends on the platform. A bake draws into a scratch texture, with its own
uniform and instance buffers so it never disturbs the frame's, and copies the result into the
atlas.

Bakes are at the component's size in logical pixels, not the display's, so on a high-density
display text and baked content are slightly softer than shapes the shader draws directly. This
is a known shortcoming, planned to be fixed in a future release.

## Transitions

A transition moves a component's `QuadState` from one geometry to another over time. Every
transition, whatever started it, is the same ECS component, `ActiveTransition`, advanced the
same way:

```text
TransitionRequest     added by animate_to, a channel, or a split or merge
        │  transition_setup
        ▼
ActiveTransition      start and end geometry, config, progress
        │  transition_tick, every tick: once any delay has passed,
        │  progress = elapsed / duration, and QuadState = lerp(start, end, easing(progress))
        ▼
transition_complete   progress reaches 1: back to Idle, recorded in CompletedTransitions
```

The easing may overshoot past 0 or 1, and the geometry follows it, but sizes and corner radii
never go below zero and colors stay within 0 to 1. Rotation is interpolated directly, not the
shortest way round. A transition starts from wherever the component is, even partway through
another, unless it is given a start, as a channel's is.

**Interaction styles** (hover, pressed, focused and disabled) are transitions too, to the
declared geometry with the style's changes applied, but they don't count as transitions of the
component: it stays idle, keeps taking input, and their completion isn't reported. While a
component is transitioning for another reason, its style waits until it is idle.

### 1→1: channels

A `TransitionChannel` transitions one component into another: the `to` component starts at the
`from` component's geometry and ends at its own declared geometry, and the `from` component is
hidden. `TransitionChannel::set` only records the request; `channel_dispatch` turns it into a
`TransitionRequest` on the next tick, after checking it can run. A request that can't, because
the channel or a component is gone, `from` is hidden, or `to` is already transitioning and the
request isn't interruptible, is reported to the channel's `on_dropped` handlers rather than
ignored. A channel with an owner is destroyed with its owner.

### Splits and merges

A **split** (1→N) turns one component into several; a **merge** (N→1) turns several into one.
Both work with **virtual** components: temporary stand-ins that do the moving while the real
components are hidden.

```text
split_to(targets, strategy)
        │  one_to_n_setup
        ▼
the source and each target are baked into the transition atlas
the source and the targets are hidden
one virtual per target, from its slice of the source to the target's geometry
        │  each virtual: an ordinary ActiveTransition, crossfading
        │  from its slice of the source's image to its target's image
        ▼
group_complete        every virtual has arrived: show the targets, remove the virtuals
```

The strategy decides how the source is sliced: into a row, a column or a grid, with slice `i`
going to target `i`. A merge is the reverse, with the destination baked once and divided by
its layout. Because each virtual fades between two images, the pieces look like parts of the
source at the start and like the targets at the end, whatever those contain: text, images,
borders, children. If there's no GPU, as in tests, or a bake fails, a virtual is a plain
colored shape carrying its source's shadow, glow and text instead.

A virtual has the `Virtual` marker, and every behavioral system, such as input and
transition completion, skips entities that have it. Transition-atlas regions are freed by
`bevy_ecs` hooks when the ECS components that hold them are removed, so they are returned
however the split or merge ends, even if a component is destroyed partway.

`SplitStrategy::PerTarget` is the exception: each target transitions from the source's
geometry to its own as an ordinary 1→1 transition, showing its own content throughout, with no
virtuals and no bakes. It is experimental.

## Input

The host converts its platform's pointer events into world units and reports them to the
engine: moved (or left), pressed, and released. In the next tick, `hit_test_system` finds the
component under the pointer and records this tick's events in `InteractionEvents`; the
interaction styles and the app's callbacks read them from there.

A component is hit-tested against the area it is drawn in, with its parents' position,
rotation and scale applied. When several contain the pointer, the one drawn on top wins. The
pointer passes through components that are hidden, virtual, made non-interactive, or
transitioning without `TransitionInteractionConfig::allow_pointer`. A disabled component
doesn't pass it through: it blocks what is behind it but receives no events, as a disabled
control does on the web.

Keyboard input isn't handled yet; the navigation stage holds its place in the schedule.

## Texture lifetime

The main atlas's `TextureRegistry` tracks each texture's reference count and when it was last
used. References are ECS components (`TextTextureRef`, `ImageTextureRef` and
`CompositeTextureRef`) on the entities that show the texture, and `bevy_ecs` hooks keep the
count right when one is added, replaced or removed, or its entity destroyed. Nothing else
increments or decrements it.

A count of zero doesn't free a texture. It becomes a candidate: when a new texture doesn't
fit, the registry frees unreferenced textures, least recently used first, until it does. It
never frees one in use, because components keep the texture coordinates of their region, and a
reused region would make them draw the wrong image. A texture can't be evicted in the frame it
was registered or last used, so an app can load several textures and attach them in the same
frame. A texture made with `eternal` is never evicted. If freeing every candidate isn't
enough, the texture fails to load, with a warning.

Transition-atlas regions aren't in the registry: each belongs to one split or merge and is
freed when it ends. The video has one slot, which a new video replaces.

## Apps and hosts

An app implements `App`: `setup` once, then `update` every frame. Each call gets a `Frame`,
with the app's `Proteus`, the host's `HostServices` and the current viewport. The app keeps
its own data; Proteus keeps the components.

`HostServices` is the one trait a platform implements, and only for what Proteus's own APIs
need: `load_asset`, which returns an asset's bytes immediately, and `fetch_async` with
`poll_fetches`, for anything that arrives later. Everything else a platform offers, such as the
clipboard or a file picker, the app uses directly, as any program would. Video isn't a host
service either: Proteus shows video but doesn't play it, and the app uploads frames from its
own player.

**A host is a crate, not a trait.** Each owns its platform's window or canvas, GPU surface,
event loop, input, and viewport, and provides its own `run` function, which creates an
`Engine` and calls `Engine::frame` each frame. What hosts share is in the layers below them:
`proteus-gpu` sets up the device the same way on every platform, and `proteus-runtime` runs the
frame. `proteus-host-winit` reads assets from a directory. `proteus-host-web` can only fetch
asynchronously, so `PreloadedHostServices` downloads a list of assets before the app starts and
serves them from memory.

### TypeScript

The TypeScript SDK is the same Rust core, compiled to WebAssembly. `proteus-sdk-web` exposes
`proteus-sdk`'s API to JavaScript with `wasm-bindgen`, each method delegating to the Rust one;
values such as a `ComponentSpec` cross as plain JavaScript objects. The TypeScript layer in
`ts/` adds handle objects with methods and the conversions between the two.

The npm package contains two WebAssembly modules: `pkg/`, built from `proteus-sdk-web`, and
`pkg-host/`, built from `proteus-host-web`, whose `mount` runs the app. The app object `mount`
passes to `setup` comes from the host's module, and the TypeScript `ProteusApp` wraps it. This
is sound because both modules compile the same bindings for `ProteusApp`, so its TypeScript
types match, and each object's methods call into the module that created it, so nothing crosses
between the two at run time.

Each call from TypeScript crosses into WebAssembly, converting its arguments on the way. That
costs little for most apps, but adds up when an app starts thousands of transitions at once;
[BENCHMARKS.md](./BENCHMARKS.md) measures it.

## Key design decisions

**Components transform; they aren't replaced.** A component keeps its identity while its
appearance changes, and moving from one screen to another is a transition of the components on
it, not a swap. This is why geometry is a single interpolable value, `QuadState`, and why
splits and merges exist as first-class shapes rather than effects built from fades.

**An ECS for state.** Components are entities in a `bevy_ecs` world, without the rest of
Bevy. Features are separate ECS components that an entity has or doesn't, so a plain rectangle
carries no image, text or effect data, and each stage of a tick is a system that queries only
what it needs. The order of stages is fixed in one place, the schedule. The ECS stays behind
`proteus-sdk`: an app uses handles, not entities and queries.

**Everything is a textured rounded rectangle, drawn in one call.** Restricting geometry to one
shape, with content as textures, is what lets one shader and one draw call render a whole
interface, and what makes any two components interpolable. The CPU cost of a frame barely
depends on what the components show. The price is per-pixel work on the GPU and a full
attribute budget, both described under [Rendering](#rendering).

**Bake, then crossfade.** A split or merge doesn't try to interpolate between the contents of
components that may be nothing alike. It bakes both ends into images and fades each piece from
one to the other while it moves, so any content transitions, at the cost of GPU work when the
transition starts and transition-atlas space while it runs.

**Handles are IDs, and `Proteus` is passed explicitly.** Handles are `Copy` and own nothing, so
closures can capture them freely, and a callback receives `&mut Proteus` as an argument rather
than holding a reference to it. A handle can't keep a destroyed component alive; using one
gives an error instead.

**Hosts are crates, not implementations of a trait.** A host's control flow differs by
platform: the native `run` blocks in winit's event loop until the window closes, while the web
`run` returns and the browser calls each frame from `requestAnimationFrame`. One polymorphic
entry point would fit neither, so each host runs its platform's loop its own way and calls the
same `Engine` underneath.

**One core, two languages.** The TypeScript SDK wraps the Rust core rather than
reimplementing it, so behavior is the same in both, and every fix reaches both. Rust is the
first-class language: the API is designed in Rust, and TypeScript follows it.

**Bring your own video player.** Decoding video well is a platform's job, and every platform
does it differently: `<video>` in a browser, `ffmpeg` or a hardware decoder natively. Proteus
only takes frames and shows them, so it ships no decoder and works with any.

**The same limits everywhere on the web.** The web host holds every config to WebGL2's limits,
even in browsers with WebGPU, so an app that works in one browser works in all of them.

## Known limits

These are known shortcomings, each planned to be addressed in a future release:

- Proteus is 2D: components can't be placed, rotated or seen in perspective in 3D space, and
  every component is a rectangle.
- Per-pixel work in the shader, rather than the number of components, limits large,
  overlapping scenes.
- Every frame collects and uploads every component, even those that haven't changed.
- Text and baked components are baked at logical-pixel resolution, so they are slightly soft
  on high-density displays.
- Text is a single line.
- One video at a time.
- No keyboard input.
- On the web, a lost GPU context leaves the canvas blank until the page reloads.
