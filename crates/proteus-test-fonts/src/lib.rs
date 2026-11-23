//! Fonts for Proteus's own tests, so that tests of fonts, weights, italics
//! and fallback don't depend on any example's files.
//!
//! Each font is named for what the tests need it for, with the family name
//! inside it. All are from Google Fonts, under the SIL Open Font License 1.1,
//! whose text for each is in `fonts/`.
//!
//! This crate isn't published: `proteus-render` and `proteus-runtime` take it
//! as a dev-dependency only, which `cargo publish` leaves out.

/// A family besides the built-in Inter: Lora, a serif, variable in weight
/// from 400 to 700, so a heavier weight uses its nearest.
pub const SECOND_FAMILY: &[u8] = include_bytes!("../fonts/Lora-Variable.ttf");

/// The family name inside [`SECOND_FAMILY`] and [`SECOND_FAMILY_ITALIC`].
pub const SECOND_FAMILY_NAME: &str = "Lora";

/// [`SECOND_FAMILY`]'s italic face, a design of its own rather than a slant.
pub const SECOND_FAMILY_ITALIC: &[u8] = include_bytes!("../fonts/Lora-Italic-Variable.ttf");

/// A third family: Crimson Pro, a serif with letters, such as "ĸ", that
/// [`SECOND_FAMILY`] lacks and Inter has, for testing the order fallback
/// fonts are tried in.
pub const THIRD_FAMILY: &[u8] = include_bytes!("../fonts/CrimsonPro-Variable.ttf");

/// The family name inside [`THIRD_FAMILY`].
pub const THIRD_FAMILY_NAME: &str = "Crimson Pro";

/// A font for a script Inter lacks: Noto Serif Devanagari, for Hindi.
pub const DEVANAGARI: &[u8] = include_bytes!("../fonts/NotoSerifDevanagari-Variable.ttf");

/// The family name inside [`DEVANAGARI`].
pub const DEVANAGARI_NAME: &str = "Noto Serif Devanagari";
