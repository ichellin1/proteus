//! An onboarding flow, the same app as the TypeScript stepper example.
//!
//! Four steps, each a card with its own shape and color. Next and Back turn
//! the current card into the next or previous one with a transition channel
//! (1→1): nothing is swapped, the card becomes the next step. Dots below show
//! the progress, and Back is disabled on the first step.
//!
//! Techniques worth copying:
//!   - One transition channel drives the whole flow. The code that calls
//!     `set` decides where each step goes; the cards don't refer to each
//!     other.
//!   - Clicking while a card is still moving needs no special handling: the
//!     next card starts from wherever the moving one is, and with an
//!     interruptible request, a card asked to come back changes course.
//!   - The callbacks only change components, so they hold what they need,
//!     the handles and the current step, in an `Rc` and do all the work
//!     themselves. Compare the gallery example, whose callbacks need the
//!     whole app and hand events to `update` instead.
//!   - Back is disabled, not hidden, on the first step: it stays in place,
//!     turns gray, and ignores clicks.
//!   - Positions are measured from the window's center, so the layout needs
//!     no window size.

use std::cell::Cell;
use std::rc::Rc;

use proteus_runtime::{App, Frame, ProteusConfig};
use proteus_sdk::glam::{Vec2, Vec3, Vec4};
use proteus_sdk::{
    ComponentSpec, Easing, Handle, Proteus, QuadState, StyleOverride, Text, TransitionChannel,
    TransitionConfig,
};

const BACKGROUND: Vec4 = rgb(0x1d1b26);
const BUTTON: Vec4 = rgb(0x2d2a3a);
const BUTTON_HOVER: Vec4 = rgb(0x3d394f);
const DOT: Vec4 = rgb(0x4a4756);
const WHITE: Vec4 = rgb(0xffffff);

const STEP: TransitionConfig = TransitionConfig {
    duration: 0.5,
    delay: 0.0,
    easing: Easing::EaseOutCubic,
};
const DOT_CHANGE: TransitionConfig = TransitionConfig {
    duration: 0.25,
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

/// One step of the flow: its card's title, size, corner radius and color.
struct Step {
    title: &'static str,
    size: Vec2,
    corner_radius: f32,
    color: Vec4,
}

const STEPS: [Step; 4] = [
    Step {
        title: "Welcome",
        size: Vec2::new(240.0, 240.0),
        corner_radius: 120.0,
        color: rgb(0x7a5fb0),
    },
    Step {
        title: "Choose a theme",
        size: Vec2::new(520.0, 200.0),
        corner_radius: 24.0,
        color: rgb(0x2a9d8f),
    },
    Step {
        title: "Turn on notifications",
        size: Vec2::new(340.0, 360.0),
        corner_radius: 40.0,
        color: rgb(0xd08a1e),
    },
    Step {
        title: "You're all set",
        size: Vec2::new(600.0, 320.0),
        corner_radius: 16.0,
        color: rgb(0xc9506f),
    },
];

const CARD_Y: f32 = 60.0;
const DOTS_Y: f32 = -180.0;
const BUTTONS_Y: f32 = -250.0;
const DOT_SIZE: f32 = 12.0;
const CURRENT_DOT_WIDTH: f32 = 36.0;
const DOT_GAP: f32 = 12.0;

/// The engine settings both entry points use.
pub fn config() -> ProteusConfig {
    let mut config = ProteusConfig::web();
    config.render.clear_color = BACKGROUND.as_dvec4().to_array();
    config
}

/// The stepper app. It does all its work in callbacks, set up in `setup`.
#[derive(Default)]
pub struct Stepper;

/// What the callbacks share.
struct Flow {
    cards: Vec<Handle>,
    dots: Vec<Handle>,
    back: Handle,
    next: Handle,
    channel: TransitionChannel,
    /// The step showing, or arriving.
    current: Cell<usize>,
}

impl Flow {
    /// Turns the current card into step `to`'s, and updates the dots and
    /// buttons to match.
    fn go(&self, app: &mut Proteus, to: usize) {
        let from = self.current.replace(to);
        // Interruptible: after Next, Back and Next again in quick succession,
        // the last card is still moving when it's asked to come back, and it
        // changes course from wherever it is. Without this, that request
        // would be dropped.
        self.channel
            .set(app, self.cards[to], self.cards[from], STEP, true);
        for (i, &dot) in self.dots.iter().enumerate() {
            let _ = dot.animate_to(app, dot_geometry(i, to), DOT_CHANGE);
        }
        let _ = self.back.set_disabled(app, to == 0);
        let label = if to == STEPS.len() - 1 {
            "Start over"
        } else {
            "Next"
        };
        let _ = self
            .next
            .set_text(app, Text::new(label, 18.0).with_color(WHITE));
    }
}

/// Dot `i`'s geometry when step `current` is showing: the current dot is
/// stretched and white. The row stays centered.
fn dot_geometry(i: usize, current: usize) -> QuadState {
    let widths = (0..STEPS.len()).map(|j| {
        if j == current {
            CURRENT_DOT_WIDTH
        } else {
            DOT_SIZE
        }
    });
    let total: f32 = widths.clone().sum::<f32>() + DOT_GAP * (STEPS.len() - 1) as f32;
    let left: f32 = widths.clone().take(i).map(|w| w + DOT_GAP).sum();
    let width = widths.clone().nth(i).unwrap_or(DOT_SIZE);
    QuadState {
        position: Vec3::new(-total / 2.0 + left + width / 2.0, DOTS_Y, 0.0),
        size: Vec2::new(width, DOT_SIZE),
        corner_radius: DOT_SIZE / 2.0,
        color: if i == current { WHITE } else { DOT },
        ..Default::default()
    }
}

/// A button's spec: dark, lighter on hover, smaller while pressed, and gray
/// while disabled.
fn button(x: f32, label: &str) -> ComponentSpec {
    ComponentSpec::new(QuadState {
        position: Vec3::new(x, BUTTONS_Y, 0.0),
        size: Vec2::new(150.0, 48.0),
        corner_radius: 24.0,
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
    .disabled(StyleOverride {
        color: Some(BUTTON.with_w(0.4)),
        ..Default::default()
    })
    .text(Text::new(label, 18.0).with_color(WHITE))
}

impl App for Stepper {
    fn setup(&mut self, f: &mut Frame) {
        build(f.proteus);
    }
}

/// Creates the stepper's components, and its callbacks, which do the rest.
fn build(app: &mut Proteus) -> Rc<Flow> {
    // A card for each step, all in the same place. Only the first starts
    // visible; the channel shows each as the flow reaches it.
    let cards = STEPS
        .iter()
        .enumerate()
        .map(|(i, step)| {
            app.component(
                ComponentSpec::new(QuadState {
                    position: Vec3::new(0.0, CARD_Y, 0.0),
                    size: step.size,
                    corner_radius: step.corner_radius,
                    color: step.color,
                    ..Default::default()
                })
                .non_interactive()
                .visible(i == 0)
                .text(Text::new(step.title, 28.0).with_color(WHITE)),
            )
        })
        .collect();
    let dots = (0..STEPS.len())
        .map(|i| app.component(ComponentSpec::new(dot_geometry(i, 0)).non_interactive()))
        .collect();
    let back = app.component(button(-90.0, "Back").start_disabled());
    let next = app.component(button(90.0, "Next"));

    let flow = Rc::new(Flow {
        cards,
        dots,
        back,
        next,
        channel: app.transition_channel(None),
        current: Cell::new(0),
    });

    let on_next = Rc::clone(&flow);
    next.on_click(app, move |app| {
        // After the last step, start over.
        let to = (on_next.current.get() + 1) % STEPS.len();
        on_next.go(app, to);
    });
    let on_back = Rc::clone(&flow);
    back.on_click(app, move |app| {
        if let Some(to) = on_back.current.get().checked_sub(1) {
            on_back.go(app, to);
        }
    });
    flow
}

/// Runs the stepper on the `<canvas>` element with the id `canvas_id`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Warn));
    // The stepper loads no assets.
    let services = proteus_host_web::PreloadedHostServices::fetch("", &[]).await;
    proteus_host_web::run(Stepper, &canvas_id, config(), services).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // Next, Back and Next again, faster than a transition, must still end
    // with exactly the current step's card showing, at rest.
    #[test]
    fn quick_clicks_end_on_the_current_step() {
        let mut app = Proteus::new();
        let flow = build(&mut app);
        app.tick(0.0);
        for (to, wait) in [(1, 0.1), (0, 0.1), (1, 0.05), (2, 0.0), (1, 0.2)] {
            flow.go(&mut app, to);
            app.tick(0.0);
            app.tick(wait);
        }
        app.tick(1.0);
        app.tick(1.0);

        assert_eq!(flow.current.get(), 1);
        for (i, &card) in flow.cards.iter().enumerate() {
            let data = app.get(card).unwrap();
            assert_eq!(data.visible, i == 1, "card {i}'s visibility");
            if i == 1 {
                assert!(data.transition.is_none(), "the card should be at rest");
                assert_eq!(data.geometry.size, STEPS[1].size);
            }
        }
    }

    // The row of dots stays centered, with the current dot stretched, so it
    // doesn't shift sideways as the current step changes.
    #[test]
    fn the_dots_stay_centered_and_dont_overlap() {
        for current in 0..STEPS.len() {
            let dots: Vec<QuadState> = (0..STEPS.len()).map(|i| dot_geometry(i, current)).collect();
            let left = dots[0].position.x - dots[0].size.x / 2.0;
            let last = dots.last().unwrap();
            let right = last.position.x + last.size.x / 2.0;
            assert!((left + right).abs() < 0.001, "step {current}: not centered");
            for pair in dots.windows(2) {
                let gap = (pair[1].position.x - pair[1].size.x / 2.0)
                    - (pair[0].position.x + pair[0].size.x / 2.0);
                assert!((gap - DOT_GAP).abs() < 0.001, "step {current}: gap {gap}");
            }
            assert_eq!(dots[current].size.x, CURRENT_DOT_WIDTH);
        }
    }
}
