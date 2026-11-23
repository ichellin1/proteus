# Text, images and video

A component can show text, an image, or video. Proteus draws all three from textures
on the GPU, which the host prepares from what you give it: it renders text, and decodes images,
before it next draws. You give the content; when and how it reaches the GPU is the host's job.

## Text

A component's text is drawn over its fill, in the component's center, or wherever its anchor
puts it; see [Anchor](./components.md#anchor). It's one line, or more where it has a `\n`, unless
it wraps; see [Lines](#lines).

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

- **Size** is in pixels. The text isn't shrunk to fit the component, or wrapped at its width:
  it wraps at a width of its own.
- **Color** defaults to white. Its alpha is multiplied by the component's opacity.
- **Letter spacing** adds space between characters, in pixels; a negative value tightens it.
- **The font** is the app's default font, Inter, built into Proteus, bold unless the text sets
  a weight. A text can name another family the app has loaded, a weight and italic; see
  [Fonts](#fonts).
- **Text wider than an atlas page**, 2048 pixels by default, is cut off at the page's width,
  with a warning.

### Lines

A text can wrap at a width, align its lines within it, space them, and stop after a number of
lines.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text, TextAlign};
# let mut app = Proteus::new();
let summary = app.component(
    ComponentSpec::new(QuadState::default()).text(
        Text::new("Proteus draws every component as a textured, rounded rectangle.", 16.0)
            .with_wrap_width(280.0)
            .with_align(TextAlign::Center)
            .with_line_height(1.4)
            .with_max_lines(2)
            .with_ellipsis(" more"),
    ),
);
```

#### TypeScript

```ts
const summary = app.component({
  geometry: app.get(card)!.geometry,
  text: {
    content: "Proteus draws every component as a textured, rounded rectangle.",
    sizePx: 16,
    wrapWidth: 280,
    align: "center",
    lineHeight: 1.4,
    maxLines: 2,
    overflow: { ellipsis: " more" },
  },
});
```

- **The wrap width** is in logical pixels, and is also the width of the text's box. Without one,
  lines break only at `\n`, and the box is as wide as the widest line. A word wider than the
  wrap width breaks between characters. Lines break at the same words on every display.
- **Alignment** places each line at the start, center or end of the box. The anchor places the
  box on the component.
- **Line height** is the distance from one baseline to the next, as a multiple of the size, as
  CSS's unitless `line-height` is. Without one, it's the font's own line spacing.
- **The maximum number of lines** ends text that runs past it in an ellipsis, "…" by default, or
  clips it, dropping the lines past the last with no mark: `TextOverflow::Clip` in Rust,
  `overflow: "clip"` in TypeScript.

An ellipsis is laid out with the text, so it wraps as words do, and the text keeps as much of
itself as fits with the ellipsis after it, cut between whole characters. An ellipsis wider than
a line, such as " read the whole article" on a narrow card, takes lines of its own, and the
text gives them up. One too long to fit in the maximum number of lines on its own is clipped,
with a warning suggesting more lines, a wider wrap width or a shorter ellipsis.

The [essays example](../../examples/essays) shows each of these once, in both languages.

### Fonts

An app loads a font from its file's bytes, and texts name its family to be drawn in it. There's
no name to give when loading: the file is the font, and `load_font` returns the family name
written inside it. Loading a second file of the same family, such as its italic, adds it to the
family.

#### Rust

```rust,no_run
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
// The font is its file: here, Lora, a TTF file read at run time.
let bytes = std::fs::read("fonts/Lora-Variable.ttf")?;
// Or built into the program, which works on the web too:
// let bytes = include_bytes!("fonts/Lora-Variable.ttf");
match app.load_font(bytes) {
    // `family` is "Lora", the name inside the file.
    Ok(family) => {
        app.component(ComponentSpec::new(QuadState::default()).text(
            Text::new("Chapter one", 28.0)
                .with_family(family)
                .with_weight(600)
                .italic(),
        ));
    }
    Err(e) => eprintln!("can't read the font: {e}"),
}
# Ok::<(), std::io::Error>(())
```

#### TypeScript

```ts
const bytes = new Uint8Array(await (await fetch("fonts/Lora.ttf")).arrayBuffer());
const family = app.loadFont(bytes);
app.component({
  geometry: app.get(card)!.geometry,
  text: { content: "Chapter one", sizePx: 28, family, weight: 600, italic: true },
});
```

- **The family** is the name inside the font's file, which `load_font` returns. Without one, a
  text is in the default font.
- **The weight** runs from 100 to 900: 400 is regular and 700 bold. A family without the weight
  uses its nearest, as in CSS, and one with no bold face is thickened. Without one, a text is in
  the font's own weight, or bold in the built-in Inter.
- **Italic** uses the family's italic face, and slants the upright one if it has none.

A font that's downloaded arrives after the app starts. Proteus waits for it if it's told the
family is coming, so its texts aren't drawn in one font and then another:

#### Rust

Tell Proteus the family is coming, and start the download, in `setup`. Load the font when it
arrives:

```rust
use proteus_runtime::{App, FetchId, Frame};
use proteus_sdk::WhileLoading;

struct Reader {
    lora: Option<FetchId>,
}

impl App for Reader {
    fn setup(&mut self, f: &mut Frame) {
        // The family is named here, before its file arrives, so its texts wait for it.
        f.proteus.expect_font("Lora", WhileLoading::Hide);
        self.lora = Some(f.fetch_async("https://example.com/fonts/Lora-Variable.ttf"));
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        for (id, bytes) in f.poll_fetches() {
            if Some(id) != self.lora {
                continue;
            }
            // `None` means the download failed; the host logs why.
            let loaded = bytes.map(|bytes| f.proteus.load_font(bytes));
            if !matches!(loaded, Some(Ok(_))) {
                // Its texts stop waiting, and are drawn in the default font.
                f.proteus.font_failed("Lora");
            }
        }
    }
}
```

#### TypeScript

```ts
app.loadFont(fetch("fonts/Lora.ttf").then((r) => r.arrayBuffer()), { family: "Lora" });
```

| The family is | Its texts are drawn |
|---|---|
| Loaded | in it |
| Coming, waiting: `WhileLoading::Hide`, `whileLoading: "hide"`, the default | not at all, until it loads or fails |
| Coming, meanwhile: `WhileLoading::DefaultFont`, `whileLoading: "defaultFont"` | in the default font, then again in their own once it loads |
| Failed, or never loaded or expected | in the default font, with a warning |

A family that's coming but never loads or fails keeps its texts hidden, as an image that never
loads is never drawn. The [essays example](../../examples/essays) loads Lora this way in
TypeScript.

### Fallback

A font has the characters its designers drew, and no more: Inter has Latin, Greek and Cyrillic,
but not Hindi's Devanagari, Chinese or emoji. A character a text's font lacks is looked for in
the app's fallback fonts, in order, then in the default font. One none of them has is drawn as
the font's missing-glyph box, a rectangle, with a warning naming it.

#### Rust

```rust,no_run
# use proteus_sdk::Proteus;
# let mut app = Proteus::new();
// Noto Serif Devanagari, a TTF file read at run time.
let devanagari = std::fs::read("fonts/NotoSerifDevanagari-Variable.ttf")?;
// The family name inside the file is the one to list.
if let Ok(family) = app.load_font(devanagari) {
    app.set_fallback_fonts([family]);
}
# Ok::<(), std::io::Error>(())
```

#### TypeScript

```ts
app.setFallbackFonts(["Noto Serif Devanagari"]);
```

- **There are none by default.** Fallback is the app's choice: Proteus never uses the system's
  fonts, so text is drawn the same everywhere, and nothing is fetched that the app didn't ask
  for.
- **List only what the app's text needs.** Each fallback is a font the app ships or downloads,
  and keeps in memory: a font for Chinese, Japanese or Korean is several megabytes. A long list
  makes it harder to know which font draws a given character, and a character the app didn't
  plan for can arrive in a font that doesn't match the rest. In almost every app, fonts should
  be deterministic: choose them for the text the app shows, and know what each covers.
- **The list is the app's,** for every text, tried in the order given; calling it again replaces
  it.
- **A fallback that's still loading** is treated as a text's own family is: if it's expected
  with `WhileLoading::Hide`, texts with characters no loaded font has wait for it; otherwise
  they're drawn with boxes, and again once it loads.
- **Color emoji** isn't drawn yet: emoji fonts store their glyphs in color, and text is drawn in
  one color. A black-and-white emoji font, such as Noto Emoji, works as a fallback.

The [essays example](../../examples/essays) draws an essay in Hindi with a fallback font.

### Changing text

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
sized to its label. For wrapped text, it's the wrap width across, and the lines' height down.

## Images

An image is a PNG or JPEG file's bytes. It fills the component: an image of a different shape
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
