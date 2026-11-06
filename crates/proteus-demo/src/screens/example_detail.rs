//! `ExampleDetail` — reached from `screens::home`'s grid. One shared panel
//! entity, holding one of 6 categories' content at a time: Effects, Text,
//! Transformations (static labelled rows demonstrating
//! `DropShadow`/`Glow`/`Border`/`Opacity`, `Text` styling, and
//! rotation/scale/continuous animation), Layout (coming in v0.6.0, which
//! `layout_message` says), Keyboard & Focus (a virtual keyboard whose
//! keycaps flash as their keys are pressed, which the arrows and Tab move
//! round — see `screens::keyboard` — and what the keys do below it),
//! and About (what Proteus is, for a visitor).
//!
//! ## Standalone content, not `ChildOf` the panel
//!
//! Content is spawned as ordinary standalone entities, positioned once in
//! *absolute* world coordinates from the panel's known target rect (the
//! panel's own `QuadState` never changes once a merge starts — it's the
//! fixed destination the sources converge onto), and simply hidden/revealed
//! by `Demo`'s own timer-based reveal (mirrors `screens::home`'s labels), so
//! the merge's bake of the panel never includes any category's rows.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, DropShadow, Glow, Handle, Proteus, QuadState, Text};

use super::keyboard::{self, Keyboard};

/// The categories' titles, in order: also the last six of `screens::home`'s
/// tiles.
pub const CATEGORY_TITLES: [&str; 6] = [
    "Effects",
    "Text",
    "Transformations",
    "Layout",
    "Keyboard & Focus",
    "About",
];
/// The Transformations category, whose box animates continuously.
pub const TRANSFORMS: usize = 2;
/// The Keyboard & Focus category, with its virtual keyboard.
pub const KEYS: usize = 4;

pub(crate) const PANEL_PADDING_PX: f32 = 24.0;
const HEADING_PANEL_GAP_PX: f32 = 20.0;
/// Local Z for all row content — above the panel's own 0.5 so it renders on
/// top of the panel; headings use 0.51 for the same reason.
pub(crate) const CONTENT_Z: f32 = 0.51;

/// `panel`'s theme-blend target in `Demo::advance_theme`, toward
/// [`CORNER_RADIUS_DARK`].
pub const CORNER_RADIUS: f32 = 12.0;
/// Dark-theme counterpart of [`CORNER_RADIUS`]: twice as round, as the
/// buttons and tiles are.
pub const CORNER_RADIUS_DARK: f32 = 2.0 * CORNER_RADIUS;
const BORDER_WIDTH: f32 = 3.0;
/// Neutral grey (not violet-tinted like everything else in this demo) —
/// deliberately opaque and desaturated, so the effect/text/transform
/// examples sitting on top of it (many close to white) read clearly against
/// it instead of blending into the app's own light background. Unlike every
/// other themed color in this crate, this one does *not* lerp to a darker
/// variant in dark mode (`Demo::advance_theme` never touches `panel`'s own
/// `color`, only its `Border`/corner radius) — a darkened panel read as
/// near-black, at odds with the rest of the demo's dark-mode surfaces; this
/// backdrop stays the same light grey in both themes.
const LIGHT_GREY: Vec4 = Vec4::new(0.82, 0.82, 0.83, 1.0);
/// A non-brand accent color, used only for the "different color" examples
/// in the Glow/Border rows and the Text screen's Color row — everything
/// else in this demo is violet.
const ACCENT: Vec4 = Vec4::new(0.95, 0.55, 0.25, 1.0);
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// Vertical space reserved at the top of the viewport so the panel never
/// overlaps `screens::nav`'s icons, or the heading above it.
const TOP_CLEARANCE_PX: f32 = 110.0;
/// Fixed content height per category, in `CATEGORY_TITLES`' order.
const CONTENT_HEIGHT: [f32; 6] = [
    3.0 * 90.0 + 64.0 + 18.0 + 18.0, // Effects = 370.0
    2.0 * 90.0 + 56.0,               // Text = 236.0
    2.0 * 120.0 + 64.0 + 2.0 * 20.0, // Transforms = 344.0
    100.0,                           // Layout: one line
    keyboard::SIZE.y + KEYS_GAP_PX + KEYS_LINES.len() as f32 * LINE_HEIGHT_PX, // Keyboard & Focus
    ABOUT_LINES.len() as f32 * LINE_HEIGHT_PX, // About
];

/// The height of a line of 16 px text in the Keyboard & Focus and About
/// panels.
const LINE_HEIGHT_PX: f32 = 28.0;
/// The space between the keyboard and the lines below it.
const KEYS_GAP_PX: f32 = 24.0;
const KEYS_LINES: [&str; 4] = [
    "The arrow keys and Tab move focus.",
    "Enter or Space chooses the focused key.",
    "Caps Lock switches the keys to their shifted values.",
    "Escape or Backspace goes back.",
];
const ABOUT_LINES: [&str; 5] = [
    "Proteus is a rendering framework where UI components are metamorphic.",
    "They don't navigate to new screens: they transform into them.",
    "A button can become a list, and a list item can become a video player.",
    "It draws with the GPU, natively on macOS, Windows and Linux, and in browsers.",
    "Apps are written in Rust or TypeScript.",
];

struct EffectsContent {
    row_labels: [Handle; 4],
    row_boxes: [Vec<Handle>; 4],
    opacity_item_labels: [Handle; 5],
}

struct TextContent {
    row_labels: [Handle; 3],
    row_items: [Vec<Handle>; 3],
}

struct TransformsContent {
    row_labels: [Handle; 3],
    row_boxes: [Vec<Handle>; 3],
    item_labels: [Vec<Handle>; 3],
}

/// The Keyboard & Focus panel's content.
pub struct KeysContent {
    /// The virtual keyboard. It keeps the light theme's colors in both
    /// themes, as the rest of the panel's content does: the dark theme's
    /// lighter violet is faint on the panel's light grey.
    pub keyboard: Keyboard,
    lines: [Handle; 4],
}

struct AboutContent {
    lines: [Handle; 5],
}

pub struct ExampleDetail {
    pub panel: Handle,
    pub transforms_continuous_box: Handle,
    /// The 6 category titles — `pub` so `Demo::advance_theme` can blend
    /// their color; unlike each category's own row labels/content, these
    /// alone get the live theme lerp — see `Demo::advance_theme`'s doc for
    /// why the two aren't treated the same.
    pub headings: [Handle; 6],
    /// Layout's one line: the layout system is coming in v0.6.0.
    pub layout_message: Handle,
    pub keys: KeysContent,
    effects: EffectsContent,
    text: TextContent,
    transforms: TransformsContent,
    about: AboutContent,
}

impl ExampleDetail {
    /// Every entity belonging to category `idx` (0–5) — used to reveal them
    /// together once their merge transition completes, and to hide them
    /// together on the way out. See the module doc for why this is a plain
    /// `Visibility` toggle rather than a `ChildOf` cascade; the Keyboard &
    /// Focus buttons are the one exception, children of their focus group,
    /// which shows and hides them with it.
    pub fn content_handles(&self, idx: usize) -> Vec<Handle> {
        let mut handles = vec![self.headings[idx]];
        match idx {
            0 => {
                handles.extend(self.effects.row_labels);
                handles.extend(self.effects.row_boxes.iter().flatten().copied());
                handles.extend(self.effects.opacity_item_labels);
            }
            1 => {
                handles.extend(self.text.row_labels);
                handles.extend(self.text.row_items.iter().flatten().copied());
            }
            2 => {
                handles.extend(self.transforms.row_labels);
                handles.extend(self.transforms.row_boxes.iter().flatten().copied());
                handles.extend(self.transforms.item_labels.iter().flatten().copied());
            }
            3 => handles.push(self.layout_message),
            KEYS => {
                handles.push(self.keys.keyboard.board);
                handles.extend(self.keys.lines);
            }
            5 => handles.extend(self.about.lines),
            _ => {}
        }
        handles
    }
}

fn heading(app: &mut Proteus, text: &str) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(text, 28.0).with_color(violet()))
        // Decorative, not a click target — see
        // `ComponentSpec::non_interactive`'s doc.
        .non_interactive(),
    )
}

fn row_label(app: &mut Proteus, text: &str, size_px: f32) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(text, size_px).with_color(violet()))
        .non_interactive(),
    )
}

fn effect_box(app: &mut Proteus, size: f32) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            size: Vec2::splat(size),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: 10.0,
        })
        .non_interactive(),
    )
}

pub fn spawn(app: &mut Proteus) -> ExampleDetail {
    let panel = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(400.0, 300.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            // Flat opaque light grey, not this crate's usual transparent
            // fill — see `LIGHT_GREY`'s own doc for why. No `Glow` — this
            // is a backdrop, not a click target.
            color: LIGHT_GREY,
            corner_radius: CORNER_RADIUS,
        })
        .border(Border::new(BORDER_WIDTH, violet()))
        // Backdrop, not a click target — see
        // `ComponentSpec::non_interactive`'s doc.
        .non_interactive(),
    );

    let headings = CATEGORY_TITLES.map(|title| heading(app, title));

    // --- Layout (category 3) — coming in v0.6.0 ---
    let layout_message = row_label(app, "The layout system is coming in v0.6.0.", 20.0);

    // --- Effects (category 0) ---
    let drop_shadow_label = row_label(app, "Drop Shadow", 16.0);
    let drop_shadow_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(2.0, -2.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.5),
                softness: 4.0,
                spread: 0.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(8.0, -8.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.8),
                softness: 2.0,
                spread: 2.0,
            });
            b
        },
        {
            let b = effect_box(app, 61.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(6.0, -6.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.6),
                softness: 6.0,
                spread: 10.0,
            });
            b
        },
    ];
    let glow_label = row_label(app, "Glow", 16.0);
    let glow_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 6.0,
                color: violet(),
                intensity: 1.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 24.0,
                color: violet(),
                intensity: 1.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 16.0,
                color: ACCENT,
                intensity: 1.0,
            });
            b
        },
    ];
    let border_label = row_label(app, "Border", 16.0);
    let border_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(2.0, violet()));
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(8.0, violet()));
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(4.0, ACCENT));
            b
        },
    ];
    let opacity_label = row_label(app, "Opacity", 16.0);
    let mut opacity_boxes = Vec::with_capacity(5);
    let mut opacity_item_labels = Vec::with_capacity(5);
    for (alpha, caption) in [(0.25, "25%"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%")] {
        let b = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                size: Vec2::splat(64.0),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::new(1.0, 1.0, 1.0, alpha),
                corner_radius: 10.0,
            })
            .non_interactive(),
        );
        opacity_boxes.push(b);
        opacity_item_labels.push(row_label(app, caption, 12.0));
    }
    // Nested Opacity cascade: parent 0.6 with a child also 0.6 multiplies
    // down to an effective 0.36 — distinguishes cascaded `Opacity` from a
    // flat `color.w` swatch. `ChildOf` here (not standalone): unlike every
    // other row this pair genuinely needs live parent-child composition, so
    // it stays an ordinary hierarchy relationship independent of the
    // panel — no bake/reparent hazard applies since neither entity is ever
    // `ChildOf` the panel itself.
    let nested_parent = effect_box(app, 64.0);
    let _ = nested_parent.set_opacity(app, 0.6);
    let nested_child = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::ZERO,
            size: Vec2::splat(36.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: ACCENT,
            corner_radius: 6.0,
        })
        .non_interactive()
        .opacity(0.6),
    );
    let _ = nested_parent.add_child(app, nested_child);
    opacity_boxes.push(nested_parent);
    opacity_item_labels.push(row_label(app, "0.6 × 0.6", 12.0));

    let effects = EffectsContent {
        row_labels: [drop_shadow_label, glow_label, border_label, opacity_label],
        row_boxes: [drop_shadow_boxes, glow_boxes, border_boxes, opacity_boxes],
        opacity_item_labels: opacity_item_labels.try_into().unwrap(),
    };

    // --- Text (category 1) ---
    const SAMPLE: &str = "Sample Text";
    let text_grey = Vec4::new(0.16, 0.16, 0.17, 1.0);
    let sample = |app: &mut Proteus, text: Text| -> Handle {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(text)
            .non_interactive(),
        )
    };
    let font_size_label = row_label(app, "Font Size", 16.0);
    let font_size_items: Vec<Handle> = [14.0, 20.0, 28.0, 40.0]
        .into_iter()
        .map(|size| sample(app, Text::new(SAMPLE, size).with_color(text_grey)))
        .collect();
    let color_label = row_label(app, "Color", 16.0);
    let color_items: Vec<Handle> = [text_grey, Vec4::ONE, ACCENT]
        .into_iter()
        .map(|color| sample(app, Text::new(SAMPLE, 20.0).with_color(color)))
        .collect();
    let letter_spacing_label = row_label(app, "Letter Spacing", 16.0);
    let letter_spacing_items: Vec<Handle> = [0.0, 4.0, 10.0]
        .into_iter()
        .map(|spacing| {
            sample(
                app,
                Text::new(SAMPLE, 20.0)
                    .with_color(text_grey)
                    .with_letter_spacing(spacing),
            )
        })
        .collect();
    let text = TextContent {
        row_labels: [font_size_label, color_label, letter_spacing_label],
        row_items: [font_size_items, color_items, letter_spacing_items],
    };

    // --- Transformations (category 2) ---
    let transform_box = |app: &mut Proteus, rotation: f32, scale: f32| -> Handle {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                size: Vec2::splat(64.0),
                rotation,
                scale,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::ONE,
                corner_radius: 10.0,
            })
            .non_interactive(),
        )
    };
    let rotation_label = row_label(app, "Rotation", 16.0);
    let rotation_degrees = [0.0_f32, 20.0, -35.0];
    let rotation_boxes: Vec<Handle> = rotation_degrees
        .iter()
        .map(|deg| transform_box(app, deg.to_radians(), 1.0))
        .collect();
    let rotation_item_labels: Vec<Handle> = rotation_degrees
        .iter()
        .map(|deg| row_label(app, &format!("{deg:.0}°"), 12.0))
        .collect();
    let scale_label = row_label(app, "Scale", 16.0);
    let scale_factors = [0.6_f32, 1.0, 1.4];
    let scale_boxes: Vec<Handle> = scale_factors
        .iter()
        .map(|scale| transform_box(app, 0.0, *scale))
        .collect();
    let scale_item_labels: Vec<Handle> = scale_factors
        .iter()
        .map(|scale| row_label(app, &format!("{scale:.1}×"), 12.0))
        .collect();
    let continuous_label = row_label(app, "Continuous Animation", 16.0);
    let continuous_box = transform_box(app, 0.0, 1.0);

    let transforms = TransformsContent {
        row_labels: [rotation_label, scale_label, continuous_label],
        row_boxes: [rotation_boxes, scale_boxes, vec![continuous_box]],
        item_labels: [rotation_item_labels, scale_item_labels, Vec::new()],
    };

    // --- Keyboard & Focus (category 4) ---
    let keys = KeysContent {
        keyboard: keyboard::spawn(app, CONTENT_Z),
        lines: KEYS_LINES.map(|line| row_label(app, line, 16.0)),
    };

    // --- About (category 5) ---
    let about = AboutContent {
        lines: ABOUT_LINES.map(|line| row_label(app, line, 16.0)),
    };

    ExampleDetail {
        panel,
        transforms_continuous_box: continuous_box,
        headings,
        layout_message,
        keys,
        effects,
        text,
        transforms,
        about,
    }
}

/// The panel's target rest geometry for category `idx` — computed fresh
/// each time, not cached.
/// Corner radius here is the static light-theme value (`CORNER_RADIUS`) —
/// `Demo::advance_theme` overwrites it every tick with the live blended
/// value once the panel actually lands, same "settle to a hardcoded light
/// value, `advance_theme` immediately overwrites it with the real one"
/// convention `screens::home`'s buttons already use.
pub fn panel_target(idx: usize, viewport_size: Vec2) -> QuadState {
    let width = (viewport_size.x * 0.85).min(1000.0);
    let max_height = (viewport_size.y - 2.0 * TOP_CLEARANCE_PX).max(0.0);
    let height = (CONTENT_HEIGHT[idx] + 2.0 * PANEL_PADDING_PX).min(max_height);
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(width, height),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: LIGHT_GREY,
        corner_radius: CORNER_RADIUS,
    }
}

/// Positions category `idx`'s heading and every row/item — all in absolute
/// world coordinates derived from `panel`'s (fixed, already-settled) target
/// rect. Safe to call every tick: idempotent once every baked size involved
/// has stabilized (mirrors `screens::splash::recenter`'s "wait on baking"
/// pattern) — rows silently keep their previous position for a frame or two
/// if a label hasn't baked yet.
pub fn layout_content(app: &mut Proteus, detail: &ExampleDetail, idx: usize, panel: &QuadState) {
    let heading = detail.headings[idx];
    if let Some(size) = heading.baked_text_size(app) {
        let x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX + size.x / 2.0;
        let y = panel.position.y + panel.size.y / 2.0 + HEADING_PANEL_GAP_PX + size.y / 2.0;
        if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(heading.id()) {
            qs.position.x = x;
            qs.position.y = y;
        }
    }

    match idx {
        0 => layout_effects(app, detail, panel),
        1 => layout_text(app, detail, panel),
        2 => layout_transforms(app, detail, panel),
        3 => place(
            app,
            detail.layout_message,
            panel.position.x,
            panel.position.y,
        ),
        KEYS => layout_keys(app, detail, panel),
        5 => layout_about(app, detail, panel),
        _ => {}
    }
}

fn place(app: &mut Proteus, handle: Handle, world_x: f32, world_y: f32) {
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(handle.id()) {
        qs.position.x = world_x;
        qs.position.y = world_y;
    }
}

fn label_width(app: &Proteus, handle: Handle) -> f32 {
    handle.baked_text_size(app).map(|s| s.x).unwrap_or(0.0)
}

fn layout_effects(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 90.0;
    const LABEL_BOX_GAP: f32 = 24.0;
    const BOX_GAP: f32 = 30.0;
    const BOX_SIZE: f32 = 64.0;
    const OPACITY_LABEL_GAP: f32 = 18.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - BOX_SIZE / 2.0;
    let row_y = [
        top_y,
        top_y - ROW_GAP_Y,
        top_y - 2.0 * ROW_GAP_Y,
        top_y - 3.0 * ROW_GAP_Y,
    ];

    let boxes_start_x = row_left_x + label_width(app, detail.effects.row_labels[0]) + LABEL_BOX_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.effects.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        for (i, &box_entity) in detail.effects.row_boxes[row].iter().enumerate() {
            let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
            place(app, box_entity, x, y);
            if row == 3 {
                let caption = detail.effects.opacity_item_labels[i];
                place(app, caption, x, y - BOX_SIZE / 2.0 - OPACITY_LABEL_GAP);
            }
        }
    }
}

fn layout_text(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 90.0;
    const LABEL_ITEM_GAP: f32 = 24.0;
    const ITEM_GAP: f32 = 30.0;
    const ROW_HEIGHT: f32 = 56.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - ROW_HEIGHT / 2.0;
    let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

    let items_start_x = row_left_x + label_width(app, detail.text.row_labels[2]) + LABEL_ITEM_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.text.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        let mut running_x = items_start_x;
        for &item in &detail.text.row_items[row] {
            let iw = label_width(app, item);
            place(app, item, running_x + iw / 2.0, y);
            running_x += iw + ITEM_GAP;
        }
    }
}

fn layout_transforms(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 120.0;
    const LABEL_BOX_GAP: f32 = 29.0;
    const BOX_GAP: f32 = 40.0;
    const BOX_SIZE: f32 = 64.0;
    const ITEM_LABEL_GAP: f32 = 21.0;
    const EXTRA_VERTICAL_PADDING: f32 = 20.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - EXTRA_VERTICAL_PADDING - BOX_SIZE / 2.0;
    let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

    let boxes_start_x =
        row_left_x + label_width(app, detail.transforms.row_labels[2]) + LABEL_BOX_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.transforms.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        for (i, &box_entity) in detail.transforms.row_boxes[row].iter().enumerate() {
            let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
            place(app, box_entity, x, y);
            if let Some(&caption) = detail.transforms.item_labels[row].get(i) {
                place(app, caption, x, y - BOX_SIZE / 2.0 - ITEM_LABEL_GAP);
            }
        }
    }
}

/// Positions the Keyboard & Focus panel's content, each centered
/// horizontally: the keyboard at the top, and what the keys do below it.
fn layout_keys(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    let x = panel.position.x;
    let top_y = panel.position.y + panel.size.y / 2.0 - PANEL_PADDING_PX;
    place(
        app,
        detail.keys.keyboard.board,
        x,
        top_y - keyboard::SIZE.y / 2.0,
    );
    let lines_top = top_y - keyboard::SIZE.y - KEYS_GAP_PX;
    for (i, &line) in detail.keys.lines.iter().enumerate() {
        place(app, line, x, lines_top - (i as f32 + 0.5) * LINE_HEIGHT_PX);
    }
}

/// Positions the About panel's lines, aligned left, as the other panels'
/// rows are.
fn layout_about(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX + ROW_INDENT_EXTRA;
    let top_y = panel.position.y + panel.size.y / 2.0 - PANEL_PADDING_PX;
    for (i, &line) in detail.about.lines.iter().enumerate() {
        let w = label_width(app, line);
        place(
            app,
            line,
            left_x + w / 2.0,
            top_y - (i as f32 + 0.5) * LINE_HEIGHT_PX,
        );
    }
}

/// Convert a fully-saturated, full-value HSV color (`hue_deg` in degrees,
/// wrapped to `[0, 360)`; s=1, v=1 fixed) to RGB — standard six-sector
/// conversion. Used only by `advance_continuous_animation`'s rainbow hue
/// cycle; all the other colors in the demo are fixed design tokens.
pub(crate) fn hsv_to_rgb(hue_deg: f32) -> Vec3 {
    let h = hue_deg.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as i32 {
        0 => Vec3::new(1.0, x, 0.0),
        1 => Vec3::new(x, 1.0, 0.0),
        2 => Vec3::new(0.0, 1.0, x),
        3 => Vec3::new(0.0, x, 1.0),
        4 => Vec3::new(x, 0.0, 1.0),
        _ => Vec3::new(1.0, 0.0, x),
    }
}

/// Drives the Transformations screen's "Continuous Animation" box:
/// full rotation every 4s, breathing scale 0.8×–1.2× (sine, 2s period),
/// full hue cycle every 5s. `elapsed` accumulates only while this screen is
/// active — the caller (`Demo::advance_example_animation`) is responsible
/// for pausing (not resetting) it otherwise, so the animation resumes from
/// wherever it left off rather than restarting.
pub fn advance_continuous_animation(app: &mut Proteus, detail: &ExampleDetail, elapsed: f32) {
    const ROTATION_PERIOD_SECS: f32 = 4.0;
    const SCALE_PERIOD_SECS: f32 = 2.0;
    const HUE_PERIOD_SECS: f32 = 5.0;
    const SCALE_MIN: f32 = 0.8;
    const SCALE_MAX: f32 = 1.2;

    let rotation = (elapsed / ROTATION_PERIOD_SECS) * std::f32::consts::TAU;
    let scale_mid = (SCALE_MIN + SCALE_MAX) / 2.0;
    let scale_amplitude = (SCALE_MAX - SCALE_MIN) / 2.0;
    let scale =
        scale_mid + scale_amplitude * (elapsed / SCALE_PERIOD_SECS * std::f32::consts::TAU).sin();
    let hue = (elapsed / HUE_PERIOD_SECS) * 360.0;
    let rgb = hsv_to_rgb(hue);

    if let Some(mut qs) = app
        .world_mut()
        .get_mut::<QuadState>(detail.transforms_continuous_box.id())
    {
        qs.rotation = rotation;
        qs.scale = scale;
        qs.color = Vec4::new(rgb.x, rgb.y, rgb.z, 1.0);
    }
}
