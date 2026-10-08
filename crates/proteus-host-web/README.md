# proteus-host-web

Runs a Proteus app on an HTML canvas, compiled to WebAssembly, with WebGPU where it's available and WebGL2 otherwise.

Part of [Proteus](https://github.com/ichellin1/proteus).

From Rust, `proteus_host_web::run` runs an app on a canvas; see [Getting started in Rust](https://github.com/ichellin1/proteus/blob/main/docs/getting-started/rust.md#run-it-in-a-browser). It is also what the [`proteus-sdk` npm package](https://www.npmjs.com/package/proteus-sdk) runs on. It only builds for `wasm32-unknown-unknown`.

## License

MIT or Apache-2.0, at your option.
