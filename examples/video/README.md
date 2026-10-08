# Video

Video from the platform's own player, shown on a Proteus component. Proteus shows video but
doesn't play it: the player decodes each frame, and the app uploads it. In the browser, the
player is a `<video>` element, and `VideoHandle.uploadFrom` uploads each new frame.

Video is **experimental**: an app has one video at a time, with frames supplied by the app. See
[Video](../../docs/guides/content.md#video).

The video is `tiger.mp4`, from the reference demo's assets in
`crates/proteus-shell-native/assets/videos`.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

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
