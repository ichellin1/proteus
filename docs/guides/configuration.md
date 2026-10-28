# Configuration

An app's engine settings, its `ProteusConfig`, set how much GPU memory Proteus uses, the color
behind everything, the font, and how images are loaded. The defaults suit most apps; change
them for an app with many images, a device with little memory, or a font of your own.

The settings are given to the host when the app starts, and can't be changed while it runs.

## Setting the config

In Rust, start from one of the presets below and change the fields you need:

#### Rust

```rust
# use proteus_runtime::ProteusConfig;
let mut config = ProteusConfig::web();
config.render.clear_color = [0.05, 0.05, 0.08, 1.0];
config.resources.image_max_side = Some(512);
```

Then give it to the host. The native host takes it in its `RunConfig`:

#### Rust, native host

```rust,no_run
# use proteus_runtime::{App, Frame, ProteusConfig};
# struct Hello;
# impl App for Hello {
#     fn setup(&mut self, _f: &mut Frame) {}
# }
# let config = ProteusConfig::web();
proteus_host_winit::run(
    Hello,
    proteus_host_winit::RunConfig {
        proteus: config,
        ..Default::default()
    },
);
```

The web host takes it as an argument to `run`, in the `start` function from
[Getting started](../getting-started/rust.md#run-it-in-a-browser):

#### Rust, web host

```rust
# use proteus_runtime::{App, Frame};
# pub struct Hello;
# impl App for Hello {
#     fn setup(&mut self, _f: &mut Frame) {}
# }
# #[cfg(target_arch = "wasm32")]
# pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
# let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
let mut config = proteus_runtime::ProteusConfig::web();
config.render.clear_color = [0.05, 0.05, 0.08, 1.0];
proteus_host_web::run(Hello, &canvas_id, config, services).await
# }
```

In TypeScript, give `mount` only the fields that change; the rest keep the web defaults:

#### TypeScript

```ts
import { mount } from "proteus-sdk";

await mount("canvas", {
  setup() {
    // Create the app's components.
  },
  config: {
    render: { clearColor: [0.05, 0.05, 0.08, 1] },
    resources: { imageMaxSide: 512 },
  },
});
```

TypeScript has all the settings on this page except the presets: the web host always starts
from the web preset.

## Presets

| Preset | For | Fits |
|---|---|---|
| `ProteusConfig::web()` | Most apps. The default. | Both hosts. |
| `ProteusConfig::desktop()` | Native apps with many images, on a desktop or a TV with a capable GPU: four times the atlas room, and 16,384 components. | The native host only. |
| `ProteusConfig::constrained()` | Devices with little memory, such as kiosks and embedded systems. No image or text can be larger than 1024 pixels on a side. | Both hosts. |

## The settings

### Memory

Proteus keeps the textures it draws, text, images and baked components, in an **atlas**: a set
of square GPU textures called pages. When the atlas is full, the textures used least recently
are removed to make room.

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `memory.main_atlas.page_size` | `memory.mainAtlas.pageSize` | 2048 | The width and height of each page, in pixels. No texture can be larger: larger text is clipped, a larger image is scaled down, and a larger baked component is drawn unbaked, each with a warning. |
| `memory.main_atlas.page_count` | `memory.mainAtlas.pageCount` | 4 | The number of pages. |
| `memory.transition_atlas_size` | `memory.transitionAtlasSize` | 2048 | The size of the atlas that holds images of components during splits and merges. |
| `memory.max_instances` | `memory.maxInstances` | 4096 | The most components drawn in one frame. Any beyond it aren't drawn. |

`estimated_gpu_bytes` returns roughly how much GPU memory a config uses, without video.

### Rendering

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `render.clear_color` | `render.clearColor` | Black | The color behind everything, as RGBA from 0 to 1. It shows through transparent components. |
| `render.present_mode` | `render.presentMode` | Auto vsync | How frames keep time with the display. The default matches its refresh rate; the no-vsync modes draw as fast as they can, for benchmarks. |
| `render.power_preference` | `render.powerPreference` | High performance | Which GPU to use on a machine with two. Low power saves battery. |
| `render.plain_pipeline` | `render.plainPipeline` | On | Draws plain components, a solid color with no texture, border, shadow, glow or crossfade, with a smaller, faster shader. Off draws everything with the one full shader. Either way the pixels are the same. |
| `render.skip_covered_pixels` | `render.skipCoveredPixels` | On | Doesn't shade the pixels a later opaque component covers. Saves the most where opaque components are stacked; costs a depth buffer the size of the window. The pixels are the same. |

### Frames

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `frame.dt_clamp_secs` | `frame.dtClampSecs` | 0.05 | The longest time one frame can step, in seconds. After a pause, such as a tab returning from the background, transitions continue where they were instead of jumping ahead. |
| `frame.skip_idle_frames` | `frame.skipIdleFrames` | On | Skips drawing a frame that would look the same as the last one, such as a still screen. `update` still runs every frame. |

### Text

An app has one font, used for all its text. It's Inter Bold, built into Proteus, unless the app
gives its own. It can't be changed while the app runs. Support for several fonts, such as a
regular and a bold weight, or a heading font and a body font, is planned for a future release.

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `text.default_font` | `text.font` | Inter Bold | The font of all text, from a TTF or OTF file's bytes. |

#### Rust

```rust
# use proteus_runtime::ProteusConfig;
# use proteus_runtime::config::FontSource;
# let bytes: Vec<u8> = Vec::new();
let mut config = ProteusConfig::web();
// `bytes` holds a TTF or OTF file, such as from `std::fs::read`.
match FontSource::from_bytes(bytes) {
    Ok(font) => config.text.default_font = font,
    Err(e) => eprintln!("can't read the font: {e}"),
}
```

#### TypeScript

```ts
import { mount } from "proteus-sdk";

const font = new Uint8Array(await (await fetch("font.ttf")).arrayBuffer());
await mount("canvas", {
  setup() {
    // Create the app's components.
  },
  config: { text: { font } },
});
```

`FontSource::from_bytes` checks the font, so a file that isn't one, such as a failed download,
is an error the app handles. In TypeScript, `mount` throws.

### Images

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `resources.image_max_side` | `resources.imageMaxSide` | None | Scales images down so their longer side is at most this many pixels, unless an image sets its own `max_side`. |
| `resources.lazy_load` | `resources.lazyLoad` | Off | Waits to prepare a component's text or image until the component is visible. |
| `resources.max_bake_scale` | `resources.maxBakeScale` | 2 | The most texels per logical pixel text, baked components and transitions are baked at; see below. |

### Sharpness on high-density displays

Text, components made with `.bake()`, and the images splits and merges fade between are baked
into textures, then drawn at their logical size. Proteus bakes them at the display's scale
factor, `devicePixelRatio` in a browser, so that on a high-density display a texel is a screen
pixel, and text is as sharp as the shapes the shader draws. `resources.max_bake_scale` caps it,
and it is never below 1.

**Why the default is 2.** It covers the high-density displays most people use: Retina Macs,
most high-density Windows and Linux laptops, and browsers at a pixel ratio of 2. On those,
bakes are pixel-exact. It also keeps the cost of a 3× display, most phones, down: there, bakes
are at 2, slightly soft where 3 would be exact.

**What it costs.** A bake at scale `s` takes `s²` times the atlas space of one at 1:

| Bake scale | Atlas space | Text and baked content |
|---:|---:|---|
| 1 | 1× | Slightly soft on high-density displays |
| 2 | 4× | Sharp at a pixel ratio of 2; slightly soft at 3 |
| 3 | 9× | Sharp at 3 |

Most apps never notice: text is small, and the main atlas holds 64 MiB by default. It matters
where an app bakes a lot, many large baked components or long text, especially on the web,
whose atlas pages are 2048 pixels on a side. A 1,200 × 800 baked component fits a page at 1 but
not at 2, so Proteus bakes it at the largest scale that fits instead, slightly softer. Splits
and merges bake their source and every target into the transition atlas together, so a
transition with a large component, such as a photo enlarged to fill the window going back to
its grid, bakes all of them at the largest scale at which they fit together, slightly softer
during the transition only.

**When to change it.**

- **Set it to 1**, the resolution before v0.2.0, if an app runs short of atlas space, with a
  warning that the main atlas is full: a quarter of the space for text and baked content, at
  the cost of softer text on high-density displays. Giving the atlas more pages,
  `memory.main_atlas.page_count`, is the alternative that keeps text sharp.
- **Set it to 3** for pixel-exact text on 3× phones, if the atlas has room for nine times the
  space.

When a window moves to a display of another density, its text is baked again at the new scale.
A baked component keeps the scale it was first baked at, since baking destroys what it was
drawn from.

### Diagnostics

`debug.validate_config`, on by default, logs `estimated_gpu_bytes` when the app starts. Rust only.

## When a setting doesn't fit

A GPU limits the size of its textures and buffers, and the web host holds every browser to
WebGL2's limits, so that an app behaves the same everywhere. A setting beyond the host's limits
stops the app before it starts, with a message naming the setting and what the host allows:

- The native host panics before the window opens.
- In Rust on the web, `run` returns the error.
- In TypeScript, `mount` throws it.

`ProteusConfig::check` tests a config against a host's limits without a GPU, so an app can test
its config in its own tests:

#### Rust

```rust
# use proteus_runtime::ProteusConfig;
# fn web_limits() -> proteus_runtime::wgpu::Limits {
#     proteus_runtime::wgpu::Limits::downlevel_webgl2_defaults()
# }
// In a test. `web_limits()` stands for `proteus_host_web::limits()`.
let error = ProteusConfig::desktop().check(&web_limits()).unwrap_err();
assert_eq!(error.setting, "memory.main_atlas.page_size");
```

The native host's limits are `proteus_host_winit::limits()`, and the web host's are
`proteus_host_web::limits()`.

Some fields in the API reference are marked "Not read yet". They are reserved for later
versions, and changing them has no effect.
