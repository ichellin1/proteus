/**
 * Proteus TypeScript SDK example: video from your own player.
 *
 * Proteus shows video but doesn't play it. Here the player is the browser's
 * own `<video>` element. It plays the file, and each new frame is uploaded to
 * a Proteus video with `VideoHandle.uploadFrom`, driven by the element's
 * `requestVideoFrameCallback`. The video is shown on a component, which
 * grows when the pointer is over it, the way any component can transition.
 *
 * Clicking toggles play and pause. Those are the player's controls, not
 * Proteus's: Proteus only shows whatever frames it is given.
 *
 * Video is experimental in V1: one video at a time, with frames supplied by
 * the app.
 */

import {
  mount,
  topLeftToWorld,
  type Geometry,
  type ProteusApp,
} from "proteus-sdk";

/** Served by Vite from the native demo's sample videos; see vite.config.ts. */
const VIDEO_URL = "/tiger.mp4";

function main(app: ProteusApp, canvas: HTMLCanvasElement): void {
  // The player: an ordinary <video> element, never added to the page.
  // Muted, so the browser lets it start without a click.
  const element = document.createElement("video");
  element.src = VIDEO_URL;
  element.muted = true;
  element.loop = true;
  element.playsInline = true;

  // A 16:9 screen in the middle of the canvas.
  const width = Math.min(canvas.clientWidth * 0.7, 960);
  const height = (width * 9) / 16;
  const center = topLeftToWorld(
    canvas.clientWidth / 2,
    canvas.clientHeight / 2,
    canvas.clientWidth,
    canvas.clientHeight,
  );
  const geometry: Geometry = {
    position: { x: center.x, y: center.y, z: 0 },
    size: { width, height },
    rotation: 0,
    scale: 1,
    anchor: { x: 0.5, y: 0.5 },
    color: { r: 1, g: 1, b: 1, a: 1 },
    cornerRadius: 16,
  };
  const screen = app.component({ geometry, hover: { scale: 1.03 } });

  const video = app.createVideo();
  screen.showVideo(video);

  // Upload each new frame as the player presents it.
  const upload = () => {
    video.uploadFrom(element);
    element.requestVideoFrameCallback(upload);
  };
  element.requestVideoFrameCallback(upload);

  screen.onClick(() => {
    if (element.paused) {
      void element.play();
    } else {
      element.pause();
    }
  });

  void element.play();
}

const canvas = document.getElementById("app") as HTMLCanvasElement;
await mount("app", {
  setup(app) {
    main(app, canvas);
  },
});
