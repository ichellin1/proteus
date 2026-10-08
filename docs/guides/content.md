# Text, images and video

A component can show a line of text, an image, or video. Proteus draws all three from textures
on the GPU, which the host prepares from what you give it: it renders text, and decodes images,
before it next draws. You give the content; when and how it reaches the GPU is the host's job.

## Text

A component's text is one line, drawn over its fill in the component's center, or wherever its
anchor puts it; see [Anchor](./components.md#anchor).

#### Rust

```rust
# use proteus_sdk::glam::Vec4;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
let title = app.component(
    ComponentSpec::new(QuadState::default()).text(
        Text::new("Proteus", 32.0)
            .with_color(Vec4::new(0.1, 0.1, 0.1, 1.0))
            .with_letter_spacing(1.5),
    ),
);
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const title = app.component({
  geometry: app.get(card)!.geometry,
  text: { content: "Proteus", sizePx: 32, color: colorFrom("#1a1a1a"), letterSpacingPx: 1.5 },
});
```

- **Size** is in pixels. The text isn't wrapped or shrunk to fit the component.
- **Color** defaults to white. Its alpha is multiplied by the component's opacity.
- **Letter spacing** adds space between characters, in pixels; a negative value tightens it.
- **The font** is the app's one font: Inter Bold, built into Proteus, unless the app sets its
  own. A text can't choose a different font. See [Configuration](./configuration.md#text).
- **Text wider than an atlas page**, 2048 pixels by default, is cut off at the page's width,
  with a warning.

To change a component's text, use `set_text`, which replaces it. The host renders the new text
before it next draws.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
# let score = app.component(ComponentSpec::new(QuadState::default()).text(Text::new("0", 24.0)));
score.set_text(&mut app, Text::new("12", 24.0))?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.setText({ content: "12", sizePx: 24 });
```

The text's size isn't known until the host has rendered it. `baked_text_size` returns it from
then on, which is useful for layout that depends on the text's real width, such as a button
sized to its label.

## Images

An image is a PNG or JPEG file's bytes. It fills the component: an image of a different shape
from the component is stretched to fit, unless it is cropped.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Image, Proteus, QuadState};
# let mut app = Proteus::new();
# let bytes: Vec<u8> = Vec::new();
// `bytes` holds a PNG or JPEG file, such as from `std::fs::read`.
let photo = app.component(
    ComponentSpec::new(QuadState::default()).image(Image::new(bytes).with_max_side(800)),
);
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
  is often much larger than it is drawn, and a smaller one uses less GPU memory. Without it, the
  app's default from [Configuration](./configuration.md) applies.
- **An image larger than an atlas page**, 2048 pixels by default, is scaled down to fit it, with
  a warning.
- **A file that can't be decoded** is removed from the component, with a warning.

### Cropping

`crop_image` shows only part of the image, so a component of a different shape shows a region of
it instead of stretching it. The crop is always measured from the whole image, so a second crop
replaces the first.

| Rust | TypeScript | Shows |
|---|---|---|
| `ImageCrop::CenteredSquare` | `{ kind: "centeredSquare" }` | The largest centered square, for a square tile. |
| `ImageCrop::Aspect { ratio, anchor }` | `{ kind: "aspect", ratio, anchor }` | The largest region of that width-to-height ratio, placed by `anchor`: `(0.5, 0.5)` centers it. |
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

`crop_image` returns `false` if the image hasn't been decoded yet; crop it once it has. An image
loaded as a texture is decoded at once, so it can be cropped straight away; see
[Load an image](../how-to/loading-images.md).

### Changing and sharing images

`set_image` replaces a component's image with new bytes, and clears its crop.

To show the same image on several components, or to swap images often, add it to the GPU once
as a **texture**, and give the texture to components with `set_texture`. Load one from a file's
bytes with `load_texture`, or from raw pixels with `bake_texture`:

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, TextureRequest};
# let mut app = Proteus::new();
# let (left, right) = (
#     app.component(ComponentSpec::new(QuadState::default())),
#     app.component(ComponentSpec::new(QuadState::default())),
# );
# let bytes: Vec<u8> = Vec::new();
if let Some(texture) = app.load_texture(&bytes, TextureRequest::default()) {
    left.set_texture(&mut app, texture)?;
    right.set_texture(&mut app, texture)?;
}
# Ok::<(), proteus_sdk::HandleError>(())
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

Releasing a texture doesn't free its GPU memory at once: the space is reused when another
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
