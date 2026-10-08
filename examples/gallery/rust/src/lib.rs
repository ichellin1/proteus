//! An image gallery, the same app as the TypeScript gallery example.
//!
//! A grid of photos downloaded from picsum.photos. Clicking a photo transforms
//! its tile into a large view of it with a transition channel (1→1). Going
//! back splits the large view into a new grid of photos with `split_to`
//! (1→N).
//!
//! Techniques worth copying:
//!   - Callbacks are given the components, `&mut Proteus`, but not the app.
//!     Each pushes an `Event` onto a shared queue, and [`App::update`] acts
//!     on the queue each frame, with the whole app at hand.
//!   - Download with `fetch_async` and collect the results from
//!     `poll_fetches` in `update`. A grid's photos are all decoded and given
//!     to their tiles in the frame the last one arrives: a texture must be
//!     given to a component in the frame it's loaded.
//!   - Create a component that a transition will reveal hidden, with
//!     `visible(false)`. `update` runs before the frame is drawn, and a
//!     transition starts on the next frame, so a visible one would be drawn
//!     once in its final place first.
//!   - Give the large view its image before it opens: the tile's photo,
//!     uncropped. A large view created blank would grow as an empty box.
//!     Cropping changes the component, not the texture, so the tile's
//!     texture shows uncropped on the large view.
//!   - Crossfade to the sharper photo with a second component on top, which
//!     fades in with `animate_to` on its color's alpha, then hands its
//!     texture to the large view.
//!   - Request the small and the sharp photo at the same aspect ratio; see
//!     `fetch_size`.
//!   - The transition channel is owned by the large view, so it is destroyed
//!     with it.
//!   - While the new grid downloads, a loader over the large view shows that
//!     something is happening. Its text is left-aligned with the anchor, so
//!     the words stay still as the arrow after them grows.
//!
//! To keep the example short, the layout is worked out once, when it starts,
//! and doesn't follow the window when it's resized.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use proteus_runtime::{App, FetchId, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    ComponentSpec, Easing, Handle, ImageCrop, Proteus, QuadState, SplitStrategy, StyleOverride,
    Text, TextureHandle, TextureRequest, TransitionConfig,
};

const COLS: usize = 4;
const ROWS: usize = 3;
const TILE: f32 = 110.0;
const GAP: f32 = 14.0;
const TITLE_HEIGHT: f32 = 60.0;

const BACKGROUND: Vec4 = rgb(0xece6f7);
const CARD: Vec4 = rgb(0xffffff);
const TEXT: Vec4 = rgb(0x3a2d52);
const ACCENT: Vec4 = rgb(0x7a5fb0);

const TRANSITION: TransitionConfig = TransitionConfig {
    duration: 0.5,
    delay: 0.0,
    easing: Easing::EaseOutCubic,
};
const CROSSFADE: TransitionConfig = TransitionConfig {
    duration: 0.3,
    delay: 0.0,
    easing: Easing::EaseOutCubic,
};

/// An opaque color from its hex code, such as `0x7a5fb0`.
const fn rgb(hex: u32) -> Vec4 {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    Vec4::new(r, g, b, 1.0)
}

/// A photo on picsum.photos, and its full size in pixels.
#[derive(Clone, Copy)]
struct Photo {
    id: u32,
    width: u32,
    height: u32,
}

const fn photo(id: u32, width: u32, height: u32) -> Photo {
    Photo { id, width, height }
}

/// Enough photos for a full grid with no repeats.
const PHOTOS: [Photo; 16] = [
    photo(12, 2500, 1667),
    photo(18, 2500, 1667),
    photo(54, 3264, 2176),
    photo(66, 3264, 2448),
    photo(108, 2000, 1333),
    photo(114, 3264, 2448),
    photo(132, 1600, 1066),
    photo(162, 1500, 998),
    photo(168, 1920, 1280),
    photo(174, 1600, 589),
    photo(198, 3456, 2304),
    photo(216, 2500, 1667),
    photo(222, 1800, 977),
    photo(228, 4608, 3456),
    photo(282, 5000, 3333),
    photo(294, 3753, 2309),
];

/// The pixel size to request a photo at, with its longer side near `side`.
///
/// picsum.photos crops the photo to exactly the size requested, so the small
/// and the sharp photo must be requested at the same aspect ratio, or they
/// are cropped differently and the photo shifts sideways when the crossfade
/// swaps them. Rounding one side from the other isn't precise enough for
/// some photos, so this tries a few sizes just below `side` and picks the one
/// closest to the photo's own ratio.
fn fetch_size(photo: Photo, side: u32) -> (u32, u32) {
    let ratio = photo.width as f32 / photo.height as f32;
    (side.saturating_sub(6).max(1)..=side)
        .map(|larger| {
            if photo.width >= photo.height {
                (larger, ((larger as f32 / ratio).round() as u32).max(1))
            } else {
                (((larger as f32 * ratio).round() as u32).max(1), larger)
            }
        })
        .min_by(|a, b| {
            let error = |(w, h): &(u32, u32)| (*w as f32 / *h as f32 - ratio).abs();
            error(a).total_cmp(&error(b))
        })
        .unwrap_or((side, side))
}

fn photo_url(photo: Photo, side: u32) -> String {
    let (width, height) = fetch_size(photo, side);
    format!("https://picsum.photos/id/{}/{width}/{height}", photo.id)
}

/// The largest size with the photo's aspect ratio that fits in `max`.
fn fit(photo: Photo, max: Vec2) -> Vec2 {
    let size = Vec2::new(photo.width as f32, photo.height as f32);
    size * (max.x / size.x).min(max.y / size.y)
}

/// What a callback tells [`App::update`].
#[derive(Clone, Copy)]
enum Event {
    TileClicked(Handle),
    BackClicked,
    TransitionDone(Handle),
}

/// The queue the callbacks push onto, shared with the app.
type Events = Rc<RefCell<Vec<Event>>>;

/// A photo in the grid.
struct Tile {
    handle: Handle,
    /// The photo, uncropped. The tile shows it cropped to a square, and the
    /// large view starts from it uncropped.
    texture: Option<TextureHandle>,
    photo: Photo,
}

/// A new grid's photos, downloading.
struct GridDownload {
    photos: Vec<Photo>,
    /// Which photo each download is for.
    fetches: HashMap<FetchId, usize>,
    /// Each photo's file, once it has arrived. `None` if it hasn't, or if its
    /// download failed.
    files: Vec<Option<Arc<[u8]>>>,
    remaining: usize,
}

/// A photo open in the large view.
struct Open {
    view: Handle,
    /// The tile it opened from, until the transition has finished with it.
    tile: Option<Handle>,
    /// Whether the transition into the large view has finished.
    opened: bool,
    /// The sharp photo's download, while it runs.
    sharp_fetch: Option<FetchId>,
    /// The sharp photo's file, from when it arrives until it's crossfaded in.
    sharp_file: Option<Arc<[u8]>>,
    /// The component the sharp photo fades in on, and its texture.
    overlay: Option<(Handle, TextureHandle)>,
    back: Handle,
    caption: Handle,
}

enum Screen {
    /// The first grid's photos are downloading.
    Starting,
    Grid(Vec<Tile>),
    Open(Open),
    /// Back was clicked: the large view shows the loader until the new
    /// grid's photos have arrived, then splits into them.
    Leaving(Leaving),
}

struct Leaving {
    view: Handle,
    loader: Loader,
    /// The new grid's tiles, hidden, once its photos have arrived.
    tiles: Option<Vec<Tile>>,
}

/// "Fetching Images" over the large view, with an arrow that grows in steps,
/// so the wait for the new grid's photos doesn't look like the app has stopped.
struct Loader {
    /// The dark box the text sits on.
    panel: Handle,
    /// The text, a child of `panel`.
    label: Handle,
    /// Seconds since the loader appeared.
    elapsed: f32,
    /// Which of [`LOADER_FRAMES`] is showing.
    frame: usize,
}

/// The loader's text, one after another, each for a quarter of a second.
const LOADER_FRAMES: [&str; 4] = [
    "Fetching Images ->",
    "Fetching Images -->",
    "Fetching Images --->",
    "Fetching Images ---->",
];
const LOADER_FRAME_SECONDS: f32 = 0.25;
const LOADER_SIZE: Vec2 = Vec2::new(250.0, 44.0);
const LOADER_TEXT_SIZE: f32 = 18.0;

/// The gallery app.
pub struct Gallery {
    /// The window's size, in logical pixels, when the app started.
    size: Vec2,
    events: Events,
    screen: Screen,
    download: Option<GridDownload>,
    /// Where in [`PHOTOS`] the next grid starts.
    next_photo: usize,
    /// The large view being split into a grid, until the split ends.
    splitting: Option<Handle>,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            size: Vec2::ZERO,
            events: Events::default(),
            screen: Screen::Starting,
            download: None,
            next_photo: 0,
            splitting: None,
        }
    }
}

/// The engine settings both entry points use.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config
}

impl App for Gallery {
    fn setup(&mut self, f: &mut Frame) {
        self.size = f.viewport.logical_size;
        let title = self.place(
            self.size.x / 2.0,
            TITLE_HEIGHT / 2.0,
            Vec2::new(240.0, 34.0),
        );
        f.proteus.component(
            ComponentSpec::new(QuadState {
                color: BACKGROUND,
                ..title
            })
            .non_interactive()
            .text(Text::new("Gallery", 26.0).with_color(TEXT)),
        );
        self.download_grid(f);
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        for (id, file) in f.poll_fetches() {
            self.fetched(f, id, file);
        }
        let events = std::mem::take(&mut *self.events.borrow_mut());
        for event in events {
            match event {
                Event::TileClicked(tile) => self.open(f, tile),
                Event::BackClicked => self.back(f),
                Event::TransitionDone(handle) => self.transition_done(f, handle),
            }
        }
        self.advance_loader(f, dt);
    }
}

impl Gallery {
    /// Geometry for a box of `size` centered at `(x, y)`, measured from the
    /// window's top-left corner with y down, the way the layout is worked
    /// out. Proteus measures from the center, with y up.
    fn place(&self, x: f32, y: f32, size: Vec2) -> QuadState {
        QuadState {
            position: Vec3::new(x - self.size.x / 2.0, self.size.y / 2.0 - y, 0.0),
            size,
            color: CARD,
            corner_radius: 12.0,
            ..Default::default()
        }
    }

    /// Pushes `event` when `handle` is clicked.
    fn on_click(&self, proteus: &mut Proteus, handle: Handle, event: Event) {
        let events = Rc::clone(&self.events);
        handle.on_click(proteus, move |_| events.borrow_mut().push(event));
    }

    /// Pushes [`Event::TransitionDone`] each time `handle`'s transition ends.
    fn on_transition_done(&self, proteus: &mut Proteus, handle: Handle) {
        let events = Rc::clone(&self.events);
        handle.on_transition_complete(proteus, move |_| {
            events.borrow_mut().push(Event::TransitionDone(handle));
        });
    }

    /// Starts downloading the next grid's photos.
    fn download_grid(&mut self, f: &mut Frame) {
        let photos: Vec<Photo> = (0..COLS * ROWS)
            .map(|i| PHOTOS[(self.next_photo + i) % PHOTOS.len()])
            .collect();
        // Each grid starts five photos on from the last, so it's different.
        self.next_photo = (self.next_photo + 5) % PHOTOS.len();
        // Twice the tile's size, for high-density displays.
        let side = (TILE * 2.0) as u32;
        let fetches = photos
            .iter()
            .enumerate()
            .map(|(i, &photo)| (f.fetch_async(&photo_url(photo, side)), i))
            .collect();
        self.download = Some(GridDownload {
            files: vec![None; photos.len()],
            remaining: photos.len(),
            photos,
            fetches,
        });
    }

    /// Handles a finished download: a grid photo, or the open photo's sharp
    /// version.
    fn fetched(&mut self, f: &mut Frame, id: FetchId, file: Option<Arc<[u8]>>) {
        if let Some(download) = &mut self.download {
            if let Some(&i) = download.fetches.get(&id) {
                download.files[i] = file;
                download.remaining -= 1;
                if download.remaining == 0 {
                    let download = self.download.take().expect("checked above");
                    self.grid_downloaded(f, download);
                }
                return;
            }
        }
        if let Screen::Open(open) = &mut self.screen {
            if open.sharp_fetch == Some(id) {
                open.sharp_fetch = None;
                open.sharp_file = file;
                self.crossfade(f);
            }
        }
    }

    /// Builds the grid once all its photos have arrived. If the large view is
    /// waiting to split into it, the tiles start hidden, and the split starts
    /// once the loader has played through once; see [`Self::advance_loader`].
    fn grid_downloaded(&mut self, f: &mut Frame, download: GridDownload) {
        let leaving = matches!(self.screen, Screen::Leaving(_));
        let width = COLS as f32 * TILE + (COLS - 1) as f32 * GAP;
        let height = ROWS as f32 * TILE + (ROWS - 1) as f32 * GAP;
        let left = (self.size.x - width) / 2.0;
        let top = TITLE_HEIGHT + (self.size.y - TITLE_HEIGHT - height) / 2.0;

        let mut tiles = Vec::new();
        for (i, (photo, file)) in download.photos.into_iter().zip(download.files).enumerate() {
            let x = left + (i % COLS) as f32 * (TILE + GAP) + TILE / 2.0;
            let y = top + (i / COLS) as f32 * (TILE + GAP) + TILE / 2.0;
            let handle = f.proteus.component(
                ComponentSpec::new(self.place(x, y, Vec2::splat(TILE)))
                    .hover(StyleOverride {
                        scale: Some(1.06),
                        ..Default::default()
                    })
                    // A split reveals its targets when it ends.
                    .visible(!leaving),
            );
            // A photo whose download failed leaves its tile a plain card.
            let texture =
                file.and_then(|file| f.proteus.load_texture(&file, TextureRequest::default()));
            if let Some(texture) = texture {
                let _ = handle.set_texture(f.proteus, texture);
                let _ = handle.crop_image(f.proteus, ImageCrop::CenteredSquare);
            }
            self.on_click(f.proteus, handle, Event::TileClicked(handle));
            tiles.push(Tile {
                handle,
                texture,
                photo,
            });
        }

        match &mut self.screen {
            Screen::Leaving(leaving) => leaving.tiles = Some(tiles),
            _ => self.screen = Screen::Grid(tiles),
        }
    }

    /// Plays the loader while the large view waits for the new grid. Once the
    /// grid's photos have arrived and the loader has played through at least
    /// once, removes it and splits the large view into the grid.
    fn advance_loader(&mut self, f: &mut Frame, dt: f32) {
        let Screen::Leaving(leaving) = &mut self.screen else {
            return;
        };
        let loader = &mut leaving.loader;
        loader.elapsed += dt;
        let played_once = loader.elapsed >= LOADER_FRAMES.len() as f32 * LOADER_FRAME_SECONDS;
        if !(played_once && leaving.tiles.is_some()) {
            let frame = (loader.elapsed / LOADER_FRAME_SECONDS) as usize % LOADER_FRAMES.len();
            if frame != loader.frame {
                loader.frame = frame;
                let text = Text::new(LOADER_FRAMES[frame], LOADER_TEXT_SIZE).with_color(CARD);
                let _ = loader.label.set_text(f.proteus, text);
            }
            return;
        }

        let Screen::Leaving(leaving) = std::mem::replace(&mut self.screen, Screen::Starting) else {
            return;
        };
        // Destroying the panel destroys the label, its child, too.
        let _ = leaving.loader.panel.destroy(f.proteus);
        let tiles = leaving.tiles.unwrap_or_default();
        let targets: Vec<Handle> = tiles.iter().map(|tile| tile.handle).collect();
        let strategy = SplitStrategy::Grid {
            cols: COLS,
            rows: ROWS,
        };
        if leaving
            .view
            .split_to(f.proteus, &targets, TRANSITION, strategy)
            .is_ok()
        {
            self.splitting = Some(leaving.view);
        } else {
            // The large view is gone: show the grid without a split.
            for tile in &targets {
                let _ = tile.set_visible(f.proteus, true);
            }
        }
        self.screen = Screen::Grid(tiles);
    }

    /// Shows the loader in the middle of `view`.
    fn show_loader(&self, f: &mut Frame, view: Handle) -> Loader {
        let center = f
            .proteus
            .get(view)
            .map_or(Vec3::ZERO, |view| view.geometry.position);
        let panel = f.proteus.component(
            ComponentSpec::new(QuadState {
                position: center,
                size: LOADER_SIZE,
                color: Vec4::new(0.0, 0.0, 0.0, 0.6),
                corner_radius: LOADER_SIZE.y / 2.0,
                ..Default::default()
            })
            .non_interactive(),
        );
        // The text is left-aligned, so the words stay still as the arrow
        // grows: the label's anchor, which places its text, is its left edge.
        // A child is placed from its parent's anchor, the panel's center.
        let label = f.proteus.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(-LOADER_SIZE.x / 2.0 + 24.0, 0.0, 0.0),
                size: Vec2::new(LOADER_SIZE.x - 48.0, LOADER_SIZE.y),
                anchor: Vec2::new(0.0, 0.5),
                color: Vec4::ZERO,
                ..Default::default()
            })
            .non_interactive()
            .text(Text::new(LOADER_FRAMES[0], LOADER_TEXT_SIZE).with_color(CARD)),
        );
        let _ = panel.add_child(f.proteus, label);
        Loader {
            panel,
            label,
            elapsed: 0.0,
            frame: 0,
        }
    }

    /// Opens the photo of `clicked` in the large view.
    fn open(&mut self, f: &mut Frame, clicked: Handle) {
        let Screen::Grid(tiles) = &mut self.screen else {
            return;
        };
        let Some(i) = tiles.iter().position(|tile| tile.handle == clicked) else {
            return;
        };
        let tile = tiles.swap_remove(i);
        for other in tiles.drain(..) {
            let _ = other.handle.destroy(f.proteus);
        }

        let room = Vec2::new(self.size.x * 0.7, self.size.y - TITLE_HEIGHT - 140.0);
        let center_y = TITLE_HEIGHT + (self.size.y - TITLE_HEIGHT) / 2.0;
        let view = f.proteus.component(
            ComponentSpec::new(QuadState {
                corner_radius: 16.0,
                ..self.place(self.size.x / 2.0, center_y, fit(tile.photo, room))
            })
            // The transition reveals it.
            .visible(false),
        );
        if let Some(texture) = tile.texture {
            let _ = view.set_texture(f.proteus, texture);
        }
        self.on_transition_done(f.proteus, view);
        let channel = f.proteus.transition_channel(Some(view));
        channel.set(f.proteus, view, tile.handle, TRANSITION, false);

        let back = f.proteus.component(
            ComponentSpec::new(QuadState {
                corner_radius: 8.0,
                ..self.place(75.0, 30.0, Vec2::new(140.0, 36.0))
            })
            .hover(StyleOverride {
                color: Some(ACCENT),
                ..Default::default()
            })
            .text(Text::new("‹ Back to grid", 15.0).with_color(TEXT)),
        );
        self.on_click(f.proteus, back, Event::BackClicked);

        let caption = f.proteus.component(
            ComponentSpec::new(QuadState {
                corner_radius: 0.0,
                ..self.place(
                    self.size.x / 2.0,
                    self.size.y - 30.0,
                    Vec2::new(280.0, 26.0),
                )
            })
            .non_interactive()
            .text(Text::new(format!("picsum.photos #{}", tile.photo.id), 14.0).with_color(TEXT)),
        );

        self.screen = Screen::Open(Open {
            view,
            tile: Some(tile.handle),
            opened: false,
            sharp_fetch: Some(f.fetch_async(&photo_url(tile.photo, 900))),
            sharp_file: None,
            overlay: None,
            back,
            caption,
        });
    }

    /// Crossfades to the sharp photo once it has arrived and the large view
    /// has finished opening, so it doesn't fade in over a box still moving.
    fn crossfade(&mut self, f: &mut Frame) {
        let Screen::Open(open) = &mut self.screen else {
            return;
        };
        if !open.opened || open.overlay.is_some() {
            return;
        }
        let Some(file) = open.sharp_file.take() else {
            return;
        };
        let Some(texture) = f.proteus.load_texture(&file, TextureRequest::default()) else {
            return;
        };
        let Some(view) = f.proteus.get(open.view) else {
            return;
        };
        let opaque = view.geometry;
        let clear = QuadState {
            color: opaque.color.with_w(0.0),
            ..opaque.clone()
        };
        let overlay = f
            .proteus
            .component(ComponentSpec::new(clear).non_interactive());
        let _ = overlay.set_texture(f.proteus, texture);
        let _ = overlay.animate_to(f.proteus, opaque, CROSSFADE);
        open.overlay = Some((overlay, texture));
        self.on_transition_done(f.proteus, overlay);
    }

    fn transition_done(&mut self, f: &mut Frame, handle: Handle) {
        if self.splitting == Some(handle) {
            // The split has ended: the large view is no longer needed.
            let _ = handle.destroy(f.proteus);
            self.splitting = None;
            return;
        }
        let Screen::Open(open) = &mut self.screen else {
            return;
        };
        if handle == open.view && !open.opened {
            open.opened = true;
            if let Some(tile) = open.tile.take() {
                let _ = tile.destroy(f.proteus);
            }
            self.crossfade(f);
        } else if let Some((overlay, texture)) = open.overlay {
            if handle == overlay {
                // The sharp photo is fully in: move it to the large view, so a
                // split carries it, and remove the overlay.
                let _ = open.view.set_texture(f.proteus, texture);
                let _ = overlay.destroy(f.proteus);
                open.overlay = None;
            }
        }
    }

    /// Leaves the large view: downloads a new grid, which the large view
    /// splits into when it arrives.
    fn back(&mut self, f: &mut Frame) {
        // Wait until the large view has opened: it can't split while it's
        // still moving.
        if !matches!(&self.screen, Screen::Open(open) if open.opened) {
            return;
        }
        let Screen::Open(open) = std::mem::replace(&mut self.screen, Screen::Starting) else {
            return;
        };
        let _ = open.back.destroy(f.proteus);
        let _ = open.caption.destroy(f.proteus);
        if let Some((overlay, _)) = open.overlay {
            let _ = overlay.destroy(f.proteus);
        }
        if let Some(tile) = open.tile {
            let _ = tile.destroy(f.proteus);
        }
        if let Some(id) = open.sharp_fetch {
            f.cancel_fetch(id);
        }
        let loader = self.show_loader(f, open.view);
        self.screen = Screen::Leaving(Leaving {
            view: open.view,
            loader,
            tiles: None,
        });
        self.download_grid(f);
    }
}

/// Runs the gallery on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // The gallery loads no assets of its own; its photos are downloaded.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Gallery::default(), &canvas_id, config(), services).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // The small and the sharp photo are cropped the same way only if their
    // sizes have the same aspect ratio.
    #[test]
    fn small_and_sharp_sizes_share_a_ratio() {
        for photo in PHOTOS {
            let ratio = |(w, h): (u32, u32)| w as f32 / h as f32;
            let small = ratio(fetch_size(photo, 220));
            let sharp = ratio(fetch_size(photo, 900));
            let real = photo.width as f32 / photo.height as f32;
            assert!(
                (small - real).abs() < 0.003,
                "photo {}: {small} vs {real}",
                photo.id
            );
            assert!(
                (sharp - real).abs() < 0.003,
                "photo {}: {sharp} vs {real}",
                photo.id
            );
        }
    }
}
