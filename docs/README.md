# Proteus guides

Proteus is a UI framework whose components transform into one another instead of loading new
screens. These guides show how to build with it, in Rust first and then in TypeScript.

The guides name things in their Rust form. TypeScript uses the same names in camelCase:
`split_to` is `splitTo`, and `on_transition_complete` is `onTransitionComplete`.

## Getting started

Build a button that transforms into a panel, step by step:

- [In Rust](./getting-started/rust.md), in a desktop window and in a web page.
- [In TypeScript](./getting-started/typescript.md), in a web page.

## Guides

- [Components and geometry](./guides/components.md): what a component is, where it is, and how
  components contain each other.
- [1→1 transitions](./guides/transitions.md): moving a component, and turning one component into
  another.
- [Splits and merges](./guides/splits-and-merges.md): one component into several, and several
  into one.
- [Interaction](./guides/interaction.md): pointer events, which component gets input, and
  hover, pressed, focused and disabled styles.
- [Keyboard and focus](./guides/keyboard-and-focus.md): actions and the key map, handling keys,
  moving focus and focus groups, and the input devices an app accepts.
- [Text, images and video](./guides/content.md): what a component shows, and how to change it.
- [Configuration](./guides/configuration.md): GPU memory, the background color, the font, how
  images load, and how keys repeat.
- [Hosts and platforms](./guides/hosts.md): what runs an app, loading assets, the size of the
  window, and the keyboard on the web.

## How-tos

- [Chain transitions](./how-to/chaining-transitions.md): run transitions one after another.
- [Stagger a group](./how-to/staggering.md): start a group's transitions one after another, as a
  wave.
- [Load an image](./how-to/loading-images.md): from a file that comes with the app, or a URL,
  cropped to fit.
- [Use a custom easing curve](./how-to/custom-easing.md): copy a curve from CSS, or write your
  own.
- [Use a platform feature Proteus doesn't provide](./how-to/platform-features.md), such as the
  clipboard or opening a web page.
- [Make an app work with the keyboard](./how-to/keyboard-navigation.md): where focus starts and
  goes as screens change, groups, Back, and one highlight at a time.
- [Tricks](./how-to/tricks.md): show anything the platform can draw, such as text in any font.

## API reference

- [Rust](https://ichellin1.github.io/proteus/api/rust/proteus_sdk/): `proteus-sdk` for building
  an app, `proteus-runtime` for the app and host contract, and the native host,
  `proteus-host-winit`. The web host, `proteus-host-web`, is documented
  [separately](https://ichellin1.github.io/proteus/api/rust-web/proteus_host_web/), since it only
  builds for WebAssembly.
- [TypeScript](https://ichellin1.github.io/proteus/api/ts/): the `proteus-sdk` npm package.
