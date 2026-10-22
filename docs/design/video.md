# Video: copying frames, punch-through, playback controls, several videos, and splits and merges

**Status:** planned for v0.5.0. Video is experimental in v0.1.0.

Proteus shows video but doesn't play it: the app brings its own player and uploads each frame
with `VideoHandle::upload_frame` (see [ARCHITECTURE.md](../../ARCHITECTURE.md#apps-and-hosts)).
V1 supports one video at a time; creating another replaces it.

## A video handle with playback controls

Play, pause, seek, loop, mute, volume, duration, position, and an "ended" event, designed around
bringing your own player. The handle drives the app's player rather than containing one, so the
design work is an interface most players can sit behind: on the web, `<video>`, hls.js, Shaka
and video.js; natively, `ffmpeg`, GStreamer and platform decoders.

## Several videos at once

Each with its own texture. Today the video has one slot in the pipeline, and a new video
replaces the old one.

## Copying frames straight to the GPU on the web

Copy frames from a `<video>` element with `copy_external_image_to_texture`, instead of reading
them back through a canvas.

Today the web shell draws each frame into an offscreen 2D canvas, reads it back with
`getImageData`, copies it into WebAssembly memory, and uploads it again with `write_texture`
(`proteus-shell-web/src/video_player.rs`). At 1080p that is about 8 MB a frame crossing between
the GPU and the CPU twice, 30 to 60 times a second, and the readback waits for the GPU. On a
Chromebook, playing video raises the CPU's clock speed noticeably while its use stays where
expected, which fits time spent waiting and copying rather than computing. Copying straight to
the GPU removes the readback and both CPU copies.

## Punch-through video

An alternative to drawing video on a component, for when drawing it costs too much or isn't
allowed. The platform shows the video itself, in a region of the screen the app leaves clear,
and Proteus draws the rest of the UI around or over it. No frame is copied: the decoder's output
goes to the screen, and the compositor can give it a hardware overlay, as browsers and video
players do for full screen video.

**Why:**

- **Full screen video on low-end devices.** Even copied straight to the GPU, a frame is still
  copied once and drawn by Proteus's shader every frame. Punch-through costs what the platform's
  own video player costs.
- **Protected (DRM) video.** On the web, a `<video>` playing content protected through EME can't
  be read into a texture at all. Natively, it depends on the DRM's security level. Content
  decrypted and decoded in software, such as Widevine L3, can sometimes reach a texture: on
  Android it can render to a `SurfaceTexture`. Content protected in hardware, such as Widevine
  L1, FairPlay or PlayReady SL3000, stays in protected memory and reaches the screen only
  through the platform's secure video path: AVFoundation won't hand an app FairPlay frames, and
  Android's secure decoders render only to a `SurfaceView`. Providers usually allow the
  software level up to about 720p and require the hardware level for HD and 4K. So natively,
  protected video can still be drawn on components at 720p, which suits tiles and previews,
  and only full screen HD needs punch-through.
- **TVs.** TV platforms, web-based or native, expect video on their own video plane, and
  protected content there is the norm. See v0.8.0.

**What it gives up:** the video isn't in Proteus's pipeline, so it takes no shader effects, can't
be swept into a split or merge, and isn't sampled by a transition. It's understood as an
alternative for when those don't matter, such as full screen playback. The region can still
move and resize with its component, a frame behind at most.

**How it could work on the web:** a `<video>` element positioned under the canvas, which is
transparent, with the region over the video left clear: the component draws nothing there,
or a cut-out. Proteus keeps the element's position, size and visibility in step with the
component each frame it changes, in CSS pixels. UI drawn by Proteus over the video, such as
playback controls, works, since the canvas is on top. The canvas needs a transparent clear
color and premultiplied alpha. Rounded corners could come from CSS on the element.

**Natively:** each host needs the platform's own video layer beside its surface: an
`AVPlayerLayer` on macOS and iOS, DirectComposition on Windows, a Wayland subsurface on Linux, a
`SurfaceView` on Android. That's much more work than the web, and winit doesn't offer it, so
the web comes first and each native platform is its own item.

**Open questions:** how an app asks for it: a kind of component, or a mode on a video handle;
switching a playing video between punch-through and a texture, for a transition out of full
screen; and whether a component should be able to sit under the video as well as over it.

**How to measure:** first confirm where the time goes today: a Chrome Performance recording on
the Chromebook during full screen playback, which should show `getImageData` and the uploads.
Then compare CPU use, clock speed and dropped frames, with Chrome's `chrome://media-internals`,
playing the same video three ways: as a texture today, copied straight to the GPU, and punched
through, against the same video in a plain `<video>` page.

## Live video during splits and merges

A component showing video that is swept into a split or merge shows a frozen frame for the
transition's length: the split bakes the source, video included, into a still image, and nothing
in `topology.rs` knows about `VideoPlayer` or `VideoCrossfade`. A 1→1 transition already keeps
video playing, by crossfading from an image in the main atlas to the live video.

The reference demo hits this, going from the video screen back to its tiles, a video component
in a `Row` split. It reads fine in practice, since the frozen frame is the one shown an instant
before the crossfade out, not a stale one.
