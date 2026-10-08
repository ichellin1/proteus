# Getting started in Rust

This guide builds a small Proteus app: a button that transforms into a panel when it's clicked,
and back again when the panel is clicked. Nothing is loaded or swapped; the button becomes the
panel. It runs in a desktop window, and then, from the same code, in a web page.

You need Rust 1.89 or later. Check with `cargo --version`. If you don't have it, install it
with `rustup`, Rust's installer. On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

On Windows:

```bash
winget install Rustlang.Rustup
```

Then open a new terminal, so that `cargo` is on your `PATH`.

## Create the project

```bash
cargo new hello-proteus
cd hello-proteus
```

Add Proteus to `Cargo.toml`:

```toml
[dependencies]
proteus-sdk = "0.1"
proteus-runtime = "0.1"
proteus-host-winit = "0.1"
```

- `proteus-sdk` is what you build an app with: components, transitions and input.
- `proteus-runtime` defines the app: the `App` trait, which a host runs.
- `proteus-host-winit` is the native host. It opens the window, runs the frame loop, and passes
  the pointer to your app.

## Write the app

Replace `src/main.rs` with:

```rust,no_run
use proteus_runtime::glam::{Vec2, Vec4};
use proteus_runtime::{App, Frame};
use proteus_sdk::{ComponentSpec, QuadState, Text, TransitionConfig};

struct Hello;

impl App for Hello {
    fn setup(&mut self, f: &mut Frame) {
        let app = &mut *f.proteus;
        let violet = Vec4::new(0.45, 0.35, 0.8, 1.0);

        // A pill-shaped button in the middle of the window.
        let button = app.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(200.0, 60.0),
                color: violet,
                corner_radius: 30.0,
                ..Default::default()
            })
            .text(Text::new("Open", 24.0)),
        );

        // The panel the button becomes. It starts hidden; a transition into it
        // shows it.
        let panel = app.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(480.0, 320.0),
                color: violet,
                corner_radius: 16.0,
                ..Default::default()
            })
            .text(Text::new("Close", 24.0))
            .visible(false),
        );

        // A channel transitions one component into another.
        let channel = app.transition_channel(None);
        let config = TransitionConfig {
            duration: 0.4,
            ..Default::default()
        };
        button.on_click(app, move |app| {
            channel.set(app, panel, button, config, false);
        });
        panel.on_click(app, move |app| {
            channel.set(app, button, panel, config, false);
        });
    }
}

fn main() {
    proteus_host_winit::run(Hello, proteus_host_winit::RunConfig::default());
}
```

Run it:

```bash
cargo run
```

Click the button: it grows into the panel, changing shape on the way. Click the panel to turn it
back into the button.

## Run it in a browser

The same app runs in a web page through the web host, `proteus-host-web`, compiled to
WebAssembly. It draws with WebGPU, or WebGL2 where WebGPU isn't available.

Add the WebAssembly target and `wasm-pack`, which builds a crate for the web:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

The app moves into a library, so that the native `main` and the web entry point can share it.
In `Cargo.toml`, add the library and give each host to its own target:

```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
proteus-sdk = "0.1"
proteus-runtime = "0.1"

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
proteus-host-winit = "0.1"

[target.'cfg(target_arch = "wasm32")'.dependencies]
proteus-host-web = "0.1"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
```

Create `src/lib.rs`, and move the `use` lines, `struct Hello` and its `impl App` into it from
`src/main.rs`, making the struct public: `pub struct Hello;`. Then add the web entry point at the
end of `src/lib.rs`:

```rust
# use proteus_runtime::{App, Frame};
# pub struct Hello;
# impl App for Hello {
#     fn setup(&mut self, _f: &mut Frame) {}
# }
/// Runs the app on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    // The app loads no assets, so there is nothing to download first.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    let config = proteus_runtime::ProteusConfig::web();
    proteus_host_web::run(Hello, &canvas_id, config, services).await
}
```

`src/main.rs` keeps only the native entry point:

```rust,no_run
# mod hello_proteus {
#     use proteus_runtime::{App, Frame};
#     pub struct Hello;
#     impl App for Hello {
#         fn setup(&mut self, _f: &mut Frame) {}
#     }
# }
fn main() {
    proteus_host_winit::run(hello_proteus::Hello, proteus_host_winit::RunConfig::default());
}
```

Create `index.html` in the project's root, with a canvas that fills the window, and a script
that loads the WebAssembly and starts the app on the canvas:

```html
<!doctype html>
<html>
  <body style="margin: 0">
    <canvas id="app" style="display: block; width: 100vw; height: 100vh"></canvas>
    <script type="module">
      import init, { start } from "./pkg/hello_proteus.js";
      await init();
      await start("app");
    </script>
  </body>
</html>
```

Build it, then serve the project's folder. A page that loads WebAssembly must be served over
HTTP, not opened as a file. Python, which comes with macOS and most Linux distributions, has a
server for this:

```bash
wasm-pack build --target web
python3 -m http.server 8080
```

Open <http://localhost:8080>. `cargo run` still runs the app natively.

## What's going on

**Components** are what you see: each is a shape, with a position, size, color and corner
radius, that can carry text or an image. `app.component` creates one from a `ComponentSpec` and
returns a `Handle`, a small copyable ID you use to change it later. Positions are in pixels,
measured from the center of the window, with y pointing up; both components here sit at the
center.

**A transition channel** turns one component into another: `channel.set(app, to, from, …)`
hides `from` and moves `to` from `from`'s shape to its own, over the transition's duration.
Neither component knows about the other; the channel holds the relationship.

**Callbacks** react to input. `on_click` takes a closure that is called with the app each time
the component is clicked. Handles and channels are `Copy`, so the closures capture them by
value.

**The app and the host.** Your app implements `App`: `setup` runs once to create the
components, and an optional `update` runs every frame. The host owns everything around it: the
window or canvas, the GPU and the frame loop. `proteus_host_winit::run` is the native host, and
`proteus_host_web::run` the web host; the app is the same for both.

## Next

The [guides](../README.md) cover each part in depth, and the
[API reference](https://ichellin1.github.io/proteus/api/rust/proteus_sdk/) lists everything
`proteus-sdk` has.
