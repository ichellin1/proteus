# Proteus guides

Proteus is a UI framework whose components transform into one another instead of loading new
screens. These guides show how to build with it, in Rust first and then in TypeScript.

## Getting started

Build a button that transforms into a panel, step by step:

- [In Rust](./getting-started/rust.md), in a desktop window and in a web page.
- [In TypeScript](./getting-started/typescript.md), in a web page.

## API reference

- [Rust](https://ichellin1.github.io/proteus/api/rust/proteus_sdk/): `proteus-sdk` for building
  an app, `proteus-runtime` for the app and host contract, and the native host,
  `proteus-host-winit`. The web host, `proteus-host-web`, is documented
  [separately](https://ichellin1.github.io/proteus/api/rust-web/proteus_host_web/), since it only
  builds for WebAssembly.
- [TypeScript](https://ichellin1.github.io/proteus/api/ts/): the `proteus-sdk` npm package.
