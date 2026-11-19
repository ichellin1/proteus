//! A reading app, the same app as the TypeScript essays example.
//!
//! A row of cards, one for each of three public-domain essays. Each card's
//! year is aligned to the end of its column; its title wraps onto at most two
//! lines, ending in "…"; its opening is spaced at a line height of 1.45 and
//! stops after five lines, ending in an ellipsis of the card's own; and its
//! source is clipped after two lines. A card opens into its essay (1→1), whose
//! byline is two lines split with a `\n`, whose opening wraps at the column's
//! width, and whose pull quote is centered. While an essay is open, the
//! buttons at the top change its alignment and line height.
//!
//! The essays are Mary Wollstonecraft's "A Vindication of the Rights of
//! Woman" (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis
//! Stevenson's "An Apology for Idlers" (1877), from their Project Gutenberg
//! editions, each cited where it's shown.
//!
//! Techniques worth copying:
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
//! Not shown here: fonts and weights, other scripts, mixed styles within a
//! text, measuring text, and components within a line of text.

use std::cell::Cell;
use std::rc::Rc;

use proteus_runtime::{App, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    ComponentSpec, Easing, Handle, InputDevices, Proteus, QuadState, StyleOverride, Text,
    TextAlign, TextOverflow, TransitionChannel, TransitionConfig,
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

const CARD_SIZE: Vec2 = Vec2::new(320.0, 300.0);
const CARD_Y: f32 = -20.0;
const CARD_SPACING: f32 = 350.0;
const CARD_PADDING: f32 = 20.0;
const CARD_COLUMN: f32 = CARD_SIZE.x - 2.0 * CARD_PADDING;

const PANEL_SIZE: Vec2 = Vec2::new(960.0, 600.0);
const PANEL_Y: f32 = -40.0;
const PANEL_PADDING: f32 = 40.0;
const PANEL_COLUMN: f32 = PANEL_SIZE.x - 2.0 * PANEL_PADDING;

/// How far the title sits from the window's left edge, and the settings bar
/// from its right edge.
const MARGIN: f32 = 40.0;
const TITLE_TOP: f32 = 362.0;
/// The middle of the title's line, where the settings bar is centered.
const BAR_Y: f32 = TITLE_TOP - 19.0;
const SETTING_SIZE: Vec2 = Vec2::new(64.0, 30.0);
/// From one setting's button to the next.
const SETTING_STEP: f32 = SETTING_SIZE.x + 6.0;
/// The widths of the settings' labels, and the gap between the two settings.
const ALIGN_LABEL: f32 = 50.0;
const LINE_HEIGHT_LABEL: f32 = 90.0;
const SETTINGS_GAP: f32 = 24.0;
/// The settings bar's width: each label and its three buttons, and the gap.
const BAR_WIDTH: f32 =
    ALIGN_LABEL + LINE_HEIGHT_LABEL + 2.0 * (2.0 * SETTING_STEP + SETTING_SIZE.x) + SETTINGS_GAP;

/// The alignments the body can take, with their buttons' labels.
const ALIGNS: [(&str, TextAlign); 3] = [
    ("Start", TextAlign::Start),
    ("Center", TextAlign::Center),
    ("End", TextAlign::End),
];
/// The line heights the body can take.
const LINE_HEIGHTS: [f32; 3] = [1.2, 1.5, 1.8];

/// The engine settings both entry points use.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config.input.autofocus = true;
    config
}

/// The essays app. It does all its work in callbacks, set up in `setup`.
#[derive(Default)]
pub struct Essays;

impl App for Essays {
    fn setup(&mut self, f: &mut Frame) {
        build(f.proteus, f.viewport.logical_size.x);
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
                .with_color(MUTED)
                .with_wrap_width(CARD_COLUMN)
                .with_align(TextAlign::End),
        ),
        // At most two lines, ending in the default "…".
        text_at(
            left,
            top - 24.0,
            Text::new(essay.title, 22.0)
                .with_color(INK)
                .with_wrap_width(CARD_COLUMN)
                .with_max_lines(2),
        ),
        // At most five lines, ending in the card's own ellipsis.
        text_at(
            left,
            top - 86.0,
            Text::new(essay.opening, 15.0)
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

/// How the body is set: the settings bar's choices.
#[derive(Clone, Copy, PartialEq)]
struct Settings {
    align: usize,
    line_height: usize,
}

/// What the callbacks share.
struct Ui {
    cards: Vec<Handle>,
    card_texts: Vec<Vec<Handle>>,
    panel: Handle,
    essay: EssayView,
    /// The settings bar's labels and buttons, shown while an essay is open.
    bar: Vec<Handle>,
    align_buttons: Vec<Handle>,
    line_height_buttons: Vec<Handle>,
    channel: TransitionChannel,
    /// The essay open, or opening.
    open: Cell<Option<usize>>,
    settings: Cell<Settings>,
}

impl Ui {
    /// The opening of essay `i`, set as the settings bar says.
    fn body(&self, i: usize) -> Text {
        let settings = self.settings.get();
        Text::new(ESSAYS[i].opening, 17.0)
            .with_color(INK)
            .with_wrap_width(PANEL_COLUMN)
            .with_align(ALIGNS[settings.align].1)
            .with_line_height(LINE_HEIGHTS[settings.line_height])
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
                .with_color(INK)
                .with_wrap_width(PANEL_COLUMN),
        );
        // Two lines, split with a `\n`.
        let byline = format!("{}\n{}", essay.author, essay.year);
        let _ = view
            .byline
            .set_text(app, Text::new(byline, 14.0).with_color(MUTED));
        let _ = view.body.set_text(app, self.body(i));
        let _ = view.quote.set_text(
            app,
            Text::new(essay.quote, 20.0)
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
        set_visible(app, &self.bar, false);
        self.channel.set(app, self.cards[i], self.panel, OPEN, true);
    }

    /// Applies the settings bar's choices to its buttons and the open body.
    fn apply(&self, app: &mut Proteus, settings: Settings) {
        self.settings.set(settings);
        select(app, &self.align_buttons, settings.align);
        select(app, &self.line_height_buttons, settings.line_height);
        if let Some(i) = self.open.get() {
            let _ = self.essay.body.set_text(app, self.body(i));
        }
    }
}

fn set_visible(app: &mut Proteus, handles: &[Handle], visible: bool) {
    for handle in handles {
        let _ = handle.set_visible(app, visible);
    }
}

/// A setting's button: shown in the accent color, and left alone by the
/// pointer, while disabled, which is how the chosen one is marked.
fn setting_button(position: Vec3, label: &str) -> ComponentSpec {
    button(position, SETTING_SIZE, label).disabled(StyleOverride {
        color: Some(ACCENT),
        ..Default::default()
    })
}

/// Marks the `chosen` button of a row by disabling it, so it shows the accent
/// color, and hovering or clicking it does nothing, since it's chosen
/// already. The rest are enabled.
fn select(app: &mut Proteus, buttons: &[Handle], chosen: usize) {
    for (i, button) in buttons.iter().enumerate() {
        let _ = button.set_disabled(app, i == chosen);
    }
}

/// Creates the app's components, for a window `width` logical pixels wide,
/// and its callbacks, which do the rest.
fn build(app: &mut Proteus, width: f32) -> Rc<Ui> {
    // The title sits a margin from the window's left edge, and the settings
    // bar, on the title's line, a margin from its right edge.
    app.component(text_at(
        -width / 2.0 + MARGIN,
        TITLE_TOP,
        Text::new("Essays", 30.0).with_color(WHITE),
    ));

    // The settings bar: a label, then its buttons, for each setting. Hidden
    // until an essay is open, since it changes only the essay.
    let mut bar = Vec::new();
    let mut row = |app: &mut Proteus, label: &str, x: f32, label_width: f32, labels: &[String]| {
        bar.push(
            app.component(
                text_at(
                    x,
                    BAR_Y + 9.0,
                    Text::new(label, 14.0)
                        .with_color(WHITE)
                        .with_wrap_width(label_width),
                )
                .visible(false),
            ),
        );
        let buttons: Vec<Handle> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let center = x + label_width + SETTING_SIZE.x / 2.0 + i as f32 * SETTING_STEP;
                let position = Vec3::new(center, BAR_Y, 0.0);
                app.component(setting_button(position, label).visible(false))
            })
            .collect();
        bar.extend(&buttons);
        buttons
    };
    let align_labels: Vec<String> = ALIGNS.iter().map(|(l, _)| l.to_string()).collect();
    let bar_left = width / 2.0 - MARGIN - BAR_WIDTH;
    let align_buttons = row(app, "Align", bar_left, ALIGN_LABEL, &align_labels);
    let height_labels: Vec<String> = LINE_HEIGHTS.iter().map(|h| h.to_string()).collect();
    let after_align = bar_left + ALIGN_LABEL + 2.0 * SETTING_STEP + SETTING_SIZE.x + SETTINGS_GAP;
    let line_height_buttons = row(
        app,
        "Line height",
        after_align,
        LINE_HEIGHT_LABEL,
        &height_labels,
    );

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
        bar,
        align_buttons,
        line_height_buttons,
        channel: app.transition_channel(None),
        open: Cell::new(None),
        settings: Cell::new(Settings {
            align: 0,
            line_height: 1,
        }),
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
        set_visible(app, &on_arrive.bar, true);
    });
    let on_back = Rc::clone(&ui);
    ui.essay.back.on_click(app, move |app| on_back.close(app));

    for (i, &button) in ui.align_buttons.iter().enumerate() {
        let on_click = Rc::clone(&ui);
        button.on_click(app, move |app| {
            let settings = Settings {
                align: i,
                ..on_click.settings.get()
            };
            on_click.apply(app, settings);
        });
    }
    for (i, &button) in ui.line_height_buttons.iter().enumerate() {
        let on_click = Rc::clone(&ui);
        button.on_click(app, move |app| {
            let settings = Settings {
                line_height: i,
                ..on_click.settings.get()
            };
            on_click.apply(app, settings);
        });
    }
    ui
}

/// Runs the app on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // The app loads no assets.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Essays, &canvas_id, config(), services).await
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
            !visible(&app, ui.bar[0]),
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
        assert!(ui.bar.iter().all(|&h| visible(&app, h)), "the bar shows");

        ui.close(&mut app);
        app.tick(0.0);
        app.tick(0.1);
        assert!(!visible(&app, ui.essay.body));
        assert!(!visible(&app, ui.bar[0]), "the bar hides with the essay");
        assert!(
            !visible(&app, ui.card_texts[1][0]),
            "hidden while the card returns"
        );
        settle(&mut app);
        assert!(visible(&app, ui.card_texts[1][0]));
        assert!(!visible(&app, ui.panel));
    }

    // The settings bar sets the open body's alignment and line height, and
    // marks the chosen buttons by disabling them, so hovering them changes
    // nothing.
    #[test]
    fn the_settings_set_the_body() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        ui.open(&mut app, 0);
        ui.apply(
            &mut app,
            Settings {
                align: 1,
                line_height: 2,
            },
        );
        let body = app.world().get::<Text>(ui.essay.body.id()).unwrap();
        assert_eq!(body.align, TextAlign::Center);
        assert_eq!(body.line_height, Some(1.8));
        let disabled = |h: &Handle| app.get(*h).unwrap().disabled;
        assert_eq!(
            ui.align_buttons.iter().map(disabled).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(
            ui.line_height_buttons
                .iter()
                .map(disabled)
                .collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    // The title sits a margin from the window's left edge, and the settings
    // bar ends a margin from its right edge, centered on the title's line.
    #[test]
    fn the_bar_is_on_the_titles_line_at_the_right_margin() {
        let mut app = Proteus::new();
        let ui = build(&mut app, 1100.0);
        let last = app.get(*ui.bar.last().unwrap()).unwrap().geometry;
        let right = last.position.x + last.size.x / 2.0;
        assert!((right - (550.0 - MARGIN)).abs() < 0.01, "{right}");
        assert_eq!(last.position.y, BAR_Y);
        let label = app.get(ui.bar[0]).unwrap().geometry;
        assert!((label.position.x - (550.0 - MARGIN - BAR_WIDTH)).abs() < 0.01);
    }
}
