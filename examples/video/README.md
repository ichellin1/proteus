# Video

Video from your own player, shown on a Proteus component. Proteus shows video but doesn't play
it: the player decodes each frame, and the app uploads it. Clicking the video pauses and resumes
the player. The component grows when the pointer is over it, the way any component can.

- **In Rust,** the player is a trait of the app's own, with an implementation for each platform:
  the `ffmpeg` command on the desktop, and a `<video>` element in a browser. The app uploads the
  player's newest frame in `update`, each frame. See the top of
  [`rust/src/lib.rs`](./rust/src/lib.rs).
- **In TypeScript,** the player is a `<video>` element, and `VideoHandle.uploadFrom` uploads each
  frame it shows. See the top of [`typescript/src/main.ts`](./typescript/src/main.ts).

Video is **experimental**: an app has one video at a time, with frames supplied by the app. See
[Video](../../docs/guides/content.md#video).

The video is `tiger.mp4`, from the reference demo's assets in
`crates/proteus-shell-native/assets/videos`.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### Rust

On the desktop, the video is decoded by `ffmpeg`, which must be installed. Check with
`ffmpeg -version`. If you don't have it, install it on macOS with Homebrew:

```bash
brew install ffmpeg
```

on Windows:

```bash
winget install Gyan.FFmpeg
```

or on Linux with your distribution's package manager, for example on Ubuntu:

```bash
sudo apt install ffmpeg
```

Then, from the repository's root:

```bash
cargo run -p video
```

To play another video, give its path: `cargo run -p video -- path/to/video.mp4`.

In a browser, which plays the video itself, so `ffmpeg` isn't needed:

```bash
make example-video-web
```

Then open <http://localhost:8080/examples/video/rust/>. Without make, build it for the web, then
serve the repository from its root:

```bash
wasm-pack build examples/video/rust --target web --release
python3 -m http.server 8080
```

### TypeScript

From the repository's root:

```bash
make example-video-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/video/typescript
npm install
npm run dev
```
