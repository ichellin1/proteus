<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/ichellin1/proteus/main/brand/logo/assets/lockup-color-dark.png">
    <img alt="Proteus" src="https://raw.githubusercontent.com/ichellin1/proteus/main/brand/logo/assets/lockup-color-light.png" width="210">
  </picture>
</p>

# proteus-sdk

The app API of [Proteus](https://github.com/ichellin1/proteus), a rendering framework designed around a new UX paradigm: **UI components are metamorphic.** They do not navigate to new screens or swap out for different components — they *transform* into them. A button can become a list. A list can collapse back into a button. A list item can become a video player. The component does not disappear; it *shapeshifts*, and the transition between one form and another is a first-class, visually continuous experience.

```toml
[dependencies]
proteus-sdk = "0.2"
proteus-runtime = "0.2"
proteus-host-winit = "0.2"
```

A button that turns into a panel when it's clicked, and back when the panel is clicked:

```rust,no_run
use proteus_runtime::{App, Frame};
use proteus_sdk::glam::Vec2;
use proteus_sdk::{ComponentSpec, QuadState, Text, TransitionConfig};

struct Hello;

impl App for Hello {
    fn setup(&mut self, f: &mut Frame) {
        let app = &mut *f.proteus;
        let button = app.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(200.0, 60.0),
                corner_radius: 30.0,
                ..Default::default()
            })
            .text(Text::new("Open", 24.0)),
        );
        let panel = app.component(
            ComponentSpec::new(QuadState {
                size: Vec2::new(480.0, 320.0),
                corner_radius: 16.0,
                ..Default::default()
            })
            .text(Text::new("Close", 24.0))
            .visible(false),
        );
        let channel = app.transition_channel(None);
        let config = TransitionConfig::default();
        button.on_click(app, move |app| channel.set(app, panel, button, config, false));
        panel.on_click(app, move |app| channel.set(app, button, panel, config, false));
    }
}

fn main() {
    proteus_host_winit::run(Hello, proteus_host_winit::RunConfig::default());
}
```

The same app runs in a browser with `proteus-host-web`.

## Learn more

- [Getting started in Rust](https://github.com/ichellin1/proteus/blob/main/docs/getting-started/rust.md),
  natively and in a browser.
- [Guides](https://github.com/ichellin1/proteus/tree/main/docs): the concepts, and how-tos.
- [API reference](https://ichellin1.github.io/proteus/api/rust/proteus_sdk/).
- [Examples](https://github.com/ichellin1/proteus/tree/main/examples), and
  [the reference demo](https://ichellin1.github.io/proteus/), running in your browser.
- Proteus also has a [TypeScript SDK](https://www.npmjs.com/package/proteus-sdk).

## License

MIT or Apache-2.0, at your option.
