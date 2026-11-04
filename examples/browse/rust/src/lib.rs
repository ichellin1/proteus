//! A browse screen, the same app as the TypeScript browse example.
//!
//! A TV-style screen, used with the arrow keys, Tab and Enter as well as the
//! pointer. A bar of genres, Drama, Action and Comedy, sits above two rows of
//! tiles, Films and Series, which run off the right edge and show the chosen
//! genre's titles. Enter on a tile, or a click, merges every tile into the
//! title's page, with a description, Play and Close. Play turns the page into
//! a full-screen video, as the genre bar fades out; Escape, Backspace or Delete
//! comes back to the page, and again, or Close, splits it back into the tiles.
//! Choosing another genre slides both rows back to their first tile.
//! Holding Enter on a tile for half a second favourites it.
//!
//! Techniques worth copying:
//!   - Focus groups, each made by giving a component the others are under a
//!     `FocusGroup`. The genre bar wraps Left and Right round, and focus
//!     entering it lands on the chosen genre, its default focus. Each row keeps
//!     Left and Right, lets Up and Down leave, and focus entering it from the
//!     bar or the other row lands on the nearest tile. The page keeps the
//!     arrows, and wraps Tab round.
//!   - Scrolling a row: a tile off screen can still take focus, as a member of
//!     the row focus is in, and its `on_focus` slides the row to show it.
//!   - The tiles become the page and back with a grid merge and split, 7 by 2,
//!     filled row by row, so each tile moves to its own part of the page. The
//!     page becomes the video and back through a transition channel.
//!   - Focus is put where it belongs from code: the app clears it as each
//!     transition starts, and gives it to Play, or to the tile the page opened
//!     from, as it ends.
//!   - The genre buttons are disabled while the page shows, so neither focus
//!     nor the pointer reaches them. They change color as a transition, and set
//!     `TransitionInteractionConfig` so they still take focus and clicks while
//!     it runs: a transitioning component otherwise doesn't.
//!   - The genre bar fades out for the video, its opacity eased frame by frame
//!     in `update`: opacity isn't part of a transition.
//!   - The focused style, with a glow and a border, shows only while the keys
//!     are in use, and the hover style only while the pointer is, so there's
//!     one highlight at a time. The glow rests at radius 0, so the focused
//!     style has one to grow.
//!   - A tap or a hold: each tile handles `Action::Activate` itself, so the
//!     key press doesn't click it, opens on a release that wasn't a long
//!     press, and favourites on the long press.
//!   - Back is the app's to define: its `on_key` steps back from the video to
//!     the page, and from the page to the tiles. Escape and Backspace are bound
//!     to it already, and the app binds Delete too, with `bind`.
//!   - The hint follows `input_mode()`, so it names the keys or the pointer,
//!     whichever is in use.
//!   - Text is drawn centered on its component, so a heading aligned left is
//!     sized to its text once the text is baked, with `baked_text_size` and
//!     `set_declared_geometry`; and a tile's title too wide for it is cut short
//!     with "…", measured the same way. Text is a single line, so the
//!     description is one component a line.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use proteus_runtime::{App, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    Action, Border, ComponentSpec, Easing, FocusEntry, FocusGroup, Glow, Handle, InputMode,
    KeyBinding, KeyResult, KeyState, MergeLayout, NamedKey, Proteus, QuadState, SplitStrategy,
    StyleOverride, Text, TransitionChannel, TransitionConfig, TransitionInteractionConfig,
};

const BACKGROUND: Vec4 = rgb(0x14131c);
const SURFACE: Vec4 = rgb(0x24222f);
const SURFACE_HOVER: Vec4 = rgb(0x302d3e);
const EDGE: Vec4 = rgb(0x3a3650);
const EDGE_HOVER: Vec4 = rgb(0x5c5778);
const ACCENT: Vec4 = rgb(0x8f74e0);
const GOLD: Vec4 = rgb(0xe0b33a);
const WHITE: Vec4 = rgb(0xffffff);
const DIM: Vec4 = rgb(0xa9a4bf);
const DISABLED: Vec4 = rgb(0x1c1a24);
const PAGE: Vec4 = rgb(0x1b1a25);
const VIDEO: Vec4 = rgb(0x050508);

const TILE: Vec2 = Vec2::new(220.0, 124.0);
/// From one tile's center to the next.
const TILE_STRIDE: f32 = 244.0;
const TILES_PER_ROW: usize = 7;
/// How close to the screen's edge a focused tile may come before its row
/// slides.
const EDGE_MARGIN: f32 = 40.0;
/// The space a tile's title keeps from the tile's edges, on each side.
const TITLE_PAD: f32 = 10.0;
/// The widest a tile's title may be: wider ones are cut short with "…".
const TITLE_MAX_WIDTH: f32 = TILE.x - 2.0 * TITLE_PAD;
const TITLE_SIZE: f32 = 17.0;

/// A genre, and the titles its rows show: films, then series.
struct Genre {
    name: &'static str,
    rows: [[&'static str; TILES_PER_ROW]; 2],
}

const GENRES: [Genre; 3] = [
    Genre {
        name: "Drama",
        rows: [
            [
                "The Shawshank Redemption",
                "The Godfather",
                "Forrest Gump",
                "Schindler's List",
                "Parasite",
                "Whiplash",
                "Good Will Hunting",
            ],
            [
                "Breaking Bad",
                "Succession",
                "The Wire",
                "The Sopranos",
                "Mad Men",
                "The Crown",
                "Chernobyl",
            ],
        ],
    },
    Genre {
        name: "Action",
        rows: [
            [
                "Mad Max: Fury Road",
                "The Dark Knight",
                "John Wick",
                "Die Hard",
                "Gladiator",
                "The Matrix",
                "Top Gun: Maverick",
            ],
            [
                "The Mandalorian",
                "The Boys",
                "Daredevil",
                "Reacher",
                "Vikings",
                "24",
                "Gangs of London",
            ],
        ],
    },
    Genre {
        name: "Comedy",
        rows: [
            [
                "Superbad",
                "The Hangover",
                "Anchorman: The Legend of Ron Burgundy",
                "Step Brothers",
                "Mean Girls",
                "Shaun of the Dead",
                "Knives Out",
            ],
            [
                "The Office (US)",
                "Ted Lasso",
                "Friends",
                "Parks and Recreation",
                "Brooklyn Nine-Nine",
                "Abbott Elementary",
                "Arrested Development",
            ],
        ],
    },
];

const ROW_NAMES: [&str; 2] = ["Films", "Series"];

/// An opaque color from its hex code, such as `0x8f74e0`.
const fn rgb(hex: u32) -> Vec4 {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    Vec4::new(r, g, b, 1.0)
}

const fn transition(duration: f32, delay: f32) -> TransitionConfig {
    TransitionConfig {
        duration,
        delay,
        easing: Easing::EaseOutCubic,
    }
}

/// A box of `size` centered at `(x, y)`.
fn quad(x: f32, y: f32, size: Vec2, color: Vec4, corner_radius: f32) -> QuadState {
    QuadState {
        position: Vec3::new(x, y, 0.0),
        size,
        color,
        corner_radius,
        ..Default::default()
    }
}

/// A transparent box that only holds text or other components.
fn holder(x: f32, y: f32, size: Vec2) -> ComponentSpec {
    ComponentSpec::new(quad(x, y, size, Vec4::ZERO, 0.0)).non_interactive()
}

fn label(text: &str, size: f32, color: Vec4) -> Text {
    Text::new(text, size).with_color(color)
}

/// The engine settings both entry points use. On the web, keys reach the
/// canvas without a click first, and Tab wraps round, as in a full-screen app;
/// without `keep_tab`, Tab would leave the canvas at either end.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config.input.autofocus = true;
    config.input.keep_tab = true;
    config
}

/// The browse app. It builds the screen in `setup`, and in `update` keeps the
/// hint in step with the input mode and fades the genre bar.
#[derive(Default)]
pub struct Browse {
    ui: Option<Rc<Ui>>,
}

impl App for Browse {
    fn setup(&mut self, f: &mut Frame) {
        self.ui = Some(build(f.proteus, f.viewport.logical_size));
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        if let Some(ui) = &self.ui {
            ui.half_width.set(f.viewport.logical_size.x / 2.0);
            ui.show_hint(f.proteus);
            ui.fit_headings(f.proteus);
            ui.fit_titles(f.proteus);
            ui.fade_bar(f.proteus, dt);
        }
    }
}

/// Which screen shows, or which change between two is under way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Tiles,
    ToPage,
    Page,
    ToTiles,
    ToVideo,
    Video,
    FromVideo,
}

/// How long the genre bar takes to fade out or in, in seconds.
const FADE_SECS: f32 = 0.3;

/// A row of tiles that slides to show the focused one.
struct Row {
    holder: Handle,
    tiles: Vec<Handle>,
    /// Its heading, left-aligned with the first tile once its text is baked.
    heading: Handle,
    heading_fitted: Cell<bool>,
    /// Where the row's holder is, across: its leftmost position, and now.
    home_x: f32,
    x: Cell<f32>,
}

/// What the callbacks share.
struct Ui {
    rows: Vec<Row>,
    /// Each tile's favourite mark, by row and tile.
    marks: Vec<Vec<Handle>>,
    /// The favourite titles, kept across genres.
    favourites: RefCell<HashSet<&'static str>>,
    /// How many of its title's characters each tile shows, by row and tile,
    /// while it's still being fitted; `None` once it fits.
    title_fits: RefCell<Vec<Vec<Option<usize>>>>,
    bar: Handle,
    genre_buttons: Vec<Handle>,
    /// The index in [`GENRES`] of the genre shown.
    genre: Cell<usize>,
    page: Handle,
    page_title: Handle,
    play: Handle,
    video: Handle,
    video_title: Handle,
    /// Between the page and the video.
    channel: TransitionChannel,
    hint: Handle,
    status: Handle,
    screen: Cell<Screen>,
    /// The tile the page opened from, by row and tile.
    opened: Cell<Option<(usize, usize)>>,
    /// The genre bar's opacity, as it fades.
    bar_opacity: Cell<f32>,
    /// What the hint says, so it's only changed when that does.
    hint_shown: Cell<Option<&'static str>>,
    half_width: Cell<f32>,
}

impl Ui {
    fn tile(&self, at: (usize, usize)) -> Handle {
        self.rows[at.0].tiles[at.1]
    }

    fn title(&self, (row, i): (usize, usize)) -> &'static str {
        GENRES[self.genre.get()].rows[row][i]
    }

    /// Every tile, films then series: the order the page's 7 by 2 grid fills
    /// in, row by row.
    fn all_tiles(&self) -> Vec<Handle> {
        self.rows
            .iter()
            .flat_map(|r| r.tiles.iter().copied())
            .collect()
    }

    /// Merges every tile into the page of the tile at `at`.
    fn open_page(&self, app: &mut Proteus, at: (usize, usize)) {
        if self.screen.get() != Screen::Tiles {
            return;
        }
        self.screen.set(Screen::ToPage);
        self.opened.set(Some(at));
        let title = self.title(at);
        // The page is hidden until the merge ends, so it comes in already
        // naming the title.
        let _ = self.page_title.set_text(app, label(title, 30.0, WHITE));
        let _ = self
            .video_title
            .set_text(app, label(&format!("Playing {title}"), 30.0, WHITE));
        for &button in &self.genre_buttons {
            let _ = button.set_disabled(app, true);
        }
        app.clear_focus();
        let _ = self.page.merge_from(
            app,
            &self.all_tiles(),
            transition(0.45, 0.0),
            MergeLayout::Grid {
                cols: TILES_PER_ROW,
                rows: 2,
            },
        );
    }

    /// Splits the page back into the tiles.
    fn close_page(&self, app: &mut Proteus) {
        if self.screen.get() != Screen::Page {
            return;
        }
        self.screen.set(Screen::ToTiles);
        app.clear_focus();
        let _ = self.page.split_to(
            app,
            &self.all_tiles(),
            transition(0.45, 0.0),
            SplitStrategy::Grid {
                cols: TILES_PER_ROW,
                rows: 2,
            },
        );
    }

    /// Turns the page into the video.
    fn play(&self, app: &mut Proteus) {
        if self.screen.get() != Screen::Page {
            return;
        }
        self.screen.set(Screen::ToVideo);
        app.clear_focus();
        self.channel
            .set(app, self.video, self.page, transition(0.5, 0.0), false);
    }

    /// Turns the video back into the page.
    fn stop(&self, app: &mut Proteus) {
        if self.screen.get() != Screen::Video {
            return;
        }
        self.screen.set(Screen::FromVideo);
        self.channel
            .set(app, self.page, self.video, transition(0.45, 0.0), false);
    }

    /// Steps back a screen: from the video to the page, or from the page to
    /// the tiles.
    fn back(&self, app: &mut Proteus) {
        match self.screen.get() {
            Screen::Video => self.stop(app),
            Screen::Page => self.close_page(app),
            _ => {}
        }
    }

    /// Finishes the change under way, as its transition ends: the page
    /// reports the end of the merge into it, the split out of it and the
    /// change back from the video, and the video the change into it.
    fn arrived(&self, app: &mut Proteus) {
        match self.screen.get() {
            Screen::ToPage | Screen::FromVideo => {
                self.screen.set(Screen::Page);
                let _ = self.play.focus(app);
            }
            Screen::ToTiles => {
                self.screen.set(Screen::Tiles);
                for &button in &self.genre_buttons {
                    let _ = button.set_disabled(app, false);
                }
                if let Some(at) = self.opened.get() {
                    let _ = self.tile(at).focus(app);
                }
            }
            Screen::ToVideo => self.screen.set(Screen::Video),
            _ => {}
        }
    }

    /// Shows genre `g`'s titles in the rows, sliding each back to where it
    /// starts, and marks its button as chosen, and as where focus entering the
    /// bar lands.
    fn show_genre(&self, app: &mut Proteus, g: usize) {
        self.genre.set(g);
        for row in &self.rows {
            self.slide(app, row, row.home_x, 0.5);
        }
        for (r, row) in self.rows.iter().enumerate() {
            for (i, &tile) in row.tiles.iter().enumerate() {
                let title = self.title((r, i));
                let _ = tile.set_text(app, label(title, TITLE_SIZE, WHITE));
                self.title_fits.borrow_mut()[r][i] = Some(title.chars().count());
                let favourite = self.favourites.borrow().contains(title);
                let _ = self.marks[r][i].set_visible(app, favourite);
            }
        }
        for (i, &button) in self.genre_buttons.iter().enumerate() {
            let _ = button.set_default_focus(app, i == g);
            let color = if i == g { ACCENT } else { SURFACE };
            if let Some(mut to) = button.declared_geometry(app) {
                to.color = color;
                let _ = button.animate_to(app, to, transition(0.25, 0.0));
            }
        }
    }

    /// Sizes each heading to its text once it's baked, with its left edge
    /// where its row's first tile starts: text is drawn centered on its
    /// component.
    fn fit_headings(&self, app: &mut Proteus) {
        for row in &self.rows {
            if row.heading_fitted.get() {
                continue;
            }
            let (Some(text), Some(mut to)) = (
                row.heading.baked_text_size(app),
                row.heading.declared_geometry(app),
            ) else {
                continue;
            };
            to.size.x = text.x;
            to.position.x = row.home_x - TILE.x / 2.0 + text.x / 2.0;
            let _ = row.heading.set_declared_geometry(app, to);
            row.heading_fitted.set(true);
        }
    }

    /// Cuts short, with "…", each tile title wider than the tile allows, once
    /// its text is baked: text is a single line, drawn at its own size. A
    /// title is measured again after each cut, so it may take a few frames.
    fn fit_titles(&self, app: &mut Proteus) {
        for (r, row) in self.rows.iter().enumerate() {
            for (i, &tile) in row.tiles.iter().enumerate() {
                let Some(shown) = self.title_fits.borrow()[r][i] else {
                    continue;
                };
                let Some(text) = tile.baked_text_size(app) else {
                    continue;
                };
                let next = shorter_title(shown, text.x);
                self.title_fits.borrow_mut()[r][i] = next;
                if let Some(n) = next {
                    let cut = cut_title(self.title((r, i)), n);
                    let _ = tile.set_text(app, label(&cut, TITLE_SIZE, WHITE));
                }
            }
        }
    }

    /// Eases the genre bar's opacity toward 0 while the video plays, or is
    /// about to, and toward 1 otherwise.
    fn fade_bar(&self, app: &mut Proteus, dt: f32) {
        let target = match self.screen.get() {
            Screen::ToVideo | Screen::Video => 0.0,
            _ => 1.0,
        };
        let now = self.bar_opacity.get();
        if now == target {
            return;
        }
        let step = dt / FADE_SECS;
        let next = if now < target {
            (now + step).min(target)
        } else {
            (now - step).max(target)
        };
        self.bar_opacity.set(next);
        let _ = self.bar.set_opacity(app, next);
    }

    fn toggle_favourite(&self, app: &mut Proteus, at: (usize, usize)) {
        let title = self.title(at);
        let now = {
            let mut favourites = self.favourites.borrow_mut();
            if !favourites.remove(title) {
                favourites.insert(title);
            }
            favourites.contains(title)
        };
        let _ = self.marks[at.0][at.1].set_visible(app, now);
        let message = if now {
            format!("{title} is a favourite")
        } else {
            format!("{title} isn't a favourite any more")
        };
        self.say(app, &message);
    }

    /// Slides `row` so its tile `i` is on screen: no further right than the
    /// screen's edge allows, and no further left than where the first tile
    /// rests, so that coming back to the first tile puts the row back.
    fn scroll_to(&self, app: &mut Proteus, row: usize, i: usize) {
        let row = &self.rows[row];
        let reach = self.half_width.get() - EDGE_MARGIN - TILE.x / 2.0;
        let tile_x = row.x.get() + i as f32 * TILE_STRIDE;
        let x = if tile_x > reach {
            row.x.get() - (tile_x - reach)
        } else if tile_x < row.home_x {
            row.x.get() + (row.home_x - tile_x)
        } else {
            return;
        };
        self.slide(app, row, x, 0.3);
    }

    /// Slides `row` across to `x`, over `secs` seconds, easing out.
    fn slide(&self, app: &mut Proteus, row: &Row, x: f32, secs: f32) {
        if row.x.get() == x {
            return;
        }
        row.x.set(x);
        let Some(mut to) = row.holder.declared_geometry(app) else {
            return;
        };
        to.position.x = x;
        let _ = row.holder.animate_to(app, to, transition(secs, 0.0));
    }

    fn say(&self, app: &mut Proteus, message: &str) {
        let _ = self.status.set_text(app, label(message, 16.0, GOLD));
    }

    /// Names the keys or the pointer in the hint, whichever is in use, for the
    /// screen that shows.
    fn show_hint(&self, app: &mut Proteus) {
        let keys = app.input_mode() == InputMode::Navigating;
        let text = match (self.screen.get(), keys) {
            (Screen::Tiles, true) => {
                "Arrows and Tab to move, Enter to open, hold Enter to favourite"
            }
            (Screen::Tiles, false) => "Click a tile to open it, or use the arrow keys",
            (Screen::Page, true) => {
                "Left and Right to choose, Enter to select, Escape, Backspace or Delete to go back"
            }
            (Screen::Page, false) => "Click Play or Close",
            _ => return,
        };
        if self.hint_shown.get() == Some(text) {
            return;
        }
        self.hint_shown.set(Some(text));
        let _ = self.hint.set_text(app, label(text, 16.0, DIM));
    }
}

/// How many characters to show of a title that shows `shown` of them and is
/// `width` wide, to fit within [`TITLE_MAX_WIDTH`]; `None` if it fits already.
/// A guess in proportion to the width, and always at least one fewer, so
/// fitting ends.
fn shorter_title(shown: usize, width: f32) -> Option<usize> {
    if width <= TITLE_MAX_WIDTH || shown <= 1 {
        return None;
    }
    let guess = (shown as f32 * TITLE_MAX_WIDTH / width) as usize;
    Some(guess.clamp(1, shown - 1))
}

/// The first `n` characters of `title`, without trailing spaces, and "…".
fn cut_title(title: &str, n: usize) -> String {
    let start: String = title.chars().take(n).collect();
    format!("{}…", start.trim_end())
}

/// The focused style: it grows, its border lights up and a glow spreads.
fn focused_style() -> StyleOverride {
    StyleOverride {
        scale: Some(1.06),
        border_color: Some(ACCENT),
        border_width: Some(3.0),
        glow_radius: Some(16.0),
        ..Default::default()
    }
}

/// The hover style: lighter, and a little larger.
fn hover_style() -> StyleOverride {
    StyleOverride {
        scale: Some(1.03),
        color: Some(SURFACE_HOVER),
        border_color: Some(EDGE_HOVER),
        ..Default::default()
    }
}

/// A button or tile at rest: a dim border, and a glow of radius 0 for the
/// focused style to grow.
fn control(geometry: QuadState) -> ComponentSpec {
    ComponentSpec::new(geometry)
        .border(Border::new(2.0, EDGE))
        .glow(Glow::new(0.0, ACCENT))
        .focused(focused_style())
        .hover(hover_style())
        .pressed(StyleOverride {
            scale: Some(0.97),
            ..Default::default()
        })
}

/// The description on every title's page.
const DESCRIPTION: [&str; 3] = [
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod",
    "tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim",
    "veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip.",
];

/// Creates the screen and its callbacks, which do the rest. `viewport` is
/// the viewport's size, for where the rows start and how big the video is.
fn build(app: &mut Proteus, viewport: Vec2) -> Rc<Ui> {
    let half = viewport.x / 2.0;
    app.component(holder(0.0, 310.0, Vec2::new(600.0, 40.0)).text(label("Browse", 28.0, WHITE)));

    // The genre bar: focus entering it lands on the chosen genre, which
    // `show_genre` marks as its default focus, and Left and Right wrap round.
    let genre_buttons: Vec<Handle> = GENRES
        .iter()
        .enumerate()
        .map(|(i, genre)| {
            let x = (i as f32 - 1.0) * 150.0;
            // Their color changes as a transition when the genre does; they
            // still take focus and clicks while it runs. Disabled while the
            // page shows.
            app.component(
                control(quad(x, 0.0, Vec2::new(130.0, 44.0), SURFACE, 22.0))
                    .text(label(genre.name, 16.0, WHITE))
                    .disabled(StyleOverride {
                        color: Some(DISABLED),
                        border_color: Some(EDGE),
                        ..Default::default()
                    })
                    .transition_interaction(TransitionInteractionConfig {
                        allow_pointer: true,
                        allow_navigation: true,
                    }),
            )
        })
        .collect();
    let mut bar = holder(0.0, 250.0, Vec2::new(460.0, 60.0)).focus_group(
        FocusGroup::new()
            .wrap(&[Action::Left, Action::Right])
            .entry(FocusEntry::Default),
    );
    for b in &genre_buttons {
        bar = bar.child(*b);
    }
    let bar = app.component(bar);

    // The rows: Left and Right stay in a row, Up and Down leave it, and focus
    // coming from the bar or the other row lands on the nearest tile.
    let home_x = -half + EDGE_MARGIN + 40.0 + TILE.x / 2.0;
    let mut rows = Vec::new();
    let mut marks = Vec::new();
    for (r, (name, y)) in ROW_NAMES.into_iter().zip([90.0, -120.0]).enumerate() {
        // Sized to its text in `fit_headings`, once the text is baked.
        let heading = app.component(
            holder(home_x, y + TILE.y / 2.0 + 22.0, Vec2::new(TILE.x, 30.0))
                .text(label(name, 18.0, DIM)),
        );
        let mut row_marks = Vec::new();
        let tiles: Vec<Handle> = GENRES[0].rows[r]
            .iter()
            .enumerate()
            .map(|(i, title)| {
                // A gold dot in the corner marks a favourite.
                let mark = app.component(
                    ComponentSpec::new(quad(
                        TILE.x / 2.0 - 18.0,
                        TILE.y / 2.0 - 18.0,
                        Vec2::splat(12.0),
                        GOLD,
                        6.0,
                    ))
                    .non_interactive()
                    .visible(false),
                );
                row_marks.push(mark);
                app.component(
                    control(quad(i as f32 * TILE_STRIDE, 0.0, TILE, SURFACE, 14.0))
                        .text(label(title, TITLE_SIZE, WHITE))
                        .child(mark),
                )
            })
            .collect();
        let mut row = holder(home_x, y, Vec2::new(TILE_STRIDE, TILE.y)).focus_group(
            FocusGroup::new()
                .stop(&[Action::Left, Action::Right])
                .entry(FocusEntry::Nearest),
        );
        for t in &tiles {
            row = row.child(*t);
        }
        rows.push(Row {
            holder: app.component(row),
            tiles,
            heading,
            heading_fitted: Cell::new(false),
            home_x,
            x: Cell::new(home_x),
        });
        marks.push(row_marks);
    }

    // The page: the title, its description, and Play and Close. Hidden until
    // the tiles merge into it. The arrows stay in it, and Tab wraps round.
    let page_size = Vec2::new(viewport.x - 2.0 * EDGE_MARGIN, 420.0);
    let page_title = app.component(holder(0.0, 150.0, Vec2::new(page_size.x, 40.0)));
    let description: Vec<Handle> = DESCRIPTION
        .iter()
        .enumerate()
        .map(|(i, line)| {
            app.component(
                holder(0.0, 85.0 - i as f32 * 28.0, Vec2::new(page_size.x, 28.0))
                    .text(label(line, 17.0, DIM)),
            )
        })
        .collect();
    let play = app.component(
        control(quad(-110.0, -110.0, Vec2::new(200.0, 56.0), ACCENT, 28.0))
            .text(label("Play", 18.0, WHITE)),
    );
    let close = app.component(
        control(quad(110.0, -110.0, Vec2::new(200.0, 56.0), SURFACE, 28.0))
            .text(label("Close", 18.0, WHITE)),
    );
    let mut page = ComponentSpec::new(quad(0.0, -15.0, page_size, PAGE, 24.0))
        .border(Border::new(2.0, EDGE))
        .non_interactive()
        .visible(false)
        .child(page_title)
        .child(play)
        .child(close)
        .focus_group(
            FocusGroup::new()
                .stop(&[Action::Up, Action::Down, Action::Left, Action::Right])
                .wrap(&[Action::Next, Action::Previous]),
        );
    for line in &description {
        page = page.child(*line);
    }
    let page = app.component(page);

    let hint = app.component(holder(0.0, -270.0, Vec2::new(900.0, 30.0)));
    let status = app.component(holder(0.0, -305.0, Vec2::new(900.0, 30.0)));

    // The video, over everything: the page turns into it.
    let video_title = app.component(holder(0.0, 20.0, Vec2::new(viewport.x, 40.0)));
    let video_hint = app.component(holder(0.0, -30.0, Vec2::new(viewport.x, 30.0)).text(label(
        "Escape, Backspace or Delete to go back",
        16.0,
        DIM,
    )));
    let video = app.component(
        ComponentSpec::new(quad(0.0, 0.0, viewport, VIDEO, 0.0))
            .non_interactive()
            .visible(false)
            .child(video_title)
            .child(video_hint),
    );
    let channel = app.transition_channel(None);

    let ui = Rc::new(Ui {
        favourites: RefCell::new(HashSet::new()),
        title_fits: RefCell::new(vec![vec![None; TILES_PER_ROW]; 2]),
        rows,
        marks,
        bar,
        genre_buttons,
        genre: Cell::new(0),
        page,
        page_title,
        play,
        video,
        video_title,
        channel,
        hint,
        status,
        screen: Cell::new(Screen::Tiles),
        opened: Cell::new(None),
        bar_opacity: Cell::new(1.0),
        hint_shown: Cell::new(None),
        half_width: Cell::new(half),
    });

    for (g, &button) in ui.genre_buttons.iter().enumerate() {
        let u = Rc::clone(&ui);
        button.on_click(app, move |app| {
            u.show_genre(app, g);
            u.say(app, &format!("Showing {}", GENRES[g].name));
        });
    }
    ui.show_genre(app, 0);

    for (r, row) in ui.rows.iter().enumerate() {
        for (i, &tile) in row.tiles.iter().enumerate() {
            let at = (r, i);
            let u = Rc::clone(&ui);
            tile.on_click(app, move |app| u.open_page(app, at));
            let u = Rc::clone(&ui);
            tile.on_focus(app, move |app| u.scroll_to(app, r, i));
            // A tap opens the tile, and a hold favourites it. Handling
            // Activate here stops its press from clicking the tile, so it opens
            // on the release, once it's known not to be a hold.
            let u = Rc::clone(&ui);
            tile.on_key(app, move |app, e| {
                if e.action != Some(Action::Activate) {
                    return KeyResult::Ignored;
                }
                match e.state {
                    KeyState::LongPressed => u.toggle_favourite(app, at),
                    KeyState::Released if !e.after_long_press => u.open_page(app, at),
                    _ => {}
                }
                KeyResult::Handled
            });
        }
    }

    let u = Rc::clone(&ui);
    play.on_click(app, move |app| u.play(app));
    let u = Rc::clone(&ui);
    close.on_click(app, move |app| u.close_page(app));
    let u = Rc::clone(&ui);
    page.on_transition_complete(app, move |app| u.arrived(app));
    let u = Rc::clone(&ui);
    video.on_transition_complete(app, move |app| u.arrived(app));

    // Escape and Backspace are bound to Back already; the app binds Delete
    // too. Back means whatever the app says: here, a step back from the video
    // or the page.
    app.bind(KeyBinding::named(NamedKey::Delete), Action::Back);
    let u = Rc::clone(&ui);
    app.on_key(move |app, e| {
        if e.action == Some(Action::Back) && e.state == KeyState::Pressed {
            u.back(app);
            return KeyResult::Handled;
        }
        KeyResult::Ignored
    });

    // Focus starts on the first film; it shows once the keys are used.
    let _ = ui.rows[0].tiles[0].focus(app);
    ui
}

/// Runs the browse screen on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // The browse screen loads no assets.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Browse::default(), &canvas_id, config(), services).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use proteus_sdk::{Code, Key, KeyEvent, Modifiers, NamedKey};

    fn settle(app: &mut Proteus) {
        for _ in 0..4 {
            app.tick(0.5);
        }
    }

    fn press(app: &mut Proteus, key: NamedKey, code: Code, hold: f32) {
        let event = |state| {
            KeyEvent::new(
                Key::Named(key),
                code.clone(),
                state,
                Modifiers::default(),
                None,
            )
        };
        app.key_event(event(KeyState::Pressed));
        app.tick(0.016);
        let mut held = 0.0;
        while held < hold {
            app.tick(0.05);
            held += 0.05;
        }
        app.key_event(event(KeyState::Released));
        app.tick(0.016);
    }

    fn arrow(app: &mut Proteus, key: NamedKey) {
        let code = match key {
            NamedKey::ArrowUp => Code::ArrowUp,
            NamedKey::ArrowDown => Code::ArrowDown,
            NamedKey::ArrowLeft => Code::ArrowLeft,
            _ => Code::ArrowRight,
        };
        press(app, key, code, 0.0);
    }

    fn enter(app: &mut Proteus) {
        press(app, NamedKey::Enter, Code::Enter, 0.0);
    }

    fn escape(app: &mut Proteus) {
        press(app, NamedKey::Escape, Code::Escape, 0.0);
    }

    fn browse() -> (Proteus, Rc<Ui>) {
        let mut app = Proteus::new();
        app.set_viewport_size(Vec2::new(1100.0, 720.0));
        let ui = build(&mut app, Vec2::new(1100.0, 720.0));
        app.tick(0.0);
        // The first key only shows focus, on the first film.
        arrow(&mut app, NamedKey::ArrowRight);
        assert_eq!(app.focused(), Some(ui.rows[0].tiles[0]));
        (app, ui)
    }

    // Enter merges the tiles into the page, with focus on Play and the genres
    // out of reach, and Escape splits it back, with focus on the tile again.
    #[test]
    fn enter_opens_the_page_and_escape_closes_it() {
        let (mut app, ui) = browse();
        arrow(&mut app, NamedKey::ArrowRight);
        let tile = ui.rows[0].tiles[1];
        enter(&mut app);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Page);
        assert_eq!(app.focused(), Some(ui.play));
        assert!(ui.all_tiles().iter().all(|t| !app.get(*t).unwrap().visible));
        assert!(ui
            .genre_buttons
            .iter()
            .all(|b| app.get(*b).unwrap().disabled));
        arrow(&mut app, NamedKey::ArrowUp);
        assert_eq!(app.focused(), Some(ui.play), "the page keeps the arrows");

        escape(&mut app);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Tiles);
        assert_eq!(app.focused(), Some(tile));
        assert!(ui.all_tiles().iter().all(|t| app.get(*t).unwrap().visible));
        assert!(ui
            .genre_buttons
            .iter()
            .all(|b| !app.get(*b).unwrap().disabled));
    }

    // Play turns the page into the video as the genre bar fades out, and
    // Escape comes back to the page as it fades in.
    #[test]
    fn play_shows_the_video_and_escape_comes_back_to_the_page() {
        let (mut app, ui) = browse();
        enter(&mut app);
        settle(&mut app);
        enter(&mut app);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Video);
        assert!(app.get(ui.video).unwrap().visible);
        for _ in 0..30 {
            ui.fade_bar(&mut app, 0.016);
        }
        assert_eq!(ui.bar_opacity.get(), 0.0);

        escape(&mut app);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Page);
        assert_eq!(app.focused(), Some(ui.play));
        for _ in 0..30 {
            ui.fade_bar(&mut app, 0.016);
        }
        assert_eq!(ui.bar_opacity.get(), 1.0);
    }

    // A title too wide for its tile is cut by a guess in proportion, then a
    // character at a time as it's measured again, until it fits.
    #[test]
    fn a_title_too_wide_is_cut_short_until_it_fits() {
        assert_eq!(shorter_title(24, 180.0), None, "it fits");
        assert_eq!(shorter_title(24, 249.0), Some(19));
        assert_eq!(shorter_title(19, 205.0), Some(18), "at least one fewer");
        assert_eq!(shorter_title(1, 500.0), None, "nothing left to cut");
        assert_eq!(cut_title("The Shawshank Redemption", 13), "The Shawshank…");
        assert_eq!(cut_title("The Shawshank Redemption", 14), "The Shawshank…");
    }

    #[test]
    fn holding_enter_favourites_a_tile_without_opening_it() {
        let (mut app, ui) = browse();
        press(&mut app, NamedKey::Enter, Code::Enter, 0.7);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Tiles);
        assert!(ui.favourites.borrow().contains(GENRES[0].rows[0][0]));
    }

    // Up into the genre bar lands on the chosen genre, wherever the tile is,
    // and Down goes to the nearest film.
    #[test]
    fn the_bar_takes_focus_at_the_chosen_genre_and_gives_it_to_the_nearest_film() {
        let (mut app, ui) = browse();
        for _ in 0..4 {
            arrow(&mut app, NamedKey::ArrowRight);
            settle(&mut app);
        }
        assert_eq!(app.focused(), Some(ui.rows[0].tiles[4]));
        arrow(&mut app, NamedKey::ArrowUp);
        assert_eq!(app.focused(), Some(ui.genre_buttons[0]), "Drama, chosen");
        // The row has slid, so the film below Drama is the third.
        arrow(&mut app, NamedKey::ArrowDown);
        assert_eq!(app.focused(), Some(ui.rows[0].tiles[2]));
    }

    // Between the rows, focus goes to the nearest tile, not the one focused
    // last in the other row.
    #[test]
    fn between_rows_focus_goes_to_the_nearest_tile() {
        let (mut app, ui) = browse();
        arrow(&mut app, NamedKey::ArrowDown);
        assert_eq!(app.focused(), Some(ui.rows[1].tiles[0]));
        arrow(&mut app, NamedKey::ArrowUp);
        arrow(&mut app, NamedKey::ArrowRight);
        arrow(&mut app, NamedKey::ArrowRight);
        arrow(&mut app, NamedKey::ArrowDown);
        assert_eq!(app.focused(), Some(ui.rows[1].tiles[2]));
    }

    // A favourite is a title: it isn't marked in another genre, and is again
    // when its genre comes back.
    #[test]
    fn a_genre_shows_its_titles_and_keeps_its_favourites() {
        let (mut app, ui) = browse();
        press(&mut app, NamedKey::Enter, Code::Enter, 0.7);
        let mark = ui.marks[0][0];
        settle(&mut app);
        assert!(app.get(mark).unwrap().visible);

        // Up to the genre bar, at Drama, the chosen genre, then Right to Action.
        arrow(&mut app, NamedKey::ArrowUp);
        arrow(&mut app, NamedKey::ArrowRight);
        enter(&mut app);
        settle(&mut app);
        assert_eq!(ui.genre.get(), 1);
        assert!(!app.get(mark).unwrap().visible);

        arrow(&mut app, NamedKey::ArrowLeft);
        enter(&mut app);
        settle(&mut app);
        assert_eq!(ui.genre.get(), 0);
        assert!(app.get(mark).unwrap().visible);
    }

    // A new genre starts both rows from their first tile, sliding back a row
    // that had slid.
    #[test]
    fn a_new_genre_slides_both_rows_back_to_the_start() {
        let (mut app, ui) = browse();
        for _ in 0..5 {
            arrow(&mut app, NamedKey::ArrowRight);
            settle(&mut app);
        }
        assert!(ui.rows[0].x.get() < ui.rows[0].home_x, "the row slid");
        arrow(&mut app, NamedKey::ArrowUp);
        arrow(&mut app, NamedKey::ArrowRight);
        enter(&mut app);
        settle(&mut app);
        for row in &ui.rows {
            assert_eq!(row.x.get(), row.home_x);
            let at = app.get(row.holder).unwrap().geometry.position.x;
            assert_eq!(at, row.home_x);
        }
    }

    // Delete steps back as Escape and Backspace do: the app binds it to Back.
    #[test]
    fn delete_steps_back_too() {
        let (mut app, ui) = browse();
        enter(&mut app);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Page);
        press(&mut app, NamedKey::Delete, Code::Delete, 0.0);
        settle(&mut app);
        assert_eq!(ui.screen.get(), Screen::Tiles);
    }

    // After the row slides left, coming back to the first tile slides it all
    // the way back. A row that slid only a little used to stay shifted, since
    // its first tile was already in view.
    #[test]
    fn coming_back_to_the_first_tile_puts_the_row_back() {
        let (mut app, ui) = browse();
        for _ in 0..4 {
            arrow(&mut app, NamedKey::ArrowRight);
            settle(&mut app);
        }
        assert!(ui.rows[0].x.get() < ui.rows[0].home_x, "the row slid");
        for _ in 0..4 {
            arrow(&mut app, NamedKey::ArrowLeft);
            settle(&mut app);
        }
        assert_eq!(app.focused(), Some(ui.rows[0].tiles[0]));
        assert_eq!(ui.rows[0].x.get(), ui.rows[0].home_x);
        let at = app.get(ui.rows[0].holder).unwrap().geometry.position.x;
        assert_eq!(at, ui.rows[0].home_x);
    }

    // The row's last tiles start off screen; moving to them slides the row.
    #[test]
    fn the_row_slides_to_show_a_tile_off_screen() {
        let (mut app, ui) = browse();
        for _ in 0..TILES_PER_ROW - 1 {
            arrow(&mut app, NamedKey::ArrowRight);
            settle(&mut app);
        }
        assert_eq!(app.focused(), Some(ui.rows[0].tiles[TILES_PER_ROW - 1]));
        assert!(ui.rows[0].x.get() < ui.rows[0].home_x);
        arrow(&mut app, NamedKey::ArrowRight);
        assert_eq!(
            app.focused(),
            Some(ui.rows[0].tiles[TILES_PER_ROW - 1]),
            "Right stops at the row's end"
        );
    }
}
