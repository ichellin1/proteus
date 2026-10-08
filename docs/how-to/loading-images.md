# Load an image

This page shows an image in a square tile, cropped to fit, from a file that comes with the app
and from a URL. For what images can do, see [Text, images and video](../guides/content.md#images).

Both ways load the image as a **texture**, which is decoded and on the GPU as soon as the call
returns, so it can be cropped at once. An image given with `image` in the spec, or `set_image`,
is decoded later, and can't be cropped until it has been.

## From a file that comes with the app

In Rust, `Frame::load_texture` loads an asset by its key; see [Assets](../guides/hosts.md#assets).
On the web, the asset must be in the list the web host downloads before the app starts.

#### Rust

```rust
use proteus_runtime::{App, Frame};
use proteus_sdk::glam::Vec2;
use proteus_sdk::{ComponentSpec, ImageCrop, QuadState, TextureRequest};

struct Gallery;

impl App for Gallery {
    fn setup(&mut self, f: &mut Frame) {
        let tile = f.proteus.component(ComponentSpec::new(QuadState {
            size: Vec2::new(200.0, 200.0),
            ..Default::default()
        }));
        // A 200-pixel tile needs no more than 400 pixels, for high-density displays.
        let request = TextureRequest {
            max_side: Some(400),
            ..Default::default()
        };
        let texture = f.load_texture("photos/beach.jpg", request);
        if tile.set_texture(f.proteus, texture) == Ok(true) {
            let _ = tile.crop_image(f.proteus, ImageCrop::CenteredSquare);
        }
    }
}
```

`set_texture` returns `Ok(false)` if the asset wasn't found or couldn't be decoded; the host
logs why.

## From a URL

#### Rust

Start the download in `setup`, and load the texture when it arrives:

```rust
use proteus_runtime::{App, FetchId, Frame};
use proteus_sdk::{ComponentSpec, Handle, ImageCrop, QuadState, TextureRequest};

struct Gallery {
    tile: Option<Handle>,
    download: Option<FetchId>,
}

impl App for Gallery {
    fn setup(&mut self, f: &mut Frame) {
        self.tile = Some(f.proteus.component(ComponentSpec::new(QuadState::default())));
        self.download = Some(f.fetch_async("https://example.com/beach.jpg"));
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        for (id, bytes) in f.poll_fetches() {
            if Some(id) != self.download {
                continue;
            }
            // `None` means the download failed; the host logs why.
            let (Some(tile), Some(bytes)) = (self.tile, bytes) else {
                continue;
            };
            let request = TextureRequest {
                max_side: Some(400),
                ..Default::default()
            };
            if let Some(texture) = f.proteus.load_texture(&bytes, request) {
                let _ = tile.set_texture(f.proteus, texture);
                let _ = tile.crop_image(f.proteus, ImageCrop::CenteredSquare);
            }
        }
    }
}
```

#### TypeScript

The same steps work for a file that comes with the app: fetch it by its path on the server.

```ts
const response = await fetch("https://example.com/beach.jpg");
if (response.ok) {
  const bytes = new Uint8Array(await response.arrayBuffer());
  const texture = app.loadTexture(bytes, { maxSide: 400 });
  if (texture) {
    tile.setTexture(texture);
    tile.cropImage({ kind: "centeredSquare" });
  }
}
```

`loadTexture` returns `undefined` if the bytes aren't a PNG or JPEG it can decode.

## While it loads

Until the image arrives, the tile is drawn as a plain shape in its color. The color also tints
the image, so to show a gray placeholder, give the tile a gray color, and set it to white when
the image arrives. Setting it with `animate_to` fades the image in from gray.

## Many images

- **Set `max_side`** to about twice the size the image is drawn, for high-density displays. A
  photo is often many times larger than it is drawn, and a smaller texture uses less GPU memory
  and leaves room in the atlas for more images.
- **Decoding happens during the frame,** and the frame isn't drawn until every image given to
  it has been decoded. Loading many large images in one frame makes that frame late, which shows
  as a stutter. Load a few each frame instead; see below.
- **Show the same image on several components** by giving each the same texture; it's stored on
  the GPU once.

The [gallery example](../../examples/gallery) loads a grid of downloaded photos, and gives a
large view a small image to show until its large one arrives.

## Spreading loads over frames

To load many images without a stutter, put them in a queue, and load a few from it in each
call to `update`, which the host makes every frame. Here 50 tiles in a grid get their images two
a frame, so all 50 have arrived within half a second:

#### Rust

```rust
use proteus_runtime::{App, Frame};
use proteus_sdk::glam::{Vec2, Vec3};
use proteus_sdk::{ComponentSpec, Handle, ImageCrop, QuadState, TextureRequest};
use std::collections::VecDeque;

/// How many images to load each frame.
const PER_FRAME: usize = 2;

struct Gallery {
    /// Tiles waiting for their image, and the image's asset key.
    waiting: VecDeque<(Handle, String)>,
}

impl App for Gallery {
    fn setup(&mut self, f: &mut Frame) {
        // Ten columns and five rows of 100-pixel tiles.
        for i in 0..50 {
            let tile = f.proteus.component(ComponentSpec::new(QuadState {
                position: Vec3::new(
                    -450.0 + (i % 10) as f32 * 100.0,
                    200.0 - (i / 10) as f32 * 100.0,
                    0.0,
                ),
                size: Vec2::new(96.0, 96.0),
                ..Default::default()
            }));
            self.waiting.push_back((tile, format!("photos/{i}.jpg")));
        }
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        let request = TextureRequest {
            max_side: Some(200),
            ..Default::default()
        };
        for _ in 0..PER_FRAME {
            let Some((tile, key)) = self.waiting.pop_front() else {
                break;
            };
            let texture = f.load_texture(&key, request);
            if tile.set_texture(f.proteus, texture) == Ok(true) {
                let _ = tile.crop_image(f.proteus, ImageCrop::CenteredSquare);
            }
        }
    }
}
```

For images downloaded with `fetch_async`, add each one to the queue as it arrives from
`poll_fetches`, and load from the queue in the same way.

#### TypeScript

In TypeScript, the downloads run in parallel, and each joins the queue when it arrives. `mount`'s
`update` function loads two from the queue each frame:

```ts
import { colorFrom, mount, type Handle, type ProteusApp } from "proteus-sdk";

/** How many images to load each frame. */
const PER_FRAME = 2;

/** Downloaded images waiting to be loaded, oldest first. */
const waiting: { tile: Handle; bytes: Uint8Array }[] = [];
let proteus: ProteusApp | undefined;

await mount("canvas", {
  setup(app) {
    proteus = app;
    // Ten columns and five rows of 100-pixel tiles.
    for (let i = 0; i < 50; i++) {
      const tile = app.component({
        geometry: {
          position: { x: -450 + (i % 10) * 100, y: 200 - Math.floor(i / 10) * 100, z: 0 },
          size: { width: 96, height: 96 },
          rotation: 0,
          scale: 1,
          anchor: { x: 0.5, y: 0.5 },
          color: colorFrom("#ffffff"),
          cornerRadius: 0,
        },
      });
      void fetch(`photos/${i}.jpg`).then(async (response) => {
        if (response.ok) {
          waiting.push({ tile, bytes: new Uint8Array(await response.arrayBuffer()) });
        }
      });
    }
  },
  update() {
    for (const { tile, bytes } of waiting.splice(0, PER_FRAME)) {
      const texture = proteus?.loadTexture(bytes, { maxSide: 200 });
      if (texture) {
        tile.setTexture(texture);
        tile.cropImage({ kind: "centeredSquare" });
      }
    }
  },
});
```

How many to load each frame depends on the images' size. Two large photos a frame is a safe
start; small thumbnails can go ten or more at a time. If the tiles fill in too slowly, raise
it; if scrolling or transitions stutter while they load, lower it.
