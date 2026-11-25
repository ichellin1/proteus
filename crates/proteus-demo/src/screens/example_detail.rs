//! `ExampleDetail` — reached from `screens::home`'s grid. One shared panel
//! entity, holding one of 6 categories' content at a time: Effects, Text,
//! Transformations (static labelled rows demonstrating
//! `DropShadow`/`Glow`/`Border`/`Opacity`, text, and
//! rotation/scale/continuous animation), Layout (coming in v0.6.0, which
//! `layout_message` says), Keyboard & Focus (a virtual keyboard whose
//! keycaps flash as their keys are pressed, which the arrows and Tab move
//! round — see `screens::keyboard` — and what the keys do below it),
//! and About (what Proteus is, for a visitor).
//!
//! ## The Text panel
//!
//! Two columns of rows, each a label and its examples. Styles on the left:
//! font size, color, letter spacing, weight and italic (Inter's slanted, and
//! Lora's own italic face), families (Inter and Lora), and fallback (Hindi
//! that Inter lacks, drawn by Noto Serif Devanagari). Paragraphs and
//! components on the right: wrapping and alignment, line height and a
//! maximum number of lines ending in an ellipsis, mixed styles in one text, a
//! key cap and a small spinning square in a line of text, and text box edges
//! (the same Hindi button centered by its line box, then by its x-height).
//! The fonts are the demo's assets, cut down to the characters shown. Their
//! folder says how to cut them again when the panel's text changes.
//!
//! ## The About panel
//!
//! Narrower than the others, for lines a reader takes in easily: five
//! paragraphs in near-black, each a wrapped text, with the key words in bold
//! violet. The brand's mark stands in the first line before "Proteus", as
//! tall as a capital: a component in the text, showing the splash's first
//! logo frame.
//!
//! Every panel is laid out by measuring its texts before they're drawn, so
//! its rows are in place in its first frame.
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

use proteus_sdk::{
    Border, ComponentSpec, DropShadow, Glow, Handle, Proteus, QuadState, Span, Text, TextAlign,
    TextBox, VerticalAlign,
};

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
/// The Text category, whose small square spins in a line of text.
pub const TEXT: usize = 1;
/// The Transformations category, whose box animates continuously.
pub const TRANSFORMS: usize = 2;

/// The Text panel's serif family, loaded from the demo's assets.
pub const LORA: &str = "Lora";
/// The fallback family for the Text panel's Hindi, loaded from the demo's
/// assets.
pub const DEVANAGARI: &str = "Noto Serif Devanagari";

/// The Text panel's only text in Lora. The font is cut down to its letters,
/// so a test checks they're all in it.
pub(crate) const IN_LORA: &str = "Lora";
/// The Text panel's only text in Lora's italic, also cut down to its letters.
pub(crate) const IN_LORA_ITALIC: &str = "Aa";
/// The Hindi in the Fallback row, "namaste", drawn in Noto Serif Devanagari,
/// which is cut down to the panel's Hindi.
pub(crate) const HINDI_GREETING: &str = "\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}";
/// The Hindi button's label, "read more".
pub(crate) const HINDI_BUTTON: &str = "\u{906}\u{917}\u{947} \u{92a}\u{922}\u{93c}\u{947}\u{902}";
/// The Keyboard & Focus category, with its virtual keyboard.
pub const KEYS: usize = 4;

pub(crate) const PANEL_PADDING_PX: f32 = 24.0;
const HEADING_PANEL_GAP_PX: f32 = 20.0;
/// The size of a panel's heading, and its height: Inter's line box at 28
/// pixels. A test checks it against the heading as it's measured.
const HEADING_SIZE_PX: f32 = 28.0;
const HEADING_HEIGHT_PX: f32 = 35.0;
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
    TEXT_CONTENT_HEIGHT,             // Text
    2.0 * 120.0 + 64.0 + 2.0 * 20.0, // Transforms = 344.0
    100.0,                           // Layout: one line
    keyboard::SIZE.y + KEYS_GAP_PX + KEYS_LINES.len() as f32 * LINE_HEIGHT_PX, // Keyboard & Focus
    ABOUT_CONTENT_HEIGHT,            // About
];

/// The height of a line of 16 px text in the Keyboard & Focus panel.
const LINE_HEIGHT_PX: f32 = 28.0;
/// The space between the keyboard and the lines below it.
const KEYS_GAP_PX: f32 = 24.0;
const KEYS_LINES: [&str; 4] = [
    "The arrow keys and Tab move focus.",
    "Enter or Space chooses the focused key.",
    "Caps Lock switches the keys to their shifted values.",
    "Escape or Backspace goes back.",
];

struct EffectsContent {
    row_labels: [Handle; 4],
    row_boxes: [Vec<Handle>; 4],
    opacity_item_labels: [Handle; 5],
}

/// The Text panel's content height: its taller column, the right, with room
/// to spare.
const TEXT_CONTENT_HEIGHT: f32 = 320.0;
/// The widest the Text panel gets, wider than the other panels' 1000 pixels,
/// for its two columns.
const TEXT_PANEL_MAX_WIDTH: f32 = 1100.0;

/// A row of the Text panel: its label, and its examples from left to right.
struct TextRow {
    label: Handle,
    /// Each example, with its size when it's a component with a size of its
    /// own rather than a text, whose size is measured.
    items: Vec<(Handle, Option<Vec2>)>,
}

struct TextContent {
    /// The rows of the left column, styles, then of the right, paragraphs
    /// and components.
    columns: [Vec<TextRow>; 2],
    /// The small square that spins in a line of text.
    square: Handle,
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

/// The About category: what Proteus is, for a visitor.
pub const ABOUT: usize = 5;

/// The About panel's width: narrower than the other panels, for lines a
/// reader takes in easily.
const ABOUT_PANEL_WIDTH: f32 = 620.0;
/// The About panel's content height: its paragraphs, with room to spare.
const ABOUT_CONTENT_HEIGHT: f32 = 520.0;
/// The space between the About panel's paragraphs.
const ABOUT_PARAGRAPH_GAP: f32 = 14.0;
/// The About panel's text size, and its wrap width: the panel's width inside
/// its padding and the rows' indent.
const ABOUT_TEXT_SIZE: f32 = 17.0;
const ABOUT_WRAP_WIDTH: f32 = ABOUT_PANEL_WIDTH - 2.0 * (PANEL_PADDING_PX + 10.0);
/// The About panel's text color, a near-black, #111.
const NEAR_BLACK: Vec4 = Vec4::new(17.0 / 255.0, 17.0 / 255.0, 17.0 / 255.0, 1.0);
/// The logo mark's size in the About panel's first line: the text's cap
/// height tall, and as wide as the mark's 208 × 288 shape allows.
const MARK_SIZE: Vec2 = Vec2::new(12.4 * 208.0 / 288.0, 12.4);

struct AboutContent {
    paragraphs: Vec<Handle>,
    /// The logo mark, in the first paragraph's first line, before "Proteus".
    mark: Handle,
}

/// Creates the About panel's paragraphs: near-black text with its key words in
/// bold violet, and the logo mark before the name.
fn about_content(app: &mut Proteus) -> AboutContent {
    let mark = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.001),
            size: MARK_SIZE,
            ..Default::default()
        })
        .non_interactive(),
    );
    let key = |words: &str| Span::new(words).bold().with_color(violet());
    let plain = Span::new;
    let em = |words: &str| Span::new(words).italic();
    let paragraphs = [
        vec![
            // The mark stands on the baseline, as tall as a capital, so it
            // reads as part of the name.
            Span::component(mark.id(), MARK_SIZE),
            plain(" "),
            key("Proteus"),
            plain(" is a rendering framework built around one idea: "),
            key("UI components are metamorphic."),
            plain(" They don't navigate to new screens or swap for other components: they "),
            em("transform"),
            plain(" into them."),
        ],
        vec![
            plain("A "),
            key("button"),
            plain(" can become a "),
            key("list"),
            plain(", and the list can collapse back into the button. A list item can become a "),
            key("video player"),
            plain(". The component never disappears; it "),
            em("shapeshifts"),
            plain(
                ", and every moment of the change is drawn, so you see one object changing \
                 rather than one screen replacing another.",
            ),
        ],
        vec![
            plain("One component can become one other, "),
            key("split"),
            plain(" into many, or "),
            key("merge"),
            plain(" many into one, each a continuous transition."),
        ],
        vec![
            plain("It draws with the "),
            key("GPU"),
            plain(
                ", in one draw call however many components there are, and outperforms CSS \
                 transitions in our benchmarks in Chrome and Safari. It runs ",
            ),
            key("natively"),
            plain(" on macOS, Windows and Linux, and "),
            key("in browsers"),
            plain("."),
        ],
        vec![
            plain("Apps are written in "),
            key("Rust"),
            plain(" or "),
            key("TypeScript"),
            plain(
                ", with text shaped and laid out properly: kerning, fallback fonts, mixed \
                 styles and components within a line, as the Text panel shows.",
            ),
        ],
    ];
    let paragraphs = paragraphs
        .into_iter()
        .map(|spans| {
            sample(
                app,
                Text::rich(spans, ABOUT_TEXT_SIZE)
                    .with_color(NEAR_BLACK)
                    .with_weight(400)
                    .with_wrap_width(ABOUT_WRAP_WIDTH)
                    .with_line_height(1.5),
            )
        })
        .collect();
    AboutContent { paragraphs, mark }
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
    /// Shows the logo mark's `texture` on the About panel's mark, in its
    /// first line. The texture is the splash's first logo frame, which is the
    /// brand's mark, and is never evicted, so it can be given at any time.
    pub fn set_about_mark(&self, app: &mut Proteus, texture: proteus_sdk::TextureHandle) {
        let _ = self.about.mark.set_texture(app, texture);
    }

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
            TEXT => {
                for row in self.text.columns.iter().flatten() {
                    handles.push(row.label);
                    handles.extend(row.items.iter().map(|(item, _)| *item));
                }
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
            ABOUT => handles.extend(&self.about.paragraphs),
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
        .text(Text::new(text, HEADING_SIZE_PX).with_color(violet()))
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

/// The Text panel's dark grey, for its examples.
const TEXT_GREY: Vec4 = Vec4::new(0.16, 0.16, 0.17, 1.0);
/// A muted grey, for the smaller words and the captions.
const MUTED_GREY: Vec4 = Vec4::new(0.42, 0.42, 0.45, 1.0);

/// A component that only shows `text`, at the panel's content depth.
fn sample(app: &mut Proteus, text: Text) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(text)
        .non_interactive(),
    )
}

/// Creates the Text panel's rows: one for each of the text features, in two
/// columns. See `layout_text` for where they go.
fn text_content(app: &mut Proteus) -> TextContent {
    let row = |app: &mut Proteus, label: &str, items: Vec<Text>| TextRow {
        label: row_label(app, label, 16.0),
        items: items
            .into_iter()
            .map(|text| (sample(app, text), None))
            .collect(),
    };
    let grey = |content: &str, size: f32| Text::new(content, size).with_color(TEXT_GREY);

    // --- Styles, on the left ---
    let font_size = row(
        app,
        "Font Size",
        [14.0, 20.0, 28.0, 40.0]
            .map(|size| grey("Text", size))
            .to_vec(),
    );
    let color = row(
        app,
        "Color",
        [TEXT_GREY, Vec4::ONE, ACCENT]
            .map(|c| Text::new("Text", 20.0).with_color(c))
            .to_vec(),
    );
    let spacing = row(
        app,
        "Letter Spacing",
        [0.0, 4.0, 10.0]
            .map(|s| grey("Text", 20.0).with_letter_spacing(s))
            .to_vec(),
    );
    // Inter is variable, so any weight from 100 to 900 is drawn. Its italic
    // isn't loaded, so it's slanted; Lora's italic is a face of its own.
    let mut weights: Vec<Text> = [300, 500, 700, 900]
        .map(|w| grey("Aa", 20.0).with_weight(w))
        .to_vec();
    weights.push(grey("Aa", 20.0).with_weight(500).italic());
    weights.push(grey(IN_LORA_ITALIC, 20.0).with_family(LORA).italic());
    let weight = row(app, "Weight & Italic", weights);
    let families = row(
        app,
        "Families",
        vec![
            grey("Inter", 20.0).with_weight(500),
            grey(IN_LORA, 20.0).with_family(LORA).with_weight(500),
        ],
    );
    // Inter has no Hindi, so Noto Serif Devanagari, the fallback, draws it.
    let fallback = row(
        app,
        "Fallback",
        vec![grey(&format!("Hello, {HINDI_GREETING}"), 20.0)],
    );

    // --- Paragraphs and components, on the right ---
    const PARAGRAPH: &str = "Lines wrap at a width, and align within it.";
    let paragraph = |align| {
        grey(PARAGRAPH, 15.0)
            .with_wrap_width(106.0)
            .with_align(align)
    };
    let wrap = row(
        app,
        "Wrap & Align",
        vec![
            paragraph(TextAlign::Start),
            paragraph(TextAlign::Center),
            paragraph(TextAlign::End),
        ],
    );
    let lines = row(
        app,
        "Lines",
        vec![
            grey(PARAGRAPH, 15.0)
                .with_wrap_width(138.0)
                .with_line_height(1.6),
            grey(
                "A paragraph longer than its two lines ends in an ellipsis of its own.",
                15.0,
            )
            .with_wrap_width(172.0)
            .with_max_lines(2)
            .with_ellipsis(" \u{2026} more"),
        ],
    );
    let mixed = row(
        app,
        "Mixed Styles",
        vec![Text::rich(
            [
                Span::new("Bold").bold(),
                Span::new(", "),
                Span::new("italic").italic(),
                Span::new(", "),
                Span::new("colored").with_color(ACCENT).bold(),
                Span::new(" and "),
                Span::new("small").with_size(13.0).with_color(MUTED_GREY),
                Span::new(", with a mark"),
                Span::new("1").with_vertical_align(VerticalAlign::Super),
            ],
            18.0,
        )
        .with_color(TEXT_GREY)
        .with_weight(400)],
    );

    // A key cap and a small spinning square, each a component in a gap in the
    // line. The square's gap is wide enough for its corners as it turns and
    // grows, so they never touch the words beside it.
    let key = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.001),
            size: KEY_SIZE,
            color: Vec4::ONE,
            corner_radius: 5.0,
            ..Default::default()
        })
        .border(Border::new(1.5, violet()))
        .text(
            Text::new("K", 14.0)
                .with_color(violet())
                .with_box(TextBox::CapHeight),
        )
        .non_interactive(),
    );
    let square = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.001),
            size: Vec2::splat(SQUARE_SIDE),
            color: ACCENT,
            corner_radius: 3.0,
            ..Default::default()
        })
        .non_interactive(),
    );
    let components = row(
        app,
        "Components",
        vec![Text::rich(
            [
                Span::new("Press "),
                Span::component(key.id(), KEY_SIZE + Vec2::new(4.0, 2.0))
                    .with_vertical_align(VerticalAlign::Middle),
                Span::new(" to continue, or watch "),
                Span::component(square.id(), Vec2::splat(SQUARE_GAP))
                    .with_vertical_align(VerticalAlign::Middle),
                Span::new(" spin."),
            ],
            18.0,
        )
        .with_color(TEXT_GREY)
        .with_weight(400)],
    );

    // The same Hindi button twice: its label centered by its line box, which
    // has room below for vowel marks the label doesn't use, then by its
    // x-height, where Devanagari's headline is.
    let label = |text_box| {
        Text::new(HINDI_BUTTON, 15.0)
            .with_color(Vec4::ONE)
            .with_box(text_box)
    };
    let pill_size = app
        .measure_text(&label(TextBox::XHeight))
        .unwrap_or(Vec2::new(60.0, 8.0))
        + Vec2::new(2.0 * 12.0, 2.0 * 9.0);
    let pill = |app: &mut Proteus, text_box| {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                size: pill_size,
                color: violet(),
                corner_radius: pill_size.y / 2.0,
                ..Default::default()
            })
            .text(label(text_box))
            .non_interactive(),
        )
    };
    let caption = |app: &mut Proteus, content: &str| {
        sample(app, Text::new(content, 14.0).with_color(MUTED_GREY))
    };
    let text_box = TextRow {
        label: row_label(app, "Text Box", 16.0),
        items: vec![
            (pill(app, TextBox::Line), Some(pill_size)),
            (caption(app, "line box"), None),
            (pill(app, TextBox::XHeight), Some(pill_size)),
            (caption(app, "x-height"), None),
        ],
    };

    TextContent {
        columns: [
            vec![font_size, color, spacing, weight, families, fallback],
            vec![wrap, lines, mixed, components, text_box],
        ],
        square,
    }
}

/// The key cap's size, in the Text panel's line of text.
const KEY_SIZE: Vec2 = Vec2::new(22.0, 22.0);
/// The spinning square's side.
const SQUARE_SIDE: f32 = 16.0;
/// The side of the spinning square's gap: its diagonal, √2 times its side, at
/// the largest it breathes to, 1.2 times, with a pixel to spare.
const SQUARE_GAP: f32 = SQUARE_SIDE * std::f32::consts::SQRT_2 * 1.2 + 1.0;

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
    let text = text_content(app);

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
    let about = about_content(app);

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
    let width = match idx {
        TEXT => (viewport_size.x * 0.9).min(TEXT_PANEL_MAX_WIDTH),
        ABOUT => (viewport_size.x * 0.85).min(ABOUT_PANEL_WIDTH),
        _ => (viewport_size.x * 0.85).min(1000.0),
    };
    let max_height = (viewport_size.y - 2.0 * TOP_CLEARANCE_PX).max(0.0);
    let height = (CONTENT_HEIGHT[idx] + 2.0 * PANEL_PADDING_PX).min(max_height);
    // The panel and its heading above it are centered together in the space
    // between the nav buttons' bottom edge and the window's bottom, so the
    // heading is clear of the buttons. The space's middle is half the
    // buttons' reach below the window's middle, and the panel sits lower
    // than that by half the heading and its gap.
    let nav_reach = super::nav::MARGIN_PX + super::nav::ICON_SIZE_PX;
    let y = -nav_reach / 2.0 - (HEADING_HEIGHT_PX + HEADING_PANEL_GAP_PX) / 2.0;
    QuadState {
        position: Vec3::new(0.0, y, 0.5),
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
/// rect. Each text is measured before it's drawn, so every row is in place in
/// the panel's first frame. Safe to call every tick: it's idempotent.
pub fn layout_content(app: &mut Proteus, detail: &ExampleDetail, idx: usize, panel: &QuadState) {
    let heading = detail.headings[idx];
    let size = measure(app, heading);
    let x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX + size.x / 2.0;
    let y = panel.position.y + panel.size.y / 2.0 + HEADING_PANEL_GAP_PX + size.y / 2.0;
    place(app, heading, x, y);

    match idx {
        0 => layout_effects(app, detail, panel),
        TEXT => layout_text(app, detail, panel),
        2 => layout_transforms(app, detail, panel),
        3 => place(
            app,
            detail.layout_message,
            panel.position.x,
            panel.position.y,
        ),
        KEYS => layout_keys(app, detail, panel),
        ABOUT => layout_about(app, detail, panel),
        _ => {}
    }
}

fn place(app: &mut Proteus, handle: Handle, world_x: f32, world_y: f32) {
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(handle.id()) {
        qs.position.x = world_x;
        qs.position.y = world_y;
    }
}

/// Returns the size of `handle`'s text, measured before it's drawn, or zero
/// for a component with no text.
fn measure(app: &mut Proteus, handle: Handle) -> Vec2 {
    let Some(text) = app.world().get::<Text>(handle.id()).cloned() else {
        return Vec2::ZERO;
    };
    app.measure_text(&text).unwrap_or(Vec2::ZERO)
}

fn label_width(app: &mut Proteus, handle: Handle) -> f32 {
    measure(app, handle).x
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

/// Positions the Text panel's two columns of rows. In each column, the
/// examples start after its widest label, and each row is as tall as its
/// tallest example, with a gap below it. The right column starts at the
/// panel's middle.
fn layout_text(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 22.0;
    const LABEL_ITEM_GAP: f32 = 20.0;
    const ITEM_GAP: f32 = 16.0;

    let panel_left = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let column_left = [
        panel_left + ROW_INDENT_EXTRA,
        panel.position.x + ROW_INDENT_EXTRA,
    ];
    let top_y = panel.position.y + panel.size.y / 2.0 - PANEL_PADDING_PX;

    for (rows, left) in detail.text.columns.iter().zip(column_left) {
        let label_width = rows
            .iter()
            .map(|row| measure(app, row.label).x)
            .fold(0.0, f32::max);
        let items_left = left + label_width + LABEL_ITEM_GAP;
        let mut y = top_y;
        for row in rows {
            let sizes: Vec<Vec2> = row
                .items
                .iter()
                .map(|&(item, size)| size.unwrap_or_else(|| measure(app, item)))
                .collect();
            let label = measure(app, row.label);
            let height = sizes.iter().map(|s| s.y).fold(label.y, f32::max);
            let middle = y - height / 2.0;
            place(app, row.label, left + label.x / 2.0, middle);
            let mut x = items_left;
            for (&(item, _), size) in row.items.iter().zip(&sizes) {
                place(app, item, x + size.x / 2.0, middle);
                x += size.x + ITEM_GAP;
            }
            y -= height + ROW_GAP_Y;
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

/// Positions the About panel's paragraphs, aligned left as the other
/// panels' rows are, one below another with a gap between them. Each is
/// measured, so the next starts below the last line of the one before.
fn layout_about(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX + ROW_INDENT_EXTRA;
    let mut top_y = panel.position.y + panel.size.y / 2.0 - PANEL_PADDING_PX;
    for &paragraph in &detail.about.paragraphs {
        let size = measure(app, paragraph);
        place(app, paragraph, left_x + size.x / 2.0, top_y - size.y / 2.0);
        top_y -= size.y + ABOUT_PARAGRAPH_GAP;
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

/// Drives the panel `idx`'s spinning box: the Transformations screen's
/// "Continuous Animation" box, or the Text screen's small square in its line
/// of text. Each turns fully every 4s, breathes in scale 0.8×–1.2× (sine, 2s
/// period), and cycles fully through the hues every 5s. `elapsed` accumulates
/// only while that screen is active — the caller
/// (`Demo::advance_example_animation`) is responsible for pausing (not
/// resetting) it otherwise, so the animation resumes from wherever it left
/// off rather than restarting.
///
/// Only the box's rotation, scale and color change. The text that holds the
/// square owns its position, so its text isn't laid out or baked again as it
/// turns.
pub fn advance_continuous_animation(
    app: &mut Proteus,
    detail: &ExampleDetail,
    idx: usize,
    elapsed: f32,
) {
    let spinning = match idx {
        TEXT => detail.text.square,
        _ => detail.transforms_continuous_box,
    };
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

    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(spinning.id()) {
        qs.rotation = rotation;
        qs.scale = scale;
        qs.color = Vec4::new(rgb.x, rgb.y, rgb.z, 1.0);
    }
}
