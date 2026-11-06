//! The Keyboard & Focus panel's virtual keyboard: a generic US layout of
//! rounded keycaps, without the function keys, Esc, Tab or the arrows. The
//! keycaps are a focus group the arrows and Tab move round, and a key pressed
//! on the real keyboard flashes its keycap, matched by where the key is.
//! Caps Lock switches the keycaps to their shifted values, and back.
//! `Demo::advance_keyboard` drives the flashes and the Caps Lock toggle.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{
    Action, Border, Code, ComponentSpec, FocusEntry, FocusGroup, Glow, Handle, Proteus, QuadState,
    StyleOverride, Text,
};

/// From one keycap's left edge to the next, for a keycap one unit wide.
const PITCH_PX: f32 = 50.0;
/// The space between two keycaps.
const GAP_PX: f32 = 6.0;
/// The width of every row, in units, as on a real keyboard.
const ROW_UNITS: f32 = 15.0;
const ROW_COUNT: usize = 5;
/// The keyboard's size.
pub const SIZE: Vec2 = Vec2::new(
    ROW_UNITS * PITCH_PX - GAP_PX,
    ROW_COUNT as f32 * PITCH_PX - GAP_PX,
);
const CORNER_RADIUS: f32 = 8.0;
const BORDER_WIDTH: f32 = 2.0;
/// A keycap's label: a character at this size, a word such as "Shift"
/// smaller, to fit.
const CHARACTER_SIZE_PX: f32 = 18.0;
const WORD_SIZE_PX: f32 = 13.0;
/// How long a keycap takes to fade back from a flash.
pub const FLASH_SECS: f32 = 0.3;
/// How far toward the primary color a keycap's fill goes at a flash's peak.
const FLASH_STRENGTH: f32 = 0.6;
/// How far toward the primary color the Caps Lock keycap's fill stays while
/// the keyboard is shifted.
const SHIFTED_TINT: f32 = 0.25;

fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// One keycap: where its key is, its label and shifted label, and its
/// width in units.
struct KeyCap {
    code: Code,
    label: &'static str,
    shifted: &'static str,
    units: f32,
}

/// A keycap one unit wide.
fn key(code: Code, label: &'static str, shifted: &'static str) -> KeyCap {
    KeyCap {
        code,
        label,
        shifted,
        units: 1.0,
    }
}

/// A keycap `units` wide, whose label doesn't change with Shift.
fn wide(code: Code, label: &'static str, units: f32) -> KeyCap {
    KeyCap {
        code,
        label,
        shifted: label,
        units,
    }
}

/// The rows, top to bottom, each with the units it starts after: the second
/// starts where a Tab key would end, so the rows stagger as they do on a
/// real keyboard.
fn rows() -> [(f32, Vec<KeyCap>); ROW_COUNT] {
    use Code::*;
    [
        (
            0.0,
            vec![
                key(Backquote, "`", "~"),
                key(Digit1, "1", "!"),
                key(Digit2, "2", "@"),
                key(Digit3, "3", "#"),
                key(Digit4, "4", "$"),
                key(Digit5, "5", "%"),
                key(Digit6, "6", "^"),
                key(Digit7, "7", "&"),
                key(Digit8, "8", "*"),
                key(Digit9, "9", "("),
                key(Digit0, "0", ")"),
                key(Minus, "-", "_"),
                key(Equal, "=", "+"),
                wide(Backspace, "Backspace", 2.0),
            ],
        ),
        (
            1.5,
            vec![
                key(KeyQ, "q", "Q"),
                key(KeyW, "w", "W"),
                key(KeyE, "e", "E"),
                key(KeyR, "r", "R"),
                key(KeyT, "t", "T"),
                key(KeyY, "y", "Y"),
                key(KeyU, "u", "U"),
                key(KeyI, "i", "I"),
                key(KeyO, "o", "O"),
                key(KeyP, "p", "P"),
                key(BracketLeft, "[", "{"),
                key(BracketRight, "]", "}"),
                KeyCap {
                    units: 1.5,
                    ..key(Backslash, "\\", "|")
                },
            ],
        ),
        (
            0.0,
            vec![
                wide(CapsLock, "Caps Lock", 1.75),
                key(KeyA, "a", "A"),
                key(KeyS, "s", "S"),
                key(KeyD, "d", "D"),
                key(KeyF, "f", "F"),
                key(KeyG, "g", "G"),
                key(KeyH, "h", "H"),
                key(KeyJ, "j", "J"),
                key(KeyK, "k", "K"),
                key(KeyL, "l", "L"),
                key(Semicolon, ";", ":"),
                key(Quote, "'", "\""),
                wide(Enter, "Enter", 2.25),
            ],
        ),
        (
            0.0,
            vec![
                wide(ShiftLeft, "Shift", 2.25),
                key(KeyZ, "z", "Z"),
                key(KeyX, "x", "X"),
                key(KeyC, "c", "C"),
                key(KeyV, "v", "V"),
                key(KeyB, "b", "B"),
                key(KeyN, "n", "N"),
                key(KeyM, "m", "M"),
                key(Comma, ",", "<"),
                key(Period, ".", ">"),
                key(Slash, "/", "?"),
                wide(ShiftRight, "Shift", 2.75),
            ],
        ),
        (
            0.0,
            vec![
                wide(ControlLeft, "Ctrl", 1.25),
                wide(MetaLeft, "Meta", 1.25),
                wide(AltLeft, "Alt", 1.25),
                wide(Space, "", 7.5),
                wide(AltRight, "Alt", 1.25),
                wide(MetaRight, "Meta", 1.25),
                wide(ControlRight, "Ctrl", 1.25),
            ],
        ),
    ]
}

pub struct Keyboard {
    /// The focus group every keycap is in, which places them: the arrows
    /// and Tab move between keycaps and don't leave it, and focus coming
    /// back lands on the keycap focused last.
    pub board: Handle,
    /// The keycaps, row by row, left to right.
    pub keys: Vec<Handle>,
    codes: Vec<Code>,
    labels: Vec<(&'static str, &'static str)>,
}

impl Keyboard {
    /// The keycap of the key at `code`, if the keyboard has one.
    pub fn index_of(&self, code: &Code) -> Option<usize> {
        self.codes.iter().position(|c| c == code)
    }

    /// Whether keycap `i` is Caps Lock.
    pub fn is_caps_lock(&self, i: usize) -> bool {
        self.codes[i] == Code::CapsLock
    }

    /// The space bar, where focus starts as the panel opens.
    pub fn space(&self) -> Handle {
        self.keys[self
            .index_of(&Code::Space)
            .expect("the keyboard has a space bar")]
    }

    /// Keycap `i`'s label, shifted or not.
    pub fn label(&self, i: usize, shifted: bool) -> &'static str {
        let (label, shifted_label) = self.labels[i];
        if shifted {
            shifted_label
        } else {
            label
        }
    }

    /// Whether keycap `i`'s label changes with Shift.
    pub fn shifts(&self, i: usize) -> bool {
        let (label, shifted) = self.labels[i];
        label != shifted
    }
}

/// Keycap `i`'s label as text: a character, or a smaller word.
pub fn label_text(label: &str) -> Text {
    let size = if label.chars().count() > 1 {
        WORD_SIZE_PX
    } else {
        CHARACTER_SIZE_PX
    };
    Text::new(label, size).with_color(violet())
}

pub fn spawn(app: &mut Proteus, z: f32) -> Keyboard {
    // A glow while focused, shown only while the keys are in use, or while
    // hovered. No growth: a grown keycap, the space bar most of all, would
    // overlap its neighbours, and moving focus to the nearest keycap goes by
    // where the keycaps are drawn.
    let lit = StyleOverride {
        glow_radius: Some(12.0),
        ..Default::default()
    };
    let mut keys = Vec::new();
    let mut codes = Vec::new();
    let mut labels = Vec::new();
    for (row, (start, caps)) in rows().into_iter().enumerate() {
        let y = SIZE.y / 2.0 - PITCH_PX / 2.0 + GAP_PX / 2.0 - row as f32 * PITCH_PX;
        let mut units = start;
        for cap in caps {
            let width = cap.units * PITCH_PX - GAP_PX;
            let x = -SIZE.x / 2.0 + units * PITCH_PX + width / 2.0;
            units += cap.units;
            let handle = app.component(
                ComponentSpec::new(QuadState {
                    position: Vec3::new(x, y, 0.0),
                    size: Vec2::new(width, PITCH_PX - GAP_PX),
                    rotation: 0.0,
                    scale: 1.0,
                    anchor: Vec2::new(0.5, 0.5),
                    color: key_fill(0.0, false),
                    corner_radius: CORNER_RADIUS,
                })
                .border(Border::new(BORDER_WIDTH, violet()))
                .glow(Glow {
                    radius: 0.0,
                    color: violet(),
                    intensity: 1.0,
                })
                .focused(lit.clone())
                .hover(lit.clone())
                .text(label_text(cap.label)),
            );
            keys.push(handle);
            codes.push(cap.code);
            labels.push((cap.label, cap.shifted));
        }
    }
    let mut board = ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, 0.0, z),
        size: SIZE,
        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
        ..Default::default()
    })
    .non_interactive()
    .focus_group(
        FocusGroup::new()
            .stop(&[
                Action::Up,
                Action::Down,
                Action::Left,
                Action::Right,
                Action::Next,
                Action::Previous,
            ])
            .entry(FocusEntry::LastFocused),
    );
    for &key in &keys {
        board = board.child(key);
    }
    Keyboard {
        board: app.component(board),
        keys,
        codes,
        labels,
    }
}

/// A keycap's fill: white, toward the primary color by `flash` (1 at a
/// flash's peak, fading to 0), and tinted a little if it's Caps Lock while
/// the keyboard is shifted. Always the light theme's primary color, as the
/// rest of the panel's content is.
pub fn key_fill(flash: f32, tinted: bool) -> Vec4 {
    let base = if tinted {
        Vec4::ONE.lerp(violet(), SHIFTED_TINT)
    } else {
        Vec4::ONE
    };
    base.lerp(violet(), FLASH_STRENGTH * flash.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every row is 15 units wide, as on a real keyboard, so the keyboard's
    // right edge is straight.
    #[test]
    fn every_row_ends_at_the_keyboards_right_edge() {
        for (i, (start, caps)) in rows().into_iter().enumerate() {
            let units: f32 = start + caps.iter().map(|c| c.units).sum::<f32>();
            assert_eq!(units, ROW_UNITS, "row {i}");
        }
    }

    #[test]
    fn a_flash_fades_toward_white_and_caps_lock_stays_tinted() {
        assert_eq!(key_fill(0.0, false), Vec4::ONE);
        assert!(key_fill(1.0, false).x < key_fill(0.5, false).x);
        assert!(key_fill(0.0, true).x < 1.0);
    }
}
