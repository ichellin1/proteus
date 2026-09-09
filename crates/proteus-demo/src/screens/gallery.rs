//! The `Gallery` screen — a 4×3 grid of fetched photos, reached from
//! `Loading` once every tile has an image (`Demo::advance_gallery_fetch`).
//! Clicking "Fetch New Images" goes back through `Loading` for a refetch;
//! clicking a tile opens `enlarged`, an enlarged single-image view
//! (`AppState::GalleryImage`, M12.5 Step 8) — a dedicated coordinator
//! entity, deliberately not one of the 12 `tiles` (see its doc for why).
//!
//! Unlike `screens::video_tiles`' fixed 3-tile row, this grid is fully
//! viewport-computed (`cell_size`/`cell_quad`) — mirrors
//! `proteus-shell-native::gallery_cell_size`/`gallery_cell_quad` exactly.
//! Hover (glow + scale on `tiles`/`fetch_button`; glow only, no scale, on
//! `enlarged` — see `Demo::advance_hovers`' call site for why that one's
//! different) and theme-color/corner-radius wiring both live in `Demo`
//! (`Demo::new`'s `register_hover` calls, `Demo::advance_theme`) — this
//! module only owns the static geometry/spawn shape. `GALLERY_CORNER_
//! RADIUS`'s dark counterpart is a genuine, verified no-op (both `20.0` in
//! source) — wired anyway per this pass's "match actual behavior, not just
//! currently-visible differences" design decision.
//!
//! Fidelity note: an empty tile (before its fetch resolves) is a plain
//! white rounded square with a violet border — the original's own resting
//! appearance too (see that file's `gallery_cell_quad` doc: color is
//! unconditionally white regardless of load state). A fetched photo's own
//! real aspect ratio is essentially never square, so — mirroring the
//! original exactly, not simplifying it away — each tile's baked image
//! gets center-cropped to a centered square in place
//! (`Handle::center_crop_to_square`, driven by
//! `Demo::advance_gallery_tile_crop`) once it lands, so the grid cell shows
//! a crop, never a stretch. The *uncropped* frame is stashed on a separate,
//! hidden `tile_full[idx]` entity first (see that field's doc) — the
//! enlarged view (`Demo::start_gallery_to_image`) reads from there, not
//! from the (by-then-cropped) tile, so it always shows the photo's true,
//! undistorted framing.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Glow, Handle, Proteus, QuadState, Text};

pub const COLS: usize = 4;
pub const ROWS: usize = 3;
pub const TILE_COUNT: usize = COLS * ROWS;

const MARGIN_LEFT: f32 = 40.0;
const MARGIN_RIGHT: f32 = 40.0;
const MARGIN_TOP: f32 = 40.0;
const MARGIN_BOTTOM: f32 = 100.0;
const GAP_PX: f32 = 20.0;
const CELL_SCALE: f32 = 0.9;
const GRID_BOTTOM_MARGIN_PX: f32 = 40.0;
/// Also the theme-blend target in `Demo::advance_theme` — the dark-theme
/// counterpart (`CORNER_RADIUS_DARK`) is a genuine, verified no-op (both
/// `20.0` in source), wired anyway — see the module doc.
pub const CORNER_RADIUS: f32 = 20.0;
/// `proteus-shell-native::GALLERY_CORNER_RADIUS_DARK`.
pub const CORNER_RADIUS_DARK: f32 = 20.0;
const BORDER_WIDTH: f32 = 3.0;

const FETCH_BUTTON_LABEL: &str = "Fetch New Images";
const FETCH_BUTTON_TOP_MARGIN_PX: f32 = 65.0;
const FETCH_BUTTON_FALLBACK_SIZE: Vec2 = Vec2::new(220.0, 46.0);
const FETCH_BUTTON_PADDING_PX: f32 = 15.0;
/// `proteus-shell-native` actually blends `gallery_fetch_button`'s corner
/// radius against `NAV_BUTTON_CORNER_RADIUS`'s own pair, not `GALLERY_
/// CORNER_RADIUS`'s — numerically identical either way (every one of these
/// pairs happens to be `20.0`/`20.0`), but `Demo::advance_theme` uses this
/// pair specifically to match source's own semantic pairing, not just its
/// current numeric output.
pub const FETCH_BUTTON_CORNER_RADIUS: f32 = 20.0;
pub const FETCH_BUTTON_CORNER_RADIUS_DARK: f32 = 20.0;
const LABEL_SIZE_PX: f32 = 24.0;
const LABEL_LETTER_SPACING_PX: f32 = LABEL_SIZE_PX * 0.02;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct Gallery {
    pub tiles: [Handle; TILE_COUNT],
    pub fetch_button: Handle,
    pub fetch_button_label: Handle,
    /// The enlarged single-image view's coordinator entity — see the
    /// module doc and `Demo::start_gallery_to_image`'s doc for why this
    /// can't just be whichever `tiles[idx]` was clicked: a merge's own
    /// destination and one of its own sources can't safely be the same
    /// entity (the setup system hides every source, including — if it were
    /// reused — the destination it's also supposed to be revealing into).
    /// Keeping it separate means the 12 real tiles are never touched at all
    /// for the whole `GalleryImage` visit — they just stay hidden, exactly
    /// like the other 11 always were.
    pub enlarged: Handle,
    /// The hires upgrade's crossfade overlay — kept glued to `enlarged`'s
    /// position/size/scale/corner_radius every frame
    /// (`Demo::advance_gallery_hires_overlay`), fading in only once it has
    /// a real baked image *and* `enlarged` has fully settled (not mid-morph
    /// — see that function's doc). A fixed z (0.6) just ahead of
    /// `enlarged`'s own (0.5) so it draws on top once its alpha ramps up.
    /// Kept as a genuinely separate entity/overlay, not a swap-in-place on
    /// `enlarged` itself, specifically so `enlarged`'s own already-baked
    /// low-res image never has to be removed — removing it a frame before
    /// the hires bake is ready would show nothing at all in between (a
    /// visible flash), exactly the bug this overlay avoids. Not
    /// `Interactable` — display-only, never a click target.
    pub hires_overlay: Handle,
    /// One hidden, never-rendered entity per tile, each holding that
    /// tile's *uncropped* baked image — see `Demo::advance_gallery_tile_crop`'s
    /// doc for the full mechanism. `tiles[idx]`'s own `BakedImage` gets
    /// center-cropped to a square in place once it lands (so the grid cell
    /// shows a crop, not a stretch); `enlarged` needs the original,
    /// undistorted frame when `idx` is clicked, so it's stashed here first,
    /// before the crop happens — mirrors
    /// `proteus-shell-native::gallery_tile_full_baked`, just as an ECS
    /// entity instead of a plain field (`Demo` has no way to hold a bare
    /// `BakedImage` value itself — see `Handle::copy_baked_image_from`'s
    /// doc for why that type only ever flows entity-to-entity here).
    pub tile_full: [Handle; TILE_COUNT],
}

pub fn spawn(app: &mut Proteus, viewport_size: Vec2) -> Gallery {
    let tiles = std::array::from_fn(|i| {
        app.component(
            ComponentSpec::new(cell_quad(i, viewport_size))
                .border(Border::new(BORDER_WIDTH, violet()))
                .glow(Glow {
                    radius: 0.0,
                    color: violet(),
                    intensity: 1.0,
                }),
        )
    });

    let fetch_button = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: FETCH_BUTTON_FALLBACK_SIZE,
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            // Transparent idle fill — border + label are what's visible,
            // same convention as `screens::home`/`screens::nav`'s buttons.
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: FETCH_BUTTON_CORNER_RADIUS,
        })
        .border(Border::new(BORDER_WIDTH, violet()))
        .glow(Glow {
            radius: 0.0,
            color: violet(),
            intensity: 1.0,
        }),
    );
    let fetch_button_label = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.1),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(
            Text::new(FETCH_BUTTON_LABEL, LABEL_SIZE_PX)
                .with_color(violet())
                .with_letter_spacing(LABEL_LETTER_SPACING_PX),
        )
        .non_interactive(),
    );
    fetch_button.add_child(app, fetch_button_label);

    let enlarged = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::ONE,
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
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

    let hires_overlay = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.6),
            size: Vec2::ONE,
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: CORNER_RADIUS,
        })
        .border(Border::new(BORDER_WIDTH, violet()))
        .non_interactive(),
    );

    let tile_full = std::array::from_fn(|_| {
        app.component(ComponentSpec::new(QuadState::default()).non_interactive())
    });

    Gallery {
        tiles,
        fetch_button,
        fetch_button_label,
        enlarged,
        hires_overlay,
        tile_full,
    }
}

/// The largest square cell that fits `COLS`×`ROWS` within `viewport_size`
/// minus margins/gaps on both axes, then scaled down slightly
/// (`CELL_SCALE`) for breathing room. Mirrors
/// `proteus-shell-native::gallery_cell_size` exactly.
fn cell_size(viewport_size: Vec2) -> f32 {
    let usable_w =
        (viewport_size.x - MARGIN_LEFT - MARGIN_RIGHT - (COLS - 1) as f32 * GAP_PX).max(0.0);
    let usable_h =
        (viewport_size.y - MARGIN_TOP - MARGIN_BOTTOM - (ROWS - 1) as f32 * GAP_PX).max(0.0);
    (usable_w / COLS as f32).min(usable_h / ROWS as f32) * CELL_SCALE
}

/// The grid's total content footprint (all cells + internal gaps, no outer
/// margins) — used both to center the grid horizontally and, negated, as
/// half-width for `cell_quad`'s own positioning math.
fn grid_content_size(viewport_size: Vec2) -> Vec2 {
    let cell = cell_size(viewport_size);
    Vec2::new(
        COLS as f32 * cell + (COLS - 1) as f32 * GAP_PX,
        ROWS as f32 * cell + (ROWS - 1) as f32 * GAP_PX,
    )
}

/// `idx` is row-major (`row = idx / COLS`, row 0 = top). Horizontally
/// centered on the viewport; vertically bottom-anchored — the grid's own
/// bottom edge always sits `GRID_BOTTOM_MARGIN_PX` above the viewport's
/// bottom edge, regardless of viewport size or how many rows fit. Mirrors
/// `proteus-shell-native::gallery_cell_quad` (light treatment only).
pub fn cell_quad(idx: usize, viewport_size: Vec2) -> QuadState {
    let cell = cell_size(viewport_size);
    let row = idx / COLS;
    let col = idx % COLS;
    let grid_w = grid_content_size(viewport_size).x;
    let x = -grid_w / 2.0 + cell / 2.0 + col as f32 * (cell + GAP_PX);
    let rows_from_bottom = (ROWS - 1 - row) as f32;
    let grid_bottom_y = -viewport_size.y / 2.0 + GRID_BOTTOM_MARGIN_PX;
    let y = grid_bottom_y + cell / 2.0 + rows_from_bottom * (cell + GAP_PX);
    QuadState {
        position: Vec3::new(x, y, 0.5),
        size: Vec2::splat(cell),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: CORNER_RADIUS,
    }
}

/// All 12 tiles' resting geometry for the current viewport — recomputed
/// whenever the viewport resizes (see `Demo::set_viewport_size`).
pub fn layout(viewport_size: Vec2) -> [QuadState; TILE_COUNT] {
    std::array::from_fn(|i| cell_quad(i, viewport_size))
}

/// The "Fetch New Images" button's resting geometry — width/height sized
/// from the label's actual baked width (`+ 2 * FETCH_BUTTON_PADDING_PX`),
/// horizontally centered, top-anchored `FETCH_BUTTON_TOP_MARGIN_PX` below
/// the viewport's top edge. `None` until the label has baked (text bakes
/// within the first frame or two — see `screens::examples_home::layout`'s
/// identical convention). Mirrors the sizing/positioning math in
/// `proteus-shell-native::layout_gallery_tiles`.
pub fn fetch_button_quad(
    app: &Proteus,
    gallery: &Gallery,
    viewport_size: Vec2,
) -> Option<QuadState> {
    let size = gallery.fetch_button_label.baked_text_size(app)?
        + Vec2::splat(2.0 * FETCH_BUTTON_PADDING_PX);
    let y = viewport_size.y / 2.0 - FETCH_BUTTON_TOP_MARGIN_PX - size.y / 2.0;
    Some(QuadState {
        position: Vec3::new(0.0, y, 0.5),
        size,
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
        corner_radius: FETCH_BUTTON_CORNER_RADIUS,
    })
}

/// `enlarged`'s target geometry — dead center of the viewport, `aspect`
/// (width, height) contain-fit within the grid's own content bounding box
/// (`grid_content_size`) without distorting it, independent of which cell
/// the source tile was in. One `min()` covers portrait (height-constrained),
/// landscape (width-constrained), and square photos alike — no branching.
/// Mirrors `proteus-shell-native::gallery_large_image_quad` (light
/// treatment only).
pub fn large_image_quad(aspect: Vec2, viewport_size: Vec2) -> QuadState {
    let box_size = grid_content_size(viewport_size);
    let scale = (box_size.x / aspect.x).min(box_size.y / aspect.y);
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: aspect * scale,
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::ONE,
        corner_radius: CORNER_RADIUS,
    }
}

/// Tile indices (row-major, matching `MergeLayout::Grid`'s expectation) for
/// the `width`-column-wide slice of the grid starting at `start_col` — used
/// by `Demo::start_gallery_to_home`'s three column-grouped merges (column 0
/// alone → nav button 0, columns 1–2 → nav button 1, column 3 alone → nav
/// button 2), mirroring `proteus-shell-native::start_gallery_to_nav`'s
/// asymmetric 1+2+1 grouping (4 grid columns onto 3 nav buttons).
pub fn column_group_tiles(start_col: usize, width: usize) -> Vec<usize> {
    (0..ROWS)
        .flat_map(move |row| (start_col..start_col + width).map(move |col| row * COLS + col))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Demo::start_gallery_to_home`'s three groups — `(0,1)`, `(1,2)`,
    /// `(3,1)` — must partition all 12 tiles with no overlaps and no gaps,
    /// or some tile would either merge into two nav buttons at once or
    /// never converge anywhere.
    #[test]
    fn start_gallery_to_home_groups_partition_every_tile_exactly_once() {
        let mut covered: Vec<usize> = [(0, 1), (1, 2), (3, 1)]
            .into_iter()
            .flat_map(|(start_col, width)| column_group_tiles(start_col, width))
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, (0..TILE_COUNT).collect::<Vec<_>>());
    }

    /// Row-major order (`MergeLayout::Grid`'s expectation): within one
    /// group, indices must increase strictly — a group spanning columns
    /// 1–2 should yield `[1, 2, 5, 6, 9, 10]`, not row/column-swapped.
    #[test]
    fn column_group_tiles_is_row_major() {
        assert_eq!(column_group_tiles(1, 2), vec![1, 2, 5, 6, 9, 10]);
        assert_eq!(column_group_tiles(0, 1), vec![0, 4, 8]);
        assert_eq!(column_group_tiles(3, 1), vec![3, 7, 11]);
    }

    /// The grid must fit within the viewport (margins/gaps respected on
    /// both axes) at a representative size, and every cell must be a
    /// positive square.
    #[test]
    fn layout_produces_a_grid_that_fits_the_viewport() {
        let viewport = Vec2::new(1280.0, 800.0);
        let states = layout(viewport);
        let cell = states[0].size.x;
        assert!(cell > 0.0);
        for state in &states {
            assert_eq!(
                state.size,
                Vec2::splat(cell),
                "every cell must be a uniform square"
            );
        }
        let content = grid_content_size(viewport);
        assert!(content.x <= viewport.x - MARGIN_LEFT - MARGIN_RIGHT + 0.001);
        assert!(content.y <= viewport.y - MARGIN_TOP - MARGIN_BOTTOM + 0.001);
    }
}
