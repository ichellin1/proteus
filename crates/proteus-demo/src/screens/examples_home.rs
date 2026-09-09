//! The Examples category grid — reached from `Home`'s third nav button.
//! Six category buttons in a 3-column × 2-row grid (column-major:
//! `buttons[col*2]` = top, `buttons[col*2+1]` = bottom), matching
//! `proteus-shell-native::EXAMPLE_CATEGORY_TITLES`. All 6 are clickable,
//! matching source — categories 0 (Effects), 1 (Text), 2 (Transforms &
//! Animation), 3 (Stress Tests) have real content; 4–5 (Layout, 3D) land on
//! a "not built yet" placeholder instead (see `screens::example_detail`'s
//! doc).
//!
//! Design-System treatment: same "no fill ever, border/glow only, violet
//! label" call as `screens::home`'s nav buttons — same spawn recipe as
//! `proteus-shell-native`'s own `example_buttons` (border/glow/interactable,
//! identical constants). Hover registration lives in `Demo::new`
//! (`register_hover`), theme-color blend in `Demo::advance_theme`.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Glow, Handle, Proteus, QuadState, Text};

pub const CATEGORY_TITLES: [&str; 6] = [
    "Effects",
    "Text",
    "Transforms & Animation",
    "Stress Tests",
    "Layout",
    "3D",
];

const LABEL_SIZE_PX: f32 = 24.0;
const LABEL_LETTER_SPACING_PX: f32 = LABEL_SIZE_PX * 0.02;
const PADDING_PX: f32 = 15.0;
const FALLBACK_SIZE: Vec2 = Vec2::new(150.0, 46.0);
const COL_GAP_PX: f32 = 40.0;
const ROW_GAP_PX: f32 = 30.0;
/// Also the theme-blend target in `Demo::advance_theme` — the dark-theme
/// counterpart is numerically identical (`NAV_BUTTON_CORNER_RADIUS_DARK`),
/// same as `screens::home::CORNER_RADIUS`'s own doc.
pub const CORNER_RADIUS: f32 = 20.0;
const BORDER_WIDTH: f32 = 3.0;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct ExamplesHome {
    pub buttons: [Handle; 6],
    pub labels: [Handle; 6],
}

pub fn spawn(app: &mut Proteus) -> ExamplesHome {
    let mut buttons = Vec::with_capacity(6);
    let mut labels = Vec::with_capacity(6);
    for title in CATEGORY_TITLES {
        let button = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, 0.5),
                size: FALLBACK_SIZE,
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                // Transparent — border+glow only, Design System spec. See
                // this module's doc.
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
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(
                Text::new(title, LABEL_SIZE_PX)
                    .with_color(violet())
                    .with_letter_spacing(LABEL_LETTER_SPACING_PX),
            )
            // Decorative label, not a click target — see
            // `ComponentSpec::non_interactive`'s doc.
            .non_interactive(),
        );
        button.add_child(app, label);
        buttons.push(button);
        labels.push(label);
    }
    ExamplesHome {
        buttons: buttons.try_into().unwrap(),
        labels: labels.try_into().unwrap(),
    }
}

/// Grid layout — each column shares the wider of its own top/bottom
/// button's baked-label width; all 6 share one row height (the tallest
/// label). Centered on both axes. Mirrors
/// `proteus-shell-native::layout_example_buttons` exactly. `None` until
/// every label has baked (text bakes within the first frame or two, well
/// before anything can click through to trigger this).
pub fn layout(app: &Proteus, examples_home: &ExamplesHome) -> Option<[QuadState; 6]> {
    let mut sizes = [Vec2::ZERO; 6];
    for (size, &label) in sizes.iter_mut().zip(examples_home.labels.iter()) {
        *size = label.baked_text_size(app)? + Vec2::splat(2.0 * PADDING_PX);
    }
    let col_width = |col: usize| sizes[col * 2].x.max(sizes[col * 2 + 1].x);
    let row_height = sizes.iter().map(|s| s.y).fold(0.0, f32::max);

    let total_width = col_width(0) + col_width(1) + col_width(2) + 2.0 * COL_GAP_PX;

    let mut col_x = [0.0; 3];
    let mut x = -total_width / 2.0;
    for (col, slot) in col_x.iter_mut().enumerate() {
        *slot = x + col_width(col) / 2.0;
        x += col_width(col) + COL_GAP_PX;
    }
    // Top row above center, bottom row below, split by half the row gap.
    let row_y = [
        ROW_GAP_PX / 2.0 + row_height / 2.0,
        -(ROW_GAP_PX / 2.0 + row_height / 2.0),
    ];

    Some(std::array::from_fn(|i| {
        let col = i / 2;
        let row = i % 2;
        QuadState {
            position: Vec3::new(col_x[col], row_y[row], 0.5),
            // Both buttons in a column share the column's own width (its
            // wider label's own size) rather than each sizing to its own
            // label — e.g. "3D" matches "Layout"'s width. This was the
            // actual bug behind "button widths don't match" (reported
            // directly): this line read `sizes[i]` (per-button, individual)
            // instead of `col_width(col)` (shared) despite this very doc
            // comment already describing the correct, column-shared
            // behavior.
            size: Vec2::new(col_width(col), row_height),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            corner_radius: CORNER_RADIUS,
        }
    }))
}
