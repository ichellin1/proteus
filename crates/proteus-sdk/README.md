# proteus-sdk

The Proteus app API: create components, transition them, and handle input.

Part of [Proteus](https://github.com/ichellin1/proteus).

```toml
[dependencies]
proteus-sdk = "0.1"
proteus-runtime = "0.1"
proteus-host-winit = "0.1"
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

The same app runs in a browser with `proteus-host-web`. [Getting started in Rust](https://github.com/ichellin1/proteus/blob/main/docs/getting-started/rust.md)
walks through both, and the [guides](https://github.com/ichellin1/proteus/tree/main/docs) cover each part. Proteus also has a
[TypeScript SDK](https://www.npmjs.com/package/proteus-sdk).

## License

MIT or Apache-2.0, at your option.
