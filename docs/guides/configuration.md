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

### Frames

| Rust | TypeScript | Default | What it sets |
|---|---|---|---|
| `frame.dt_clamp_secs` | `frame.dtClampSecs` | 0.05 | The longest time one frame can step, in seconds. After a pause, such as a tab returning from the background, transitions continue where they were instead of jumping ahead. |

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
// In a test. `web_limits()` stands for the web host's limits, below.
let error = ProteusConfig::desktop().check(&web_limits()).unwrap_err();
assert_eq!(error.setting, "memory.main_atlas.page_size");
```

The native host's limits are `proteus_host_winit::limits()`, and the web host's are
`proteus_host_web::limits()`.

Some fields in the API reference are marked "Not read yet". They are reserved for later
versions, and changing them has no effect.
