//! The Home screen — the demo's navigation hub. Three nav buttons, laid out
//! as one horizontal row, each leading to a further screen (`VideoTiles`,
//! `Loading`/`Gallery`, `ExamplesHome`).
//!
//! Design-System treatment: fully transparent fill (border+glow only —
//! `Demo::start_screen_to_home` reasserts this every Home entry, matching
//! the original's own "no fill ever" convention), violet border/glow/label,
//! sized from each label's own baked width (`+2×PADDING_PX`) rather than a
//! fixed size. Mirrors `proteus-shell-native::layout_nav_buttons` exactly.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Glow, Handle, Proteus, QuadState, Text};

pub const TITLES: [&str; 3] = ["Video Demo", "Photo Gallery", "Examples & Tests"];

const LABEL_SIZE_PX: f32 = 24.0;
const LABEL_LETTER_SPACING_PX: f32 = LABEL_SIZE_PX * 0.02;
const PADDING_PX: f32 = 15.0;
const GAP_PX: f32 = 50.0;
/// Also the theme-blend target in `Demo::advance_theme` — the dark-theme
/// counterpart is numerically identical (`NAV_BUTTON_CORNER_RADIUS_DARK`),
/// so this is the one value used for both.
pub const CORNER_RADIUS: f32 = 20.0;
const BORDER_WIDTH: f32 = 3.0;
const FALLBACK_SIZE: Vec2 = Vec2::new(150.0, 46.0);

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

pub struct Home {
    pub nav_buttons: [Handle; 3],
    pub nav_labels: [Handle; 3],
}

pub fn spawn(app: &mut Proteus) -> Home {
    let mut nav_buttons = Vec::with_capacity(3);
    let mut nav_labels = Vec::with_capacity(3);

    for title in TITLES {
        let button = app.component(
            ComponentSpec::new(QuadState {
                // z=0.5, not 0.0 — `collect_instances` draws roots in
                // ascending z order; tied with `screens::background`'s
                // z=0.0, draw order falls back to ECS iteration order,
                // which isn't guaranteed to put this above the background.
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

        let text = app.component(
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
            // `ComponentSpec::non_interactive`'s doc: this button's own click
            // handler would otherwise be shadowed whenever the click lands on
            // the label text itself rather than the surrounding padding.
            .non_interactive(),
        );
        button.add_child(app, text);

        nav_buttons.push(button);
        nav_labels.push(text);
    }

    Home {
        nav_buttons: nav_buttons.try_into().unwrap(),
        nav_labels: nav_labels.try_into().unwrap(),
    }
}

/// One horizontal row, centered on both axes, `GAP_PX` between buttons —
/// each sized from its own baked label (`+2×PADDING_PX`), or
/// `FALLBACK_SIZE` outright (not "baked size + padding" — the flat
/// constant, unpadded, same as the original) for whichever label hasn't
/// baked yet. Always returns a full result — never gated on "every label
/// baked," since even an all-fallback row still spreads its 3 (identically
/// sized, in that case) buttons out via the same cumulative-width math, not
/// stacked on top of each other. Call once, right before
/// `Handle::split_to`, same as `Demo::start_examples_to_detail`'s own
/// "compute the real target geometry immediately before the transition
/// starts" ordering — *not* on some earlier recurring gate, which is the
/// bug an earlier version of this function had (see the M12.5.5 plan's own
/// notes on that regression). Mirrors
/// `proteus-shell-native::layout_nav_buttons` exactly, including its
/// per-label (not all-or-nothing) fallback.
pub fn layout(app: &Proteus, home: &Home) -> [QuadState; 3] {
    let sizes: [Vec2; 3] = std::array::from_fn(|i| {
        home.nav_labels[i]
            .baked_text_size(app)
            .map(|size| size + Vec2::splat(2.0 * PADDING_PX))
            .unwrap_or(FALLBACK_SIZE)
    });

    let total_width: f32 = sizes.iter().map(|s| s.x).sum::<f32>() + 2.0 * GAP_PX;
    let mut x = -total_width / 2.0;
    let mut xs = [0.0; 3];
    for (slot, size) in xs.iter_mut().zip(sizes.iter()) {
        *slot = x + size.x / 2.0;
        x += size.x + GAP_PX;
    }

    std::array::from_fn(|i| QuadState {
        position: Vec3::new(xs[i], 0.0, 0.5),
        size: sizes[i],
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
        corner_radius: CORNER_RADIUS,
    })
}
