# Images and video

A component can show an image or video, as well as text; see [Text](./text.md) for text. Proteus
draws them from textures on the GPU. The host prepares the textures from what you give it, and
decodes images before it next draws. You give the content. When and how it reaches the GPU is
the host's job.

## Images

An image is a PNG or JPEG file's bytes, or RGBA pixels with their size; see
[Images from pixels](#images-from-pixels). It fills the component. An image of a different shape
from the component is stretched to fit, unless it is cropped.

#### Rust

```rust,no_run
# use proteus_sdk::{ComponentSpec, Image, Proteus, QuadState};
# let mut app = Proteus::new();
// A PNG or JPEG file, read at run time.
let bytes = std::fs::read("photo.jpg")?;
let photo = app.component(
    ComponentSpec::new(QuadState::default()).image(Image::new(bytes).with_max_side(800)),
);
# Ok::<(), std::io::Error>(())
```

#### TypeScript

```ts
const response = await fetch("photo.jpg");
const bytes = new Uint8Array(await response.arrayBuffer());
const photo = app.component({
  geometry: app.get(card)!.geometry,
  image: { bytes, maxSide: 800 },
});
```

- **The component's color tints the image.** White, the default, shows it unchanged.
- **`max_side` scales the image down** so its longer side is at most that many pixels. A photo
  is often much larger than it is drawn, and a smaller image uses less GPU memory. Without
  `max_side`, the app's default from [Configuration](./configuration.md) applies.
- **An image larger than an atlas page**, 2048 pixels by default, is scaled down to fit the
  page, with a warning.
- **A file that can't be decoded** is removed from the component, with a warning.

### Images from pixels

An app that draws its own pixels, such as on a browser canvas or in code, gives Proteus the
pixels as they are, rather than encoding a PNG for Proteus to decode again. The pixels are RGBA,
4 bytes each, row by row from the top, with straight alpha, in sRGB. This is what decoding a
file gives, and what a canvas's `getImageData` returns.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Image, Proteus, QuadState};
# let mut app = Proteus::new();
// A 64 × 64 gradient, from black at the left to purple at the right.
let (width, height) = (64u32, 64u32);
let mut pixels = Vec::with_capacity((width * height * 4) as usize);
for _y in 0..height {
    for x in 0..width {
        let t = (x * 255 / (width - 1)) as u8;
        pixels.extend([t / 2, 0, t, 255]);
    }
}
let swatch = app.component(
    ComponentSpec::new(QuadState::default()).image(Image::from_rgba(width, height, pixels)),
);
```

#### TypeScript

A canvas's `ImageData.data`, a `Uint8ClampedArray`, is passed as it is:

```ts
const canvas = document.createElement("canvas");
canvas.width = canvas.height = 64;
const ctx = canvas.getContext("2d")!;
const gradient = ctx.createLinearGradient(0, 0, 64, 0);
gradient.addColorStop(0, "#000000");
gradient.addColorStop(1, "#7a00ff");
ctx.fillStyle = gradient;
ctx.fillRect(0, 0, 64, 64);
const { data, width, height } = ctx.getImageData(0, 0, 64, 64);
const swatch = app.component({
  geometry: app.get(card)!.geometry,
  image: { pixels: data, width, height },
});
```

- **Everything after decoding is the same:** `max_side`, scaling down to fit an atlas page,
  cropping, and waiting until the component is visible with `resources.lazy_load`.
- **Pixels of the wrong size** are removed from the component, with a warning, as a file that
  can't be decoded is. That means anything but `width × height × 4` bytes, or a width or height
  of 0.
- **The pixels are copied once,** out of the canvas and into the app's memory. This saves the
  encode and the decode.
- **They stay on the component once baked,** as a file's bytes do. They aren't compressed, so a
  256 × 256 image keeps 256 KB. `max_side` shrinks the baked image, not the pixels the component
  keeps. Make pixels no larger than they're drawn, and replace or remove images the app no
  longer shows.
- **For one image on many components,** add the pixels once with `bake_texture`, called
  `bakeTexture` in TypeScript. It returns a texture handle that each component can show with
  `set_texture`. It needs the host running. The texture must be given to a component in the
  frame it's made.

The [gallery example](../../examples/gallery) shows a placeholder made from pixels on each tile
until its photo arrives. The Rust version computes the placeholder, and the TypeScript version
draws it on a canvas.

### Cropping

`crop_image` shows only part of the image. A component of a different shape then shows a region
of the image instead of stretching it. The crop is always measured from the whole image, so a
second crop replaces the first.

| Rust | TypeScript | Shows |
|---|---|---|
| `ImageCrop::CenteredSquare` | `{ kind: "centeredSquare" }` | The largest centered square, for a square tile. |
| `ImageCrop::Aspect { ratio, anchor }` | `{ kind: "aspect", ratio, anchor }` | The largest region of that width-to-height ratio, placed by `anchor`. `(0.5, 0.5)` centers it. |
| `ImageCrop::Rect { x, y, width, height }` | `{ kind: "rect", x, y, width, height }` | A region in fractions of the image, from its top-left corner. |
| `ImageCrop::None` | `{ kind: "none" }` | The whole image again. |

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, ImageCrop, Proteus, QuadState};
# let mut app = Proteus::new();
# let photo = app.component(ComponentSpec::new(QuadState::default()));
photo.crop_image(&mut app, ImageCrop::CenteredSquare)?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.cropImage({ kind: "centeredSquare" });
```

`crop_image` returns `false` if the image hasn't been decoded yet. Crop it once it has been. An
image loaded as a texture is decoded at once, so it can be cropped straight away. See
[Load an image](../how-to/loading-images.md).

### Changing and sharing images

`set_image` replaces a component's image with new bytes, and clears the crop.

To show the same image on several components, or to swap images often, add it to the GPU once
as a **texture**, and give the texture to components with `set_texture`. Load a texture from a
file's bytes with `load_texture`, or from raw pixels with `bake_texture`:

#### Rust

```rust,no_run
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TextureRequest};
# let mut app = Proteus::new();
# let (left, right) = (
#     app.component(ComponentSpec::new(QuadState::default())),
#     app.component(ComponentSpec::new(QuadState::default())),
# );
// A PNG or JPEG file, read at run time.
let bytes = std::fs::read("photo.jpg")?;
if let Some(texture) = app.load_texture(&bytes, TextureRequest::default()) {
    left.set_texture(&mut app, texture)?;
    right.set_texture(&mut app, texture)?;
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

#### TypeScript

```ts
const bytes = new Uint8Array(await (await fetch("photo.jpg")).arrayBuffer());
const texture = app.loadTexture(bytes);
if (texture) {
  card.setTexture(texture);
  tile.setTexture(texture);
}
```

Give a new texture to a component in the same frame you load it. A texture that no component
uses can be removed from the GPU from the next frame on, to make room for others.

### Freeing memory

`free_resources` removes a component's text, image and other content, and releases their
textures. The component itself stays, drawn as a plain shape in its color. Use `set_text` or
`set_image` to give it content again.

Releasing a texture doesn't free its GPU memory at once. The space is reused when another
texture needs room.

## Video

Video is **experimental**. Proteus shows video but doesn't play it: you bring your own player,
such as the browser's `<video>` element, `ffmpeg` or a hardware decoder, and give Proteus each
frame it decodes.

1. Create a video with `create_video`. There is one video at a time: creating another replaces
   the first.
2. Show it on components with `show_video`.
3. Upload each frame the player decodes. Proteus doesn't ask for frames: the app hands each one
   over as the player produces it, and components draw the newest frame they were given.
4. Release it with `release` when it's done.

In Rust, upload in the app's `update`, which the host calls every frame. Each time, ask the
player for the frame it has decoded since the last call, and upload it with `upload_frame`. A
player decodes on its own thread, so asking shouldn't wait for it: if there's no new frame yet,
upload nothing, and components keep showing the last one.

#### Rust

```rust
use proteus_runtime::{App, Frame};
use proteus_sdk::{ComponentSpec, QuadState, VideoHandle};

/// A frame the player decoded, as RGBA pixels.
struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

// `Player` stands for your video player, such as one built on `ffmpeg`.
# struct Player;
# impl Player {
#     fn newest_frame(&mut self) -> Option<Decoded> {
#         None
#     }
#     fn finished(&self) -> bool {
#         false
#     }
# }

struct Movie {
    player: Player,
    video: Option<VideoHandle>,
}

impl App for Movie {
    fn setup(&mut self, f: &mut Frame) {
        let screen = f.proteus.component(ComponentSpec::new(QuadState::default()));
        let video = f.proteus.create_video();
        let _ = screen.show_video(f.proteus, &video);
        self.video = Some(video);
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        let Some(video) = self.video else { return };
        // The newest frame decoded since the last call, if there is one.
        if let Some(frame) = self.player.newest_frame() {
            video.upload_frame(f.proteus, frame.width, frame.height, &frame.rgba);
        }
        if self.player.finished() {
            video.release(f.proteus);
            self.video = None;
        }
    }
}
```

In TypeScript, the browser's `<video>` element is the player. Its `requestVideoFrameCallback`
calls a function each time it has a new frame, and `uploadFrom` uploads the frame it's showing:

#### TypeScript

```ts
const video = app.createVideo();
card.showVideo(video);

const element = document.createElement("video");
element.src = "clip.mp4";
element.muted = true;
const upload = () => {
  video.uploadFrom(element);
  element.requestVideoFrameCallback(upload);
};
element.requestVideoFrameCallback(upload);
void element.play();
```

Playback controls, such as pause and seek, are the player's own. If a component also has an
image, `set_video_crossfade` blends between the image and the video, for a video that fades in
over a poster. The [video example](../../examples/video) plays a `<video>` element this way.
