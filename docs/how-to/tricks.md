# Tricks

Proteus shows text, images and video through its own APIs, but underneath, everything it shows
is pixels in a texture, and an app can make a texture from any pixels it has. So anything the
platform can draw, Proteus can show. These are some ways to use that.

## Text in any font, drawn by the browser

An app has one font; see [Configuration](../guides/configuration.md#text). In a browser, text
in any CSS font, with any style, can be drawn on a 2D canvas, and its pixels shown on a
component:

#### TypeScript

```ts
// The component is 300 by 80; draw at the display's density, so the text is sharp.
const density = window.devicePixelRatio;
const canvas = document.createElement("canvas");
canvas.width = 300 * density;
canvas.height = 80 * density;
const context = canvas.getContext("2d")!;
context.scale(density, density);

// A web font must have loaded before it's drawn.
await document.fonts.load("italic 32px Georgia");
context.font = "italic 32px Georgia";
context.fillStyle = "white";
context.textBaseline = "middle";
context.fillText("Welcome back", 16, 40);

const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
const texture = app.bakeTexture(canvas.width, canvas.height, new Uint8Array(pixels.data.buffer));
card.setTexture(texture);
```

The text is now an image: it's stretched with the component, and doesn't change when the font
setting does. Draw it again to change it.

The same works for anything a canvas can draw: an SVG icon, a chart from a charting library, a
QR code.

## Pixels made in code

An app can compute pixels itself and show them with `bake_texture`. This makes a vertical
gradient:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TextureRequest};
# let mut app = Proteus::new();
# let card = app.component(ComponentSpec::new(QuadState::default()));
let (width, height) = (64, 256);
let mut rgba = Vec::with_capacity(width * height * 4);
for y in 0..height {
    // From violet at the top to blue at the bottom.
    let t = y as f32 / (height - 1) as f32;
    let pixel = [
        (140.0 * (1.0 - t)) as u8,
        (60.0 + 40.0 * t) as u8,
        (220.0 + 35.0 * t) as u8,
        255,
    ];
    for _ in 0..width {
        rgba.extend_from_slice(&pixel);
    }
}
let texture = app.bake_texture(width as u32, height as u32, rgba, TextureRequest::default());
card.set_texture(&mut app, texture)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
const width = 64;
const height = 256;
const rgba = new Uint8Array(width * height * 4);
for (let y = 0; y < height; y++) {
  // From violet at the top to blue at the bottom.
  const t = y / (height - 1);
  for (let x = 0; x < width; x++) {
    rgba.set([140 * (1 - t), 60 + 40 * t, 220 + 35 * t, 255], (y * width + x) * 4);
  }
}
card.setTexture(app.bakeTexture(width, height, rgba));
```

The pixels are RGBA, 4 bytes each, row by row from the top-left, and not premultiplied by alpha.

## Something that changes every frame

A texture is for content that changes now and then. For content that changes every frame, such
as a live chart, an animation drawn on a canvas, or a camera, use a video: it's made for a new
picture each frame. Anything that gives RGBA pixels can supply its frames:

#### TypeScript

```ts
const live = app.createVideo();
card.showVideo(live);

const canvas = document.createElement("canvas");
canvas.width = 256;
canvas.height = 256;
const context = canvas.getContext("2d")!;

const draw = (time: number) => {
  // A dot that circles the canvas.
  context.fillStyle = "black";
  context.fillRect(0, 0, 256, 256);
  context.fillStyle = "white";
  context.beginPath();
  context.arc(128 + 80 * Math.cos(time / 500), 128 + 80 * Math.sin(time / 500), 20, 0, 2 * Math.PI);
  context.fill();

  live.uploadFrame(256, 256, context.getImageData(0, 0, 256, 256).data);
  requestAnimationFrame(draw);
};
requestAnimationFrame(draw);
```

Video is experimental, and an app has one video at a time; see
[Video](../guides/content.md#video).
