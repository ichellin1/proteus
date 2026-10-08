# Hosts and platforms

A Proteus app doesn't open its own window. A **host** runs it: the host owns the window or
canvas, the GPU, the frame loop, pointer input and the screen's size, and calls the app's code
each frame. The same app runs on either host unchanged. Proteus currently ships with two hosts: `proteus-host-winit` and `proteus-host-web`. Developers are welcome to create thier own hosts for unsupported platforms. New hosts like ones that support mobile targets are planned for future releases. 

| Host | Runs on | Draws with |
|---|---|---|
| `proteus-host-winit`, the native host | macOS, Windows and Linux, in a desktop window | Metal, DirectX 12 or Vulkan |
| `proteus-host-web`, the web host | Browsers, on a `<canvas>`, compiled to WebAssembly | WebGPU, or WebGL2 where WebGPU isn't available |

Proteus is developed and tested on macOS. Windows and Linux use the same code, through winit and
wgpu, but aren't tested yet.

A TypeScript app always runs in the web host: `mount` starts it. The rest of this page is about
Rust apps, except where it says otherwise.

## The app

A Rust app implements `App`. The host calls `setup` once, before the first frame, and `update`
every frame after the frame's events and callbacks have run, and before it draws:

#### Rust

```rust
use proteus_runtime::{App, Frame};
use proteus_sdk::{ComponentSpec, Handle, QuadState};

struct Clock {
    hand: Option<Handle>,
}

impl App for Clock {
    fn setup(&mut self, f: &mut Frame) {
        self.hand = Some(f.proteus.component(ComponentSpec::new(QuadState::default())));
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        // Turn the hand once a minute.
        let Some(hand) = self.hand else { return };
        if let Some(snapshot) = f.proteus.get(hand) {
            let mut turned = snapshot.geometry;
            turned.rotation -= dt * std::f32::consts::TAU / 60.0;
            let _ = hand.set_declared_geometry(f.proteus, turned);
        }
    }
}
```

`update` is optional: an app that only reacts to input does everything in callbacks registered
in `setup`. In TypeScript, `mount` takes a `setup` function and an optional `update` function
that work the same way.

Each call is given a `Frame`, which holds:

- **`proteus`**, the app's components, channels and callbacks.
- **`services`**, the host's assets and network access; see [Assets](#assets).
- **`viewport`**, the size of the window or canvas; see [The viewport](#the-viewport).

The app keeps its own data in its own struct, such as `hand` above. The host owns everything
else.

## Assets

An app's files, such as images and fonts, are **assets**, each named by a key such as
`"photos/beach.jpg"`. `Frame::load_asset` returns an asset's bytes at once, and
`Frame::load_texture` loads an image asset as a texture; see
[Text, images and video](./content.md).

The two hosts find assets differently:

- **The native host** reads them from a directory, `asset_dir` in its `RunConfig`, which is the
  working directory by default. `load_asset("photos/beach.jpg")` reads
  `photos/beach.jpg` in it.
- **The web host** can't read a file at once, since a browser only downloads asynchronously.
  `PreloadedHostServices::fetch` downloads a list of assets before the app starts, and
  `load_asset` returns them from memory. An asset not on the list isn't found.

#### Rust

```rust
# #[cfg(target_arch = "wasm32")]
# pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
# struct Gallery;
# impl proteus_runtime::App for Gallery {
#     fn setup(&mut self, _f: &mut proteus_runtime::Frame) {}
# }
// Download two assets from the page's `assets/` folder, then start the app.
let services =
    proteus_host_web::PreloadedHostServices::fetch("assets/", &["logo.png", "photos/beach.jpg"])
        .await;
let config = proteus_runtime::ProteusConfig::web();
proteus_host_web::run(Gallery, &canvas_id, config, services).await
# }
```

### Fetching

For anything that isn't known before the app starts, such as an image from a server,
`fetch_async` starts a download and returns at once. Its result arrives later, from
`poll_fetches`, which `update` calls each frame. Both hosts accept an asset's key or a full
`http://` or `https://` URL.

#### Rust

```rust
use proteus_runtime::{App, FetchId, Frame};
use proteus_sdk::{ComponentSpec, Handle, Image, QuadState};

struct Photo {
    frame: Option<Handle>,
    download: Option<FetchId>,
}

impl App for Photo {
    fn setup(&mut self, f: &mut Frame) {
        self.frame = Some(f.proteus.component(ComponentSpec::new(QuadState::default())));
        self.download = Some(f.fetch_async("https://example.com/photo.jpg"));
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        for (id, bytes) in f.poll_fetches() {
            if Some(id) != self.download {
                continue;
            }
            // `None` means the download failed; the host logs why.
            if let (Some(frame), Some(bytes)) = (self.frame, bytes) {
                let _ = frame.set_image(f.proteus, Image::new(bytes));
            }
        }
    }
}
```

`cancel_fetch` cancels a download that is no longer wanted, so its result never arrives. A URL
the native host fetches fails after a timeout, so a server that never answers doesn't hold it
up.

A TypeScript app uses the browser's own `fetch`, as in
[Text, images and video](./content.md#images).

### Your own services

`HostServices` is the trait behind `services`. To load assets some other way, such as from an
archive or a database, implement it and pass it to the native host's `run_with_services`, or as
the web host's `run`'s last argument.

## The viewport

`Frame::viewport` is the window's or canvas's size:

- **`logical_size`**, in logical pixels, the units positions and sizes are in. A logical pixel
  is the same size on every display; a high-density display draws each with several physical
  pixels.
- **`scale_factor`**, physical pixels per logical pixel: `2.0` on a typical high-density
  display. Proteus uses it to draw text and images sharply; an app rarely needs it.

The origin is the center of the viewport, so the edges are at plus and minus half of
`logical_size`. When the window is resized, components stay where they are relative to the
center. An app that lays components out against an edge reads the viewport in `update` and moves
them when it changes:

#### Rust

```rust
# use proteus_runtime::{App, Frame};
# use proteus_sdk::glam::Vec2;
# use proteus_sdk::Handle;
# struct Toolbar {
#     bar: Handle,
#     size: Vec2,
# }
# impl App for Toolbar {
#     fn setup(&mut self, _f: &mut Frame) {}
fn update(&mut self, f: &mut Frame, _dt: f32) {
    let size = f.viewport.logical_size;
    if size == self.size {
        return;
    }
    self.size = size;
    // Keep the bar along the top edge, as wide as the window.
    if let Some(snapshot) = f.proteus.get(self.bar) {
        let mut along_top = snapshot.geometry;
        along_top.size.x = size.x;
        along_top.position.y = size.y / 2.0 - along_top.size.y / 2.0;
        let _ = self.bar.set_declared_geometry(f.proteus, along_top);
    }
}
# }
```

In TypeScript, the canvas's size is its CSS size, which the page sets, such as `width: 100vw`.
Read it from the canvas element, with `clientWidth` and `clientHeight`.

## Platform notes

- **Input** is the pointer: a mouse natively, and a mouse, touch or pen in a browser. Keyboard
  input isn't supported yet.
- **A hidden tab** stops the browser's frame loop. When the tab returns, transitions continue
  where they stopped.
- **A lost GPU context** in a browser, which can happen when the GPU driver resets or the
  system is short of memory, leaves the canvas blank until the page reloads.
- **The web host holds every browser to WebGL2's limits**, even with WebGPU, so an app behaves
  the same in every browser. A setting beyond them, such as an atlas page larger than 2048
  pixels, stops the app before it starts; see [Configuration](./configuration.md).
- **Safe areas**, such as a phone's notch, aren't reported yet: `viewport.safe_area` is always
  zero. That is correct on desktops.

For what Proteus doesn't do, such as playing audio or reading files the user picks, the app
uses the platform directly, alongside Proteus: Rust crates natively, and the browser's APIs on
the web.
