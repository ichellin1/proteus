//! A menu, the same app as the TypeScript menu example.
//!
//! A theme picker. Clicking the button splits it into a column of menu items
//! (1→N). Picking a theme merges the items back into the button (N→1), which
//! now names the new theme, and the preview card above fades to its color.
//! Clicking anywhere else while the menu is open merges it back unchanged.
//! Nothing is opened or closed: the button becomes the menu, and the menu
//! becomes the button.
//!
//! Techniques worth copying:
//!   - Create the menu items once, hidden, and reuse them: a split reveals its
//!     targets when it ends, and a merge hides its sources when it starts.
//!   - Change the button while it's hidden during the menu, with `set_text`,
//!     so it comes back from the merge already naming the new choice.
//!   - Track whether the menu is opening, open, closing or closed, and ignore
//!     clicks that don't fit. The button's `on_transition_complete` reports
//!     the end of both: a split reports on its source, and a merge on its
//!     destination.
//!   - Interaction styles: the items grow on hover and shrink while pressed,
//!     and the disabled item is gray. A disabled component is still there,
//!     blocking the pointer, but its callbacks don't run.
//!   - A transparent component behind the menu, shown only while it's open,
//!     catches a click on empty space.
//!   - The callbacks only change components, so they share the handles and
//!     the menu's state in an `Rc` and do all the work themselves.

use std::cell::Cell;
use std::rc::Rc;

use proteus_runtime::{App, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    ComponentSpec, Easing, Handle, MergeLayout, Proteus, QuadState, SplitStrategy, StyleOverride,
    Text, TransitionConfig,
};

const BACKGROUND: Vec4 = rgb(0x1d1b26);
const BUTTON: Vec4 = rgb(0x2d2a3a);
const BUTTON_HOVER: Vec4 = rgb(0x3d394f);
const WHITE: Vec4 = rgb(0xffffff);
const GRAY: Vec4 = rgb(0x4a4756);

const ITEM_SIZE: Vec2 = Vec2::new(260.0, 52.0);
const ITEM_GAP: f32 = 8.0;
/// Where the button is, and where the first menu item goes.
const BUTTON_Y: f32 = -20.0;

/// An opaque color from its hex code, such as `0x7a5fb0`.
const fn rgb(hex: u32) -> Vec4 {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    Vec4::new(r, g, b, 1.0)
}

struct Theme {
    name: &'static str,
    color: Vec4,
    /// Whether the theme can't be picked yet.
    soon: bool,
}

const THEMES: [Theme; 5] = [
    Theme {
        name: "Violet",
        color: rgb(0x7a5fb0),
        soon: false,
    },
    Theme {
        name: "Teal",
        color: rgb(0x2a9d8f),
        soon: false,
    },
    Theme {
        name: "Amber",
        color: rgb(0xd08a1e),
        soon: false,
    },
    Theme {
        name: "Rose",
        color: rgb(0xc9506f),
        soon: false,
    },
    Theme {
        name: "Gold",
        color: rgb(0xb8961f),
        soon: true,
    },
];

const fn transition(duration: f32, delay: f32) -> TransitionConfig {
    TransitionConfig {
        duration,
        delay,
        easing: Easing::EaseOutCubic,
    }
}

/// The engine settings both entry points use.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config
}

/// The menu app. It does all its work in callbacks, set up in `setup`.
#[derive(Default)]
pub struct Menu;

impl App for Menu {
    fn setup(&mut self, f: &mut Frame) {
        build(f.proteus);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Closed,
    Opening,
    Open,
    Closing,
}

/// What the callbacks share.
struct Picker {
    preview: Handle,
    backdrop: Handle,
    button: Handle,
    items: Vec<Handle>,
    state: Cell<State>,
    /// The index in [`THEMES`] of the current theme.
    current: Cell<usize>,
}

impl Picker {
    fn open(&self, app: &mut Proteus) {
        if self.state.get() != State::Closed {
            return;
        }
        self.state.set(State::Opening);
        let _ = self.backdrop.set_visible(app, true);
        // The button is cut into a strip per item, top to bottom. Each strip
        // starts a little after the one above it.
        let _ = self.button.split_to_with_behavior(
            app,
            &self.items,
            transition(0.35, 0.0),
            SplitStrategy::Column,
            |i, _| transition(0.3, i as f32 * 0.04),
        );
    }

    /// Merges the menu back into the button, choosing `picked` if it's a new
    /// theme.
    fn close(&self, app: &mut Proteus, picked: Option<usize>) {
        if self.state.get() != State::Open {
            return;
        }
        self.state.set(State::Closing);
        let _ = self.backdrop.set_visible(app, false);
        if let Some(i) = picked.filter(|&i| i != self.current.get()) {
            self.current.set(i);
            let theme = &THEMES[i];
            // The button is hidden while the menu is open, so it comes back
            // from the merge already naming the new theme.
            let label = Text::new(format!("Theme: {}", theme.name), 18.0).with_color(WHITE);
            let _ = self.button.set_text(app, label);
            if let Some(card) = app.get(self.preview) {
                let faded = QuadState {
                    color: theme.color,
                    ..card.geometry
                };
                let _ = self.preview.animate_to(app, faded, transition(0.4, 0.0));
            }
            let name = Text::new(theme.name, 30.0).with_color(WHITE);
            let _ = self.preview.set_text(app, name);
        }
        // The items come back from the bottom up.
        let _ = self.button.merge_from_with_behavior(
            app,
            &self.items,
            transition(0.3, 0.0),
            MergeLayout::Column,
            |i, total| transition(0.25, (total - 1 - i) as f32 * 0.03),
        );
    }
}

/// Creates the menu's components, and its callbacks, which do the rest.
fn build(app: &mut Proteus) -> Rc<Picker> {
    let first = &THEMES[0];

    // The preview card, which shows the current theme.
    let preview = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 150.0, 0.0),
            size: Vec2::new(360.0, 180.0),
            corner_radius: 20.0,
            color: first.color,
            ..Default::default()
        })
        .non_interactive()
        .text(Text::new(first.name, 30.0).with_color(WHITE)),
    );

    // Behind the menu: catches a click on empty space while the menu is open.
    // Transparent, but it still takes the pointer.
    let backdrop = app.component(
        ComponentSpec::new(QuadState {
            size: Vec2::splat(4000.0),
            color: Vec4::ZERO,
            ..Default::default()
        })
        .visible(false),
    );

    let button = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, BUTTON_Y, 0.0),
            size: Vec2::new(ITEM_SIZE.x, 56.0),
            corner_radius: 28.0,
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
        .text(Text::new(format!("Theme: {}", first.name), 18.0).with_color(WHITE)),
    );

    // The menu items, one per theme, in a column starting where the button
    // is. Hidden until the button splits into them.
    let items = THEMES
        .iter()
        .enumerate()
        .map(|(i, theme)| {
            let label = if theme.soon {
                format!("{} (soon)", theme.name)
            } else {
                theme.name.to_string()
            };
            let spec = ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, BUTTON_Y - i as f32 * (ITEM_SIZE.y + ITEM_GAP), 0.0),
                size: ITEM_SIZE,
                corner_radius: 12.0,
                color: theme.color,
                ..Default::default()
            })
            .hover(StyleOverride {
                scale: Some(1.04),
                ..Default::default()
            })
            .pressed(StyleOverride {
                scale: Some(0.97),
                ..Default::default()
            })
            .disabled(StyleOverride {
                color: Some(GRAY),
                ..Default::default()
            })
            .visible(false)
            .text(Text::new(label, 18.0).with_color(WHITE));
            app.component(if theme.soon {
                spec.start_disabled()
            } else {
                spec
            })
        })
        .collect();

    let picker = Rc::new(Picker {
        preview,
        backdrop,
        button,
        items,
        state: Cell::new(State::Closed),
        current: Cell::new(0),
    });

    let p = Rc::clone(&picker);
    button.on_click(app, move |app| p.open(app));
    let p = Rc::clone(&picker);
    backdrop.on_click(app, move |app| p.close(app, None));
    for (i, &item) in picker.items.iter().enumerate() {
        let p = Rc::clone(&picker);
        item.on_click(app, move |app| p.close(app, Some(i)));
    }
    // A split reports its end on its source, and a merge on its destination:
    // both are the button.
    let p = Rc::clone(&picker);
    button.on_transition_complete(app, move |_| match p.state.get() {
        State::Opening => p.state.set(State::Open),
        State::Closing => p.state.set(State::Closed),
        _ => {}
    });
    picker
}

/// Runs the menu on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // The menu loads no assets.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Menu, &canvas_id, config(), services).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(app: &mut Proteus) {
        for _ in 0..4 {
            app.tick(0.5);
        }
    }

    // Open the menu, pick Teal, and the menu closes back into the button,
    // which now names it, over a teal preview.
    #[test]
    fn picking_a_theme_closes_the_menu_and_shows_it() {
        let mut app = Proteus::new();
        let picker = build(&mut app);
        app.tick(0.0);

        picker.open(&mut app);
        settle(&mut app);
        assert_eq!(picker.state.get(), State::Open);
        assert!(!app.get(picker.button).unwrap().visible);
        assert!(picker
            .items
            .iter()
            .all(|&item| app.get(item).unwrap().visible));

        picker.close(&mut app, Some(1));
        settle(&mut app);
        assert_eq!(picker.state.get(), State::Closed);
        assert!(app.get(picker.button).unwrap().visible);
        assert!(picker
            .items
            .iter()
            .all(|&item| !app.get(item).unwrap().visible));
        assert_eq!(picker.current.get(), 1);
        assert_eq!(
            app.get(picker.preview).unwrap().geometry.color,
            THEMES[1].color
        );
    }

    // A click that doesn't fit the menu's state is ignored: picking while the
    // menu is still opening does nothing.
    #[test]
    fn a_pick_while_opening_is_ignored() {
        let mut app = Proteus::new();
        let picker = build(&mut app);
        app.tick(0.0);

        picker.open(&mut app);
        app.tick(0.0);
        picker.close(&mut app, Some(2));
        assert_eq!(picker.state.get(), State::Opening);
        assert_eq!(picker.current.get(), 0);
    }
}
