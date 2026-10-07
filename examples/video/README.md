# Video — a `proteus-sdk` example

Shows video from the browser's own player, a `<video>` element, on a Proteus
component. Proteus shows video but doesn't play it: the element plays the
file, and each new frame is uploaded with `VideoHandle.uploadFrom`. See the top
comment in [`src/main.ts`](./src/main.ts).

Video is **experimental** in V1: one video at a time, with frames supplied by
the app.

## Run it

Build the SDK once, from the repo root, as for [`examples/gallery`](../gallery):

```bash
make build-sdk-web
```

Then, in this directory:

```bash
npm install
npm run dev
```

The video is `tiger.mp4` from `crates/proteus-shell-native/assets/videos`,
which Vite serves as this example's public directory.
