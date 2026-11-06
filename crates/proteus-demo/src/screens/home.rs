//! The Home screen: one 4 by 2 grid of the demo's features, under the heading
//! "See what Proteus can do" and a one-line tagline. Each tile leads to a
//! screen: the video tiles, the photo gallery, or an example's panel (see
//! `screens::example_detail`). Every tile takes part in each transition to
//! and from Home, so nothing appears or disappears; the heading and tagline
//! fade, in `Demo::advance_home_titles`.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Glow, Handle, Proteus, QuadState, Text};

/// The tiles' titles, in reading order. The last six are the examples, in
/// `screens::example_detail::CATEGORY_TITLES`' order.
pub const TITLES: [&str; TILE_COUNT] = [
    "Video Demo",
    "Photo Gallery",
    "Effects",
    "Text",
    "Transformations",
    "Layout",
    "Keyboard & Focus",
    "About",
];
/// The tiles before the examples: Video Demo and Photo Gallery.
pub const FIRST_EXAMPLE: usize = 2;
pub const COLS: usize = 4;
pub const ROWS: usize = 2;
pub const TILE_COUNT: usize = COLS * ROWS;

pub const HEADING: &str = "See what Proteus can do";
pub const TAGLINE: &str =
    "UI components that transform into each other, drawn on the GPU, natively and on the web.";

const LABEL_SIZE_PX: f32 = 20.0;
const LABEL_LETTER_SPACING_PX: f32 = LABEL_SIZE_PX * 0.02;
const HEADING_SIZE_PX: f32 = 36.0;
const TAGLINE_SIZE_PX: f32 = 18.0;
/// The widest a tile gets; narrower when the viewport can't fit four.
const TILE_MAX_WIDTH_PX: f32 = 260.0;
const TILE_HEIGHT_PX: f32 = 72.0;
const GAP_PX: f32 = 24.0;
/// The least space kept between the grid and the viewport's sides.
const SIDE_MARGIN_PX: f32 = 40.0;
/// The grid's center, a little below the viewport's, to make room for the
/// heading and tagline above it.
const GRID_CENTER_Y: f32 = -40.0;
/// From the grid's top edge to the tagline's center.
const TAGLINE_GAP_PX: f32 = 40.0;
/// From the tagline's center to the heading's.
const HEADING_GAP_PX: f32 = 48.0;
/// The tiles' corner radius in the light theme, and the theme-blend target
/// in `Demo::advance_theme`.
pub const CORNER_RADIUS: f32 = 20.0;
/// Dark-theme counterpart of [`CORNER_RADIUS`]: twice as round, the original
/// design. More than half the tiles' height, so they're drawn as pills.
pub const CORNER_RADIUS_DARK: f32 = 2.0 * CORNER_RADIUS;
const BORDER_WIDTH: f32 = 3.0;
/// How long the heading and tagline take to fade in or out.
pub const TITLE_FADE_SECS: f32 = 0.3;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct Home {
    /// In reading order, row by row: the order a 4 by 2 grid split or merge
    /// fills.
    pub tiles: [Handle; TILE_COUNT],
    pub labels: [Handle; TILE_COUNT],
    pub heading: Handle,
    pub tagline: Handle,
}

/// A transparent component that only shows `text`.
fn text_holder(app: &mut Proteus, text: Text) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(text)
        .non_interactive(),
    )
}

pub fn spawn(app: &mut Proteus) -> Home {
    let mut tiles = Vec::with_capacity(TILE_COUNT);
    let mut labels = Vec::with_capacity(TILE_COUNT);

    for title in TITLES {
        let tile = app.component(
            ComponentSpec::new(QuadState {
                // z=0.5, not 0.0 — `collect_instances` draws roots in
                // ascending z order and breaks ties by spawn order. Tied
                // with `screens::background`'s z=0.0, that would put this
                // above the background only by accident of spawn sequence;
                // an explicit z says it outright.
                position: Vec3::new(0.0, 0.0, 0.5),
                size: Vec2::new(TILE_MAX_WIDTH_PX, TILE_HEIGHT_PX),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                // Transparent: the tile shows only its border and glow.
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                corner_radius: CORNER_RADIUS,
            })
            .border(Border::new(BORDER_WIDTH, violet()))
            .glow(Glow {
                radius: 0.0,
                color: violet(),
                intensity: 1.0,
            }),
        );

        let label = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, 0.1),
                // Transparent — see `screens::splash::spawn`'s wordmark doc:
                // this quad only hosts the baked text overlay, so an opaque
                // fill here would hide the (opaque white) glyphs under an
                // identical white background.
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(
                Text::new(title, LABEL_SIZE_PX)
                    .with_color(violet())
                    .with_letter_spacing(LABEL_LETTER_SPACING_PX),
            )
            // Decorative label, not a click target — see
            // `ComponentSpec::non_interactive`'s doc: this tile's own click
            // handler would otherwise be shadowed whenever the click lands on
            // the label text itself rather than the surrounding padding.
            .non_interactive(),
        );
        let _ = tile.add_child(app, label);

        tiles.push(tile);
        labels.push(label);
    }

    let heading = text_holder(
        app,
        Text::new(HEADING, HEADING_SIZE_PX).with_color(violet()),
    );
    let tagline = text_holder(
        app,
        Text::new(TAGLINE, TAGLINE_SIZE_PX).with_color(violet()),
    );

    Home {
        tiles: tiles.try_into().unwrap(),
        labels: labels.try_into().unwrap(),
        heading,
        tagline,
    }
}

/// The tiles' size in a viewport of `viewport` size: as wide as
/// [`TILE_MAX_WIDTH_PX`], or narrower so four fit with the side margins.
fn tile_size(viewport: Vec2) -> Vec2 {
    let fit = (viewport.x - 2.0 * SIDE_MARGIN_PX - (COLS as f32 - 1.0) * GAP_PX) / COLS as f32;
    Vec2::new(fit.clamp(1.0, TILE_MAX_WIDTH_PX), TILE_HEIGHT_PX)
}

/// The tiles' resting geometry: a 4 by 2 grid in reading order, every tile
/// the same size, centered horizontally at [`GRID_CENTER_Y`].
pub fn layout(viewport: Vec2) -> [QuadState; TILE_COUNT] {
    let size = tile_size(viewport);
    let step = size + Vec2::splat(GAP_PX);
    std::array::from_fn(|i| {
        let col = (i % COLS) as f32 - (COLS as f32 - 1.0) / 2.0;
        let row = (i / COLS) as f32 - (ROWS as f32 - 1.0) / 2.0;
        QuadState {
            position: Vec3::new(col * step.x, GRID_CENTER_Y - row * step.y, 0.5),
            size,
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: CORNER_RADIUS,
        }
    })
}

/// Where the heading and the tagline go, as `(heading_y, tagline_y)`, both
/// centered horizontally: above the grid's top edge.
pub fn title_positions() -> (f32, f32) {
    let grid_top = GRID_CENTER_Y + (ROWS as f32 * TILE_HEIGHT_PX + GAP_PX) / 2.0;
    let tagline_y = grid_top + TAGLINE_GAP_PX;
    (tagline_y + HEADING_GAP_PX, tagline_y)
}

/// The tiles in columns `start_col` to `start_col + width`, in reading
/// order: the sources of a merge into one video tile (see
/// `Demo::start_home_to_tiles`), or the targets of its split back.
pub fn column_group(start_col: usize, width: usize) -> Vec<usize> {
    (0..ROWS)
        .flat_map(move |row| (start_col..start_col + width).map(move |col| row * COLS + col))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Row-major order, as a `MergeLayout::Grid` fills: columns 1–2 are
    // `[1, 2, 5, 6]`, not row and column swapped.
    #[test]
    fn column_group_is_row_major() {
        assert_eq!(column_group(1, 2), vec![1, 2, 5, 6]);
        assert_eq!(column_group(0, 1), vec![0, 4]);
        assert_eq!(column_group(3, 1), vec![3, 7]);
    }

    // The grid fits the viewport at the native window's size, with its
    // side margins, and each tile is the same size.
    #[test]
    fn layout_fits_four_columns_in_the_viewport() {
        let viewport = Vec2::new(1280.0, 800.0);
        let tiles = layout(viewport);
        for tile in &tiles {
            assert_eq!(tile.size, tiles[0].size);
            let right = tile.position.x + tile.size.x / 2.0;
            assert!(right <= viewport.x / 2.0 - SIDE_MARGIN_PX + 0.01);
        }
        // Reading order: the first row above the second, left to right.
        assert!(tiles[0].position.x < tiles[1].position.x);
        assert!(tiles[0].position.y > tiles[COLS].position.y);
        // A narrow viewport narrows the tiles rather than overflowing.
        let narrow = layout(Vec2::new(800.0, 600.0));
        assert!(narrow[0].size.x < tiles[0].size.x);
        let right = narrow[COLS - 1].position.x + narrow[COLS - 1].size.x / 2.0;
        assert!(right <= 400.0 - SIDE_MARGIN_PX + 0.01);
    }

    // The examples' tiles are named as their panels are.
    #[test]
    fn the_example_tiles_match_the_panels_titles() {
        assert_eq!(
            TITLES[FIRST_EXAMPLE..],
            super::super::example_detail::CATEGORY_TITLES
        );
    }
}
