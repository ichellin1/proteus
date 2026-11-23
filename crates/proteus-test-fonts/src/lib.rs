//! Fonts for Proteus's own tests, so that tests of fonts, weights, italics
//! and fallback don't depend on any example's files.
//!
//! Each font is named for what the tests use it for, and a separate constant
//! holds its family name, as written inside the font. All the fonts are from
//! Google Fonts, under the SIL Open Font License 1.1. The license text for
//! each is in `fonts/`.
//!
//! This crate isn't published. `proteus-render` and `proteus-runtime` depend
//! on it only as a dev-dependency, and `cargo publish` leaves those out.

/// Lora, a serif family besides the built-in Inter. Its weight varies from
/// 400 to 700, so a heavier weight uses 700, the nearest.
pub const SECOND_FAMILY: &[u8] = include_bytes!("../fonts/Lora-Variable.ttf");

/// The family name inside [`SECOND_FAMILY`] and [`SECOND_FAMILY_ITALIC`].
pub const SECOND_FAMILY_NAME: &str = "Lora";

/// [`SECOND_FAMILY`]'s italic face, a design of its own rather than a slant.
pub const SECOND_FAMILY_ITALIC: &[u8] = include_bytes!("../fonts/Lora-Italic-Variable.ttf");

/// Crimson Pro, a serif and a third family. It has letters, such as "ĸ", that
/// Inter has and [`SECOND_FAMILY`] lacks, so tests can check the order
/// fallback fonts are tried in.
pub const THIRD_FAMILY: &[u8] = include_bytes!("../fonts/CrimsonPro-Variable.ttf");

/// The family name inside [`THIRD_FAMILY`].
pub const THIRD_FAMILY_NAME: &str = "Crimson Pro";

/// Noto Serif Devanagari, a font for Hindi, whose script Inter lacks.
pub const DEVANAGARI: &[u8] = include_bytes!("../fonts/NotoSerifDevanagari-Variable.ttf");

/// The family name inside [`DEVANAGARI`].
pub const DEVANAGARI_NAME: &str = "Noto Serif Devanagari";
