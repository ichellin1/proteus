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

## API reference

- [Rust](https://ichellin1.github.io/proteus/api/rust/proteus_sdk/): `proteus-sdk` for building
  an app, `proteus-runtime` for the app and host contract, and the native host,
  `proteus-host-winit`. The web host, `proteus-host-web`, is documented
  [separately](https://ichellin1.github.io/proteus/api/rust-web/proteus_host_web/), since it only
  builds for WebAssembly.
- [TypeScript](https://ichellin1.github.io/proteus/api/ts/): the `proteus-sdk` npm package.
