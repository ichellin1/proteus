//! A reading app, the same app as the TypeScript essays example.
//!
//! A row of cards, one for each of three public-domain essays. Each card's
//! year is aligned to the end of its column; its title wraps onto at most two
//! lines, ending in "…"; its opening is spaced at a line height of 1.45 and
//! stops after five lines, ending in an ellipsis of the card's own; and its
//! source is clipped after two lines. A card opens into its essay (1→1), whose
//! byline is two lines split with a `\n`, whose opening wraps at the column's
//! width, and whose pull quote is centered. While an essay is open, a
//! toolbar at the top sets its body's size, alignment, bold and italic, line
//! height and font, as a text editor's does.
//!
//! The essays are Mary Wollstonecraft's "A Vindication of the Rights of
//! Woman" (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis
//! Stevenson's "An Apology for Idlers" (1877), from their Project Gutenberg
//! editions, each cited where it's shown.
//!
//! The essays are set in Lora, a serif built in from `fonts/`, with its
//! italic for the pull quotes; titles are Inter at 800 and years at 500, from
//! the built-in variable font; bylines are Inter slanted, since Inter's
//! italic isn't loaded; and the buttons and sources are Inter with no weight
//! set, bold. The toolbar's icons are Material Icons, a font too, and its
//! last button sets the body in Crimson Pro, downloaded when it's first
//! chosen.
//!
//! Techniques worth copying:
//!   - A font's file is loaded once with `load_font`, which returns its
//!     family's name, and texts name the family. Lora's italic is loaded the
//!     same way, and joins its family.
//!   - A font that's downloaded later is expected first, with `expect_font`.
//!     Crimson Pro is expected with `WhileLoading::DefaultFont`, so its button
//!     and the body show the default font until it arrives, then swap to it;
//!     `update` fetches it with the host's `fetch_async`, then calls
//!     `load_font`, or `font_failed` if the fetch fails.
//!   - An icon font's ligatures turn a name, such as "format_bold", into its
//!     icon: the toolbar's icons are text in Material Icons.
//!   - A chosen button, or one toggled on, has its hover style removed with
//!     `set_style`, so it stays in the accent color under the pointer, and
//!     gets it back when it's no longer chosen.
//!   - Text is placed by its component's anchor: with an anchor of (0, 0),
//!     the text's top-left corner is at the component's position. A card's
//!     texts are small children, placed from the card's top-left corner.
//!   - A wrap width is the text's box: lines align within it, and the box
//!     doesn't change size when the words do, so the year, aligned to the
//!     end, lines up with the card's right edge whatever it says.
//!   - An ellipsis wider than a line takes lines of its own: the third card's
//!     opening ends in a whole sentence.
//!   - Texts are hidden while their card or the essay changes shape, and
//!     shown when it arrives, since children move with their parent but don't
//!     stretch with it.
//!   - Changing a text's alignment or line height means setting it again:
//!     `set_text` replaces it, and the host lays it out again.
//!
//! Not shown here: a weight a font doesn't have, other scripts, mixed styles
//! within a text, measuring text, and components within a line of text.

use std::cell::Cell;
use std::rc::Rc;

use proteus_runtime::{App, FetchId, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    ComponentSpec, Easing, Handle, InputDevices, InteractionStateKind, Proteus, QuadState,
    StyleOverride, Text, TextAlign, TextOverflow, TransitionChannel, TransitionConfig,
    WhileLoading,
};

const BACKGROUND: Vec4 = rgb(0x1d1b26);
const PAPER: Vec4 = rgb(0xf4f1ea);
const INK: Vec4 = rgb(0x24212b);
const MUTED: Vec4 = rgb(0x6b6676);
const ACCENT: Vec4 = rgb(0x7a5fb0);
/// A button, light enough to stand out from the background.
const BUTTON: Vec4 = rgb(0x4d4a58);
const BUTTON_HOVER: Vec4 = rgb(0x5f5c6a);
const WHITE: Vec4 = rgb(0xffffff);
const CLEAR: Vec4 = Vec4::ZERO;

const OPEN: TransitionConfig = TransitionConfig {
    duration: 0.45,
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

/// An essay: what its card shows, and what it opens into.
struct Essay {
    year: &'static str,
    title: &'static str,
    author: &'static str,
    /// Its first sentences, from its Project Gutenberg edition.
    opening: &'static str,
    /// What ends the opening on the card when it runs past five lines.
    more: &'static str,
    /// A line from later in the essay.
    quote: &'static str,
    /// Where the text is from: two lines on the card, in full in the essay.
    source: &'static str,
}

const ESSAYS: [Essay; 3] = [
    Essay {
        year: "1792",
        title: "A Vindication of the Rights of Woman: with Strictures on Political and \
            Moral Subjects",
        author: "Mary Wollstonecraft",
        opening: "After considering the historic page, and viewing the living world with \
            anxious solicitude, the most melancholy emotions of sorrowful indignation have \
            depressed my spirits, and I have sighed when obliged to confess, that either \
            nature has made a great difference between man and man, or that the \
            civilization, which has hitherto taken place in the world, has been very partial.",
        more: "\u{2026}",
        quote: "\u{201c}I do not wish them to have power over men; but over themselves.\u{201d}",
        source: "From \u{201c}A Vindication of the Rights of Woman\u{201d} by Mary \
            Wollstonecraft (1792), Project Gutenberg eBook #3420. Public domain.",
    },
    Essay {
        year: "1862",
        title: "Walking",
        author: "Henry David Thoreau",
        opening: "I wish to speak a word for Nature, for absolute Freedom and Wildness, as \
            contrasted with a freedom and culture merely civil,\u{2014}to regard man as an \
            inhabitant, or a part and parcel of Nature, rather than a member of society. I \
            wish to make an extreme statement, if so I may make an emphatic one, for there \
            are enough champions of civilization: the minister and the school committee and \
            every one of you will take care of that.",
        more: " read more",
        quote: "\u{201c}In Wildness is the preservation of the World.\u{201d}",
        source: "From \u{201c}Walking\u{201d} by Henry David Thoreau (1862), Project \
            Gutenberg eBook #1022. Public domain.",
    },
    Essay {
        year: "1877",
        title: "An Apology for Idlers",
        author: "Robert Louis Stevenson",
        opening: "Just now, when every one is bound, under pain of a decree in absence \
            convicting them of l\u{e8}se-respectability, to enter on some lucrative \
            profession, and labour therein with something not far short of enthusiasm, a cry \
            from the opposite party who are content when they have enough, and like to look \
            on and enjoy in the meanwhile, savours a little of bravado and gasconade. And yet \
            this should not be.",
        more: " \u{2014} continue reading the whole essay inside",
        quote: "\u{201c}Extreme busyness, whether at school or college, kirk or market, is a \
            symptom of deficient vitality.\u{201d}",
        source: "From \u{201c}An Apology for Idlers\u{201d} by Robert Louis Stevenson (1877), \
            in \u{201c}Virginibus Puerisque\u{201d}, Project Gutenberg eBook #386. Public domain.",
    },
];

/// The family the essays are set in, and its two files, from Google Fonts
/// under the SIL Open Font License; see `fonts/OFL-Lora.txt`.
const LORA: &str = "Lora";
const LORA_UPRIGHT: &[u8] = include_bytes!("../../fonts/Lora-Variable.ttf");
const LORA_ITALIC: &[u8] = include_bytes!("../../fonts/Lora-Italic-Variable.ttf");

/// The second family the body can be set in, from Google Fonts under the SIL
/// Open Font License; see `fonts/OFL-CrimsonPro.txt`. Its file isn't loaded until
/// it's first chosen, from the app's assets: `fonts/` beside `rust/`. It has
/// no italic file, so its italic is slanted.
const CRIMSON: &str = "Crimson Pro";
const CRIMSON_FILE: &str = "fonts/CrimsonPro-Variable.ttf";

/// The toolbar's icons: Google's Material Icons, a font whose ligatures turn
/// an icon's name, such as "format_bold", into its glyph. Apache License 2.0;
/// see `fonts/LICENSE-MaterialIcons.txt`.
const ICONS: &str = "Material Icons";
const ICONS_FONT: &[u8] = include_bytes!("../../fonts/MaterialIcons-Regular.ttf");

const CARD_SIZE: Vec2 = Vec2::new(320.0, 300.0);
const CARD_Y: f32 = -20.0;
const CARD_SPACING: f32 = 350.0;
const CARD_PADDING: f32 = 20.0;
const CARD_COLUMN: f32 = CARD_SIZE.x - 2.0 * CARD_PADDING;

const PANEL_SIZE: Vec2 = Vec2::new(960.0, 600.0);
const PANEL_Y: f32 = -40.0;
const PANEL_PADDING: f32 = 40.0;
const PANEL_COLUMN: f32 = PANEL_SIZE.x - 2.0 * PANEL_PADDING;

/// How far the title sits from the window's left edge, and the toolbar from
/// its right edge.
const MARGIN: f32 = 40.0;
const TITLE_TOP: f32 = 362.0;
/// The middle of the title's line, where the toolbar is centered.
const BAR_Y: f32 = TITLE_TOP - 19.0;
const TOOL_SIZE: Vec2 = Vec2::new(34.0, 30.0);
/// The gap between a group's buttons, and between groups.
const TOOL_GAP: f32 = 4.0;
const GROUP_GAP: f32 = 14.0;

/// The body's sizes: the icon's size on its button, and the body's.
const SIZES: [(f32, f32); 3] = [(14.0, 15.0), (18.0, 17.0), (22.0, 20.0)];
/// The body's alignments, with their icons.
const ALIGNS: [(&str, TextAlign); 3] = [
    ("format_align_left", TextAlign::Start),
    ("format_align_center", TextAlign::Center),
    ("format_align_right", TextAlign::End),
];
/// The body's line heights, with their icons.
const SPACINGS: [(&str, f32); 3] = [
    ("density_small", 1.2),
    ("density_medium", 1.5),
    ("density_large", 1.8),
];
/// The families the body can be set in.
const FAMILIES: [&str; 2] = [LORA, CRIMSON];

/// The engine settings both entry points use.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config.input.autofocus = true;
    config
}

/// The essays app. Its callbacks do most of its work; `update` downloads
/// Crimson Pro once it's first chosen.
#[derive(Default)]
pub struct Essays {
    ui: Option<Rc<Ui>>,
    /// Crimson Pro's download, once started.
    crimson: Option<FetchId>,
}

impl App for Essays {
    fn setup(&mut self, f: &mut Frame) {
        self.ui = Some(build(f.proteus, f.viewport.logical_size.x));
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        let Some(ui) = &self.ui else {
            return;
        };
        if ui.crimson_wanted.get() && self.crimson.is_none() {
            self.crimson = Some(f.fetch_async(CRIMSON_FILE));
        }
        for (id, bytes) in f.poll_fetches() {
            if Some(id) != self.crimson {
                continue;
            }
            // Loaded, its texts are drawn in it; failed, in the default font.
            let loaded = bytes.map(|bytes| f.proteus.load_font(bytes));
            if !matches!(loaded, Some(Ok(_))) {
                f.proteus.font_failed(CRIMSON);
            }
        }
    }
}

/// A component that only shows `text`, with the text's top-left corner at
/// `(x, y)`, relative to the parent it's given to.
fn text_at(x: f32, y: f32, text: Text) -> ComponentSpec {
    ComponentSpec::new(QuadState {
        position: Vec3::new(x, y, 0.0),
        size: Vec2::ONE,
        anchor: Vec2::ZERO,
        color: CLEAR,
        ..Default::default()
    })
    .non_interactive()
    .text(text)
}

/// A small button, with its label, lighter on hover.
fn button(position: Vec3, size: Vec2, label: &str) -> ComponentSpec {
    ComponentSpec::new(QuadState {
        position,
        size,
        corner_radius: size.y / 2.0,
        color: BUTTON,
        ..Default::default()
    })
    .hover(StyleOverride {
        color: Some(BUTTON_HOVER),
        ..Default::default()
    })
    .pressed(StyleOverride {
        scale: Some(0.96),
        ..Default::default()
    })
    .text(Text::new(label, 14.0).with_color(WHITE))
}

/// A card's texts, in reading order.
fn card_texts(app: &mut Proteus, essay: &Essay) -> Vec<Handle> {
    let left = -CARD_SIZE.x / 2.0 + CARD_PADDING;
    let top = CARD_SIZE.y / 2.0 - CARD_PADDING;
    let specs = [
        // Aligned to the end of the column, so it lines up with the card's
        // right edge.
        text_at(
            left,
            top,
            Text::new(essay.year, 13.0)
                .with_weight(500)
                .with_color(MUTED)
                .with_wrap_width(CARD_COLUMN)
                .with_align(TextAlign::End),
        ),
        // At most two lines, ending in the default "…".
        text_at(
            left,
            top - 24.0,
            Text::new(essay.title, 22.0)
                .with_weight(800)
                .with_color(INK)
                .with_wrap_width(CARD_COLUMN)
                .with_max_lines(2),
        ),
        // At most five lines, ending in the card's own ellipsis.
        text_at(
            left,
            top - 86.0,
            Text::new(essay.opening, 15.0)
                .with_family(LORA)
                .with_color(INK)
                .with_wrap_width(CARD_COLUMN)
                .with_line_height(1.45)
                .with_max_lines(5)
                .with_ellipsis(essay.more),
        ),
        // At most two lines, at the foot of the card, clipped with no mark.
        text_at(
            left,
            -CARD_SIZE.y / 2.0 + CARD_PADDING + 30.0,
            Text::new(essay.source, 12.0)
                .with_color(MUTED)
                .with_wrap_width(CARD_COLUMN)
                .with_max_lines(2)
                .with_overflow(TextOverflow::Clip),
        ),
    ];
    specs.into_iter().map(|spec| app.component(spec)).collect()
}

/// The open essay's texts and its back button.
struct EssayView {
    title: Handle,
    byline: Handle,
    body: Handle,
    quote: Handle,
    source: Handle,
    back: Handle,
}

impl EssayView {
    fn all(&self) -> [Handle; 6] {
        [
            self.title,
            self.byline,
            self.body,
            self.quote,
            self.source,
            self.back,
        ]
    }
}

/// How the body is set: the toolbar's choices.
#[derive(Clone, Copy, PartialEq)]
struct Settings {
    size: usize,
    align: usize,
    bold: bool,
    italic: bool,
    spacing: usize,
    family: usize,
}

/// The toolbar's buttons, by group.
struct Toolbar {
    sizes: Vec<Handle>,
    aligns: Vec<Handle>,
    bold: Handle,
    italic: Handle,
    spacings: Vec<Handle>,
    families: Vec<Handle>,
}

impl Toolbar {
    fn all(&self) -> Vec<Handle> {
        let mut all = Vec::new();
        all.extend(&self.sizes);
        all.extend(&self.aligns);
        all.extend([self.bold, self.italic]);
        all.extend(&self.spacings);
        all.extend(&self.families);
        all
    }
}

/// What the callbacks share.
struct Ui {
    cards: Vec<Handle>,
    card_texts: Vec<Vec<Handle>>,
    panel: Handle,
    essay: EssayView,
    /// The toolbar, shown while an essay is open.
    tools: Toolbar,
    channel: TransitionChannel,
    /// The essay open, or opening.
    open: Cell<Option<usize>>,
    settings: Cell<Settings>,
    /// Whether Crimson Pro has been chosen, so `update` downloads it.
    crimson_wanted: Cell<bool>,
}

impl Ui {
    /// The opening of essay `i`, set as the toolbar says.
    fn body(&self, i: usize) -> Text {
        let settings = self.settings.get();
        let text = Text::new(ESSAYS[i].opening, SIZES[settings.size].1)
            .with_family(FAMILIES[settings.family])
            .with_weight(if settings.bold { 700 } else { 400 })
            .with_color(INK)
            .with_wrap_width(PANEL_COLUMN)
            .with_align(ALIGNS[settings.align].1)
            .with_line_height(SPACINGS[settings.spacing].1);
        if settings.italic {
            text.italic()
        } else {
            text
        }
    }

    /// Turns card `i` into the essay panel.
    fn open(&self, app: &mut Proteus, i: usize) {
        if self.open.get().is_some() {
            return;
        }
        self.open.set(Some(i));
        let essay = &ESSAYS[i];
        let view = &self.essay;
        let _ = view.title.set_text(
            app,
            Text::new(essay.title, 30.0)
                .with_weight(800)
                .with_color(INK)
                .with_wrap_width(PANEL_COLUMN),
        );
        // Two lines, split with a `\n`.
        let byline = format!("{}\n{}", essay.author, essay.year);
        let _ = view
            .byline
            .set_text(app, Text::new(byline, 14.0).with_color(MUTED).italic());
        let _ = view.body.set_text(app, self.body(i));
        let _ = view.quote.set_text(
            app,
            Text::new(essay.quote, 20.0)
                .with_family(LORA)
                .italic()
                .with_color(ACCENT)
                .with_wrap_width(600.0)
                .with_align(TextAlign::Center),
        );
        let _ = view.source.set_text(
            app,
            Text::new(essay.source, 12.0)
                .with_color(MUTED)
                .with_wrap_width(PANEL_COLUMN),
        );
        set_visible(app, &self.card_texts[i], false);
        self.channel.set(app, self.panel, self.cards[i], OPEN, true);
    }

    /// Turns the essay panel back into the card it came from.
    fn close(&self, app: &mut Proteus) {
        let Some(i) = self.open.take() else {
            return;
        };
        set_visible(app, &self.essay.all(), false);
        set_visible(app, &self.tools.all(), false);
        self.channel.set(app, self.cards[i], self.panel, OPEN, true);
    }

    /// Applies the toolbar's choices to its buttons and the open body.
    fn apply(&self, app: &mut Proteus, settings: Settings) {
        self.settings.set(settings);
        let tools = &self.tools;
        select(app, &tools.sizes, settings.size);
        select(app, &tools.aligns, settings.align);
        mark(app, tools.bold, settings.bold);
        mark(app, tools.italic, settings.italic);
        select(app, &tools.spacings, settings.spacing);
        select(app, &tools.families, settings.family);
        if FAMILIES[settings.family] == CRIMSON {
            self.crimson_wanted.set(true);
        }
        if let Some(i) = self.open.get() {
            let _ = self.essay.body.set_text(app, self.body(i));
        }
    }

    /// Changes the settings with `change`, and applies them.
    fn change(&self, app: &mut Proteus, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.settings.get();
        change(&mut settings);
        self.apply(app, settings);
    }
}

fn set_visible(app: &mut Proteus, handles: &[Handle], visible: bool) {
    for handle in handles {
        let _ = handle.set_visible(app, visible);
    }
}

/// A toolbar button at `x`, showing `label`: an icon, or "Aa" in a family.
fn tool(x: f32, label: Text) -> ComponentSpec {
    ComponentSpec::new(QuadState {
        position: Vec3::new(x, BAR_Y, 0.0),
        size: TOOL_SIZE,
        corner_radius: 8.0,
        color: BUTTON,
        ..Default::default()
    })
    .hover(StyleOverride {
        color: Some(BUTTON_HOVER),
        ..Default::default()
    })
    .pressed(StyleOverride {
        scale: Some(0.94),
        ..Default::default()
    })
    .visible(false)
    .text(label.with_color(WHITE))
}

/// A toolbar button's label, from its index in its group.
type Label = fn(usize) -> Text;

/// An icon from Material Icons, by name, at `size` pixels.
fn icon(name: &str, size: f32) -> Text {
    Text::new(name, size).with_family(ICONS)
}

/// Marks `button` as on, in the accent color, or off. An on button has no
/// hover style, so the pointer leaves it as it is; off, it gets it back.
fn mark(app: &mut Proteus, button: Handle, on: bool) {
    let hover = (!on).then_some(StyleOverride {
        color: Some(BUTTON_HOVER),
        ..Default::default()
    });
    let _ = button.set_style(app, InteractionStateKind::Hover, hover);
    if let Some(mut geometry) = button.declared_geometry(app) {
        geometry.color = if on { ACCENT } else { BUTTON };
        let _ = button.set_declared_geometry(app, geometry);
    }
}

/// Marks the `chosen` button of a group as on, and the rest as off.
fn select(app: &mut Proteus, buttons: &[Handle], chosen: usize) {
    for (i, &button) in buttons.iter().enumerate() {
        mark(app, button, i == chosen);
    }
}

/// Creates the app's components, for a window `width` logical pixels wide,
/// and its callbacks, which do the rest.
fn build(app: &mut Proteus, width: f32) -> Rc<Ui> {
    // Lora, upright and italic: both files are one family. The icons are a
    // font too. Crimson Pro is coming, once it's chosen: until then, its
    // texts are drawn in the default font, and again in it once it loads.
    for font in [LORA_UPRIGHT, LORA_ITALIC, ICONS_FONT] {
        app.load_font(font).expect("the example's fonts are valid");
    }
    app.expect_font(CRIMSON, WhileLoading::DefaultFont);

    // The title sits a margin from the window's left edge, and the toolbar,
    // on the title's line, a margin from its right edge.
    app.component(text_at(
        -width / 2.0 + MARGIN,
        TITLE_TOP,
        Text::new("Essays", 30.0).with_color(WHITE),
    ));

    // The toolbar: its groups, left to right, each button's label made from
    // its index in the group. Hidden until an essay is open, since it changes
    // only the essay.
    let groups: [(usize, Label); 6] = [
        (SIZES.len(), |i| icon("format_size", SIZES[i].0)),
        (ALIGNS.len(), |i| icon(ALIGNS[i].0, 20.0)),
        (1, |_| icon("format_bold", 20.0)),
        (1, |_| icon("format_italic", 20.0)),
        (SPACINGS.len(), |i| icon(SPACINGS[i].0, 20.0)),
        (FAMILIES.len(), |i| {
            Text::new("Aa", 17.0).with_family(FAMILIES[i])
        }),
    ];
    // Bold and italic sit together, as one group.
    let gaps = [GROUP_GAP, GROUP_GAP, TOOL_GAP, GROUP_GAP, GROUP_GAP, 0.0];
    let width_of = |n: usize| n as f32 * TOOL_SIZE.x + (n as f32 - 1.0) * TOOL_GAP;
    let bar_width: f32 = groups
        .iter()
        .zip(gaps)
        .map(|((n, _), gap)| width_of(*n) + gap)
        .sum();
    let mut x = width / 2.0 - MARGIN - bar_width + TOOL_SIZE.x / 2.0;
    let mut made: Vec<Vec<Handle>> = Vec::new();
    for ((count, label), gap) in groups.into_iter().zip(gaps) {
        let mut group = Vec::new();
        for i in 0..count {
            group.push(app.component(tool(x, label(i))));
            x += TOOL_SIZE.x + TOOL_GAP;
        }
        x += gap - TOOL_GAP;
        made.push(group);
    }
    let mut made = made.into_iter();
    let mut next = || made.next().expect("one for each group");
    let tools = Toolbar {
        sizes: next(),
        aligns: next(),
        bold: next()[0],
        italic: next()[0],
        spacings: next(),
        families: next(),
    };

    // A card for each essay, with its texts as children.
    let mut cards = Vec::new();
    let mut texts = Vec::new();
    for (i, essay) in ESSAYS.iter().enumerate() {
        let card_text = card_texts(app, essay);
        let mut spec = ComponentSpec::new(QuadState {
            position: Vec3::new((i as f32 - 1.0) * CARD_SPACING, CARD_Y, 0.0),
            size: CARD_SIZE,
            corner_radius: 18.0,
            color: PAPER,
            ..Default::default()
        })
        .hover(StyleOverride {
            scale: Some(1.03),
            ..Default::default()
        });
        for &text in &card_text {
            spec = spec.child(text);
        }
        cards.push(app.component(spec));
        texts.push(card_text);
    }

    // The essay panel, over the cards. It takes the pointer, so a click on
    // the open essay doesn't reach the cards under it, but not the keyboard,
    // so it's never a stop for focus.
    let left = -PANEL_SIZE.x / 2.0 + PANEL_PADDING;
    let top = PANEL_SIZE.y / 2.0 - PANEL_PADDING;
    let hidden_text = |app: &mut Proteus, x: f32, y: f32| {
        app.component(text_at(x, y, Text::new("", 16.0)).visible(false))
    };
    let essay = EssayView {
        back: app.component(
            button(
                Vec3::new(left + 85.0, top - 18.0, 0.0),
                Vec2::new(170.0, 36.0),
                "\u{2190} All essays",
            )
            .visible(false),
        ),
        title: hidden_text(app, left, top - 60.0),
        byline: hidden_text(app, left, top - 150.0),
        body: hidden_text(app, left, top - 200.0),
        quote: hidden_text(app, -300.0, -120.0),
        source: hidden_text(app, left, -PANEL_SIZE.y / 2.0 + PANEL_PADDING + 14.0),
    };
    let mut panel_spec = ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, PANEL_Y, 1.0),
        size: PANEL_SIZE,
        corner_radius: 18.0,
        color: PAPER,
        ..Default::default()
    })
    .visible(false)
    .devices(InputDevices::POINTER);
    for handle in essay.all() {
        panel_spec = panel_spec.child(handle);
    }
    let panel = app.component(panel_spec);

    let ui = Rc::new(Ui {
        cards,
        card_texts: texts,
        panel,
        essay,
        tools,
        channel: app.transition_channel(None),
        open: Cell::new(None),
        settings: Cell::new(Settings {
            size: 1,
            align: 0,
            bold: false,
            italic: false,
            spacing: 1,
            family: 0,
        }),
        crimson_wanted: Cell::new(false),
    });
    ui.apply(app, ui.settings.get());

    for (i, &card) in ui.cards.iter().enumerate() {
        let on_click = Rc::clone(&ui);
        card.on_click(app, move |app| on_click.open(app, i));
        // Back from the essay: the card's texts return once it's whole.
        let on_arrive = Rc::clone(&ui);
        card.on_transition_complete(app, move |app| {
            set_visible(app, &on_arrive.card_texts[i], true);
        });
    }
    let on_arrive = Rc::clone(&ui);
    panel.on_transition_complete(app, move |app| {
        set_visible(app, &on_arrive.essay.all(), true);
        set_visible(app, &on_arrive.tools.all(), true);
    });
    let on_back = Rc::clone(&ui);
    ui.essay.back.on_click(app, move |app| on_back.close(app));

    // Each button changes one setting.
    let on = |app: &mut Proteus, button: Handle, change: Box<dyn Fn(&mut Settings)>| {
        let ui = Rc::clone(&ui);
        button.on_click(app, move |app| ui.change(app, &change));
    };
    for (i, &button) in ui.tools.sizes.iter().enumerate() {
        on(app, button, Box::new(move |s| s.size = i));
    }
    for (i, &button) in ui.tools.aligns.iter().enumerate() {
        on(app, button, Box::new(move |s| s.align = i));
    }
    on(app, ui.tools.bold, Box::new(|s| s.bold = !s.bold));
    on(app, ui.tools.italic, Box::new(|s| s.italic = !s.italic));
    for (i, &button) in ui.tools.spacings.iter().enumerate() {
        on(app, button, Box::new(move |s| s.spacing = i));
    }
    for (i, &button) in ui.tools.families.iter().enumerate() {
        on(app, button, Box::new(move |s| s.family = i));
    }
    ui
}

/// Runs the app on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // Nothing is loaded up front. Assets are fetched from the example's
    // folder, a level above the page, as `fonts/` is.
    let services = proteus_host_web::PreloadedHostServices::fetch("..", &[]).await;
    proteus_host_web::run(Essays::default(), &canvas_id, config(), services).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `app` until every transition has ended.
    fn settle(app: &mut Proteus) {
        for _ in 0..4 {
            app.tick(0.0);
            app.tick(1.0);
        }
    }

    fn visible(app: &Proteus, handle: Handle) -> bool {
        app.get(handle).unwrap().visible
    }

    // Opening an essay shows its texts and the settings bar only once the
    // panel has arrived, and going back shows the card's texts once the card
    // has.
    #[test]
    fn texts_show_once_their_component_arrives() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        settle(&mut app);
        assert!(
            !visible(&app, ui.tools.all()[0]),
            "the bar is hidden with no essay open"
        );

        ui.open(&mut app, 1);
        app.tick(0.0);
        app.tick(0.1);
        assert!(
            !visible(&app, ui.essay.body),
            "hidden while the panel opens"
        );
        assert!(
            !visible(&app, ui.card_texts[1][0]),
            "the card's texts are hidden"
        );
        settle(&mut app);
        assert!(visible(&app, ui.essay.body));
        assert!(visible(&app, ui.panel));
        assert!(
            ui.tools.all().iter().all(|&h| visible(&app, h)),
            "the toolbar shows"
        );

        ui.close(&mut app);
        app.tick(0.0);
        app.tick(0.1);
        assert!(!visible(&app, ui.essay.body));
        assert!(
            !visible(&app, ui.tools.all()[0]),
            "the bar hides with the essay"
        );
        assert!(
            !visible(&app, ui.card_texts[1][0]),
            "hidden while the card returns"
        );
        settle(&mut app);
        assert!(visible(&app, ui.card_texts[1][0]));
        assert!(!visible(&app, ui.panel));
    }

    // The toolbar sets the open body's size, alignment, weight, italic, line
    // height and family.
    #[test]
    fn the_toolbar_sets_the_body() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        ui.open(&mut app, 0);
        ui.change(&mut app, |s| {
            s.size = 2;
            s.align = 1;
            s.bold = true;
            s.italic = true;
            s.spacing = 2;
        });
        let body = app.world().get::<Text>(ui.essay.body.id()).unwrap();
        assert_eq!(body.size_px, 20.0);
        assert_eq!(body.align, TextAlign::Center);
        assert_eq!(body.weight, Some(700));
        assert!(body.italic);
        assert_eq!(body.line_height, Some(1.8));
        assert_eq!(body.family.as_deref(), Some(LORA));
    }

    // A chosen or toggled-on button stays in the accent color under the
    // pointer; the others turn lighter, and a button toggled off does again.
    #[test]
    fn chosen_buttons_keep_their_color_under_the_pointer() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        ui.open(&mut app, 0);
        settle(&mut app);
        ui.change(&mut app, |s| {
            s.align = 2;
            s.bold = true;
        });
        settle(&mut app);
        let hovered = |app: &mut Proteus, h: Handle| {
            let at = app.get(h).unwrap().geometry.position.truncate();
            app.pointer_moved(Some(at));
            settle(app);
            app.get(h).unwrap().geometry.color
        };
        assert_eq!(hovered(&mut app, ui.tools.aligns[2]), ACCENT, "chosen");
        assert_eq!(hovered(&mut app, ui.tools.bold), ACCENT, "toggled on");
        assert_eq!(
            hovered(&mut app, ui.tools.aligns[0]),
            BUTTON_HOVER,
            "not chosen"
        );
        ui.change(&mut app, |s| s.bold = false);
        assert_eq!(
            hovered(&mut app, ui.tools.bold),
            BUTTON_HOVER,
            "toggled off"
        );
    }

    // Crimson Pro is downloaded only once it's chosen. Meanwhile the body is
    // drawn in the default font, and in Crimson Pro once it loads.
    #[test]
    fn crimson_pro_loads_once_chosen() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        assert!(!ui.crimson_wanted.get());
        assert_eq!(
            app.font_status(CRIMSON),
            Some(proteus_sdk::FontStatus::Expected(WhileLoading::DefaultFont))
        );
        ui.open(&mut app, 1);
        ui.change(&mut app, |s| s.family = 1);
        assert!(ui.crimson_wanted.get(), "update downloads it now");
        let body = app.world().get::<Text>(ui.essay.body.id()).unwrap();
        assert_eq!(body.family.as_deref(), Some(CRIMSON));

        let crimson = include_bytes!("../../fonts/CrimsonPro-Variable.ttf");
        assert_eq!(app.load_font(crimson.as_slice()).unwrap(), CRIMSON);
        assert_eq!(
            app.font_status(CRIMSON),
            Some(proteus_sdk::FontStatus::Loaded)
        );
    }

    // The title sits a margin from the window's left edge, and the toolbar
    // ends a margin from its right edge, centered on the title's line.
    #[test]
    fn the_toolbar_is_on_the_titles_line_at_the_right_margin() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        let last = app
            .get(*ui.tools.families.last().unwrap())
            .unwrap()
            .geometry;
        let right = last.position.x + last.size.x / 2.0;
        assert!((right - (550.0 - MARGIN)).abs() < 0.01, "{right}");
        assert_eq!(last.position.y, BAR_Y);
        let first = app.get(ui.tools.sizes[0]).unwrap().geometry;
        let left = first.position.x - first.size.x / 2.0;
        assert!(left > -550.0 + MARGIN + 200.0, "clear of the title: {left}");
    }
}
