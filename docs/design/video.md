# Video: playback controls, several videos, and video in splits and merges

**Status:** a candidate. Video is experimental in v0.1.0.

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

## Live video during splits and merges

A component showing video that is swept into a split or merge shows a frozen frame for the
transition's length: the split bakes the source, video included, into a still image, and nothing
in `topology.rs` knows about `VideoPlayer` or `VideoCrossfade`. A 1→1 transition already keeps
video playing, by crossfading from an image in the main atlas to the live video.

The reference demo hits this, going from the video screen back to its tiles, a video component
in a `Row` split. It reads fine in practice, since the frozen frame is the one shown an instant
before the crossfade out, not a stale one.
