# Use a platform feature Proteus doesn't provide

Proteus draws the app and handles the pointer. For anything else, such as the clipboard, a file
picker, opening a web page or playing a sound, the app uses the platform directly, alongside
Proteus. Nothing in Proteus needs to know.

## In TypeScript

Call the browser's APIs from your callbacks:

#### TypeScript

```ts
button.onClick(() => {
  void navigator.clipboard.writeText("Copied from Proteus");
});

card.onClick(() => {
  window.open("https://example.com/help", "_blank");
});
```

Browsers only allow some of these, such as opening a tab, soon after the user clicks or presses
a key, so call them from a click callback.

## In Rust

A feature that works the same way on every platform the app runs on, such as reading a file
with `std::fs` in a desktop app, can be called directly.

A feature that works differently on each platform needs an implementation per platform. Give it
a trait of the app's own, with an implementation for each platform, and pass the right one in
where the app is created. The app calls the trait and never needs to know which platform it's
on.

Here the app opens a help page when a button is clicked. The trait and the app:

#### Rust

```rust
use proteus_runtime::{App, Frame};
use proteus_sdk::{ComponentSpec, QuadState, Text};
use std::rc::Rc;

/// Opens a web page in the platform's browser.
pub trait OpenLink {
    fn open(&self, url: &str);
}

pub struct HelpApp {
    links: Rc<dyn OpenLink>,
}

impl HelpApp {
    pub fn new(links: impl OpenLink + 'static) -> Self {
        Self {
            links: Rc::new(links),
        }
    }
}

impl App for HelpApp {
    fn setup(&mut self, f: &mut Frame) {
        let help = f.proteus.component(
            ComponentSpec::new(QuadState::default()).text(Text::new("Help", 24.0)),
        );
        let links = Rc::clone(&self.links);
        help.on_click(f.proteus, move |_app| links.open("https://example.com/help"));
    }
}
```

On the desktop, each operating system has a command that opens a URL in the default browser.
The native entry point passes this implementation in:

#### Rust, native

```rust,no_run
# use proteus_runtime::{App, Frame};
# use std::rc::Rc;
# pub trait OpenLink {
#     fn open(&self, url: &str);
# }
# pub struct HelpApp {
#     links: Rc<dyn OpenLink>,
# }
# impl HelpApp {
#     pub fn new(links: impl OpenLink + 'static) -> Self {
#         Self { links: Rc::new(links) }
#     }
# }
# impl App for HelpApp {
#     fn setup(&mut self, _f: &mut Frame) {}
# }
struct DesktopLinks;

impl OpenLink for DesktopLinks {
    fn open(&self, url: &str) {
        let command = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        if let Err(e) = std::process::Command::new(command).arg(url).spawn() {
            eprintln!("can't open {url}: {e}");
        }
    }
}

fn main() {
    let app = HelpApp::new(DesktopLinks);
    proteus_host_winit::run(app, proteus_host_winit::RunConfig::default());
}
```

In a browser, the app opens a new tab. The web entry point, the `start` function from
[Getting started](../getting-started/rust.md#run-it-in-a-browser), passes this one in:

#### Rust, web

```rust
# #[cfg(target_arch = "wasm32")]
# mod web {
# use proteus_runtime::{App, Frame};
# use std::rc::Rc;
# pub trait OpenLink {
#     fn open(&self, url: &str);
# }
# pub struct HelpApp {
#     links: Rc<dyn OpenLink>,
# }
# impl HelpApp {
#     pub fn new(links: impl OpenLink + 'static) -> Self {
#         Self { links: Rc::new(links) }
#     }
# }
# impl App for HelpApp {
#     fn setup(&mut self, _f: &mut Frame) {}
# }
struct BrowserLinks;

impl OpenLink for BrowserLinks {
    fn open(&self, url: &str) {
        if let Some(window) = web_sys::window() {
            let _ = window.open_with_url_and_target(url, "_blank");
        }
    }
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    let config = proteus_runtime::ProteusConfig::web();
    proteus_host_web::run(HelpApp::new(BrowserLinks), &canvas_id, config, services).await
}
# }
```

The same pattern fits any feature that differs by platform. For the clipboard or a file picker
on the desktop, the implementation can use a crate that provides it.

## Assets and downloads

Loading the app's files and downloading from URLs already go through the host: see
[Assets](../guides/hosts.md#assets). To load them some other way, such as from an archive, the
app can give the host its own `HostServices`, which is the same pattern built into Proteus.
