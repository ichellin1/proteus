//! Laying out and rasterizing text, on the CPU.
//!
//! [`FontAtlas`] lays a text out with parley, which shapes it, and rasterizes
//! its glyphs with swash into one RGBA image. The caller then allocates a
//! main-atlas region for it with [`crate::TextureRegistry::register_static`]
//! and uploads it with [`crate::QuadPipeline::write_to_main_atlas`].
//!
//! ## Shaping
//!
//! Shaping turns characters into glyphs placed by the font's own rules: it
//! applies the font's kerning, which pulls pairs such as "AV" closer, joins
//! ligatures, places accents on their letters, and joins the letters of
//! scripts that need it. Each glyph is placed to a quarter of a pixel, so text
//! is as wide as its font designs it, at every size and display scale.
//!
//! ## Text as a texture
//!
//! The whole text becomes one image, at its size in pixels:
//! - red, green and blue are 255 everywhere, so the text can take any color;
//! - alpha is each pixel's glyph coverage, antialiased.
//!
//! The text is then drawn like any other texture: it transitions, takes a
//! color, and is clipped by rounded corners, with no special handling.
//!
//! ## Embedded font
//!
//! [`EMBEDDED_FONT_BYTES`] is Inter Bold (SIL Open Font License 1.1), a static
//! instance at weight 700 and optical size 14, made with `fonttools`:
//! `fonttools varLib.instancer --update-name-table Inter-Variable.ttf wght=700 opsz=14`.
//! To use another font, pass its bytes to [`FontAtlas::new`], which returns a
//! [`FontError`] if they aren't a font it can read. No system font is ever
//! used.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache, SourceCacheOptions};
use parley::{
    FontContext, FontFamily, FontFamilyName, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

/// Inter Bold TTF (static instance, see the module docs), embedded at compile time.
///
/// License: SIL Open Font License 1.1 (permissive, bundling allowed). See
/// `assets/LICENSE-Inter.txt`.
pub const EMBEDDED_FONT_BYTES: &[u8] = include_bytes!("../assets/Inter-Bold.ttf");

/// How many positions within a pixel a glyph can start at, across: a glyph
/// is rasterized at the nearest quarter of a pixel to where it's placed.
const SUBPIXEL_STEPS: f32 = 4.0;

// ---------------------------------------------------------------------------
// RasterizedGlyphs
// ---------------------------------------------------------------------------

/// The pixels from one [`FontAtlas::rasterize_text`] call, with no atlas
/// position yet. Allocate a region with
/// [`crate::TextureRegistry::register_static`], then upload with
/// [`crate::QuadPipeline::write_to_main_atlas`].
#[derive(Debug, Clone)]
pub struct RasterizedGlyphs {
    /// Width of the rasterized text image in pixels.
    pub width: u32,
    /// Height of the rasterized text image in pixels.
    pub height: u32,
    /// RGBA pixels, `width * height * 4` bytes. Alpha is the glyph coverage,
    /// not premultiplied.
    pub rgba_pixels: Vec<u8>,
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// Lays out and rasterizes text in one font, on the CPU. It doesn't allocate
/// atlas space (see the module docs).
///
/// One is enough for an app: the renderer owns one and uses it for all text.
/// Call [`rasterize_text`] for each string and size, allocate a main-atlas
/// region for the result with `TextureRegistry::register_static`, then upload
/// it with `QuadPipeline::write_to_main_atlas`.
///
/// [`rasterize_text`]: FontAtlas::rasterize_text
pub struct FontAtlas {
    fonts: FontContext,
    layout: LayoutContext<()>,
    scale: ScaleContext,
    /// The font's family name, which each text asks parley for.
    family: String,
}

/// Font bytes that [`FontAtlas::new`] couldn't read as a TTF or OTF font.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not a TTF or OTF font that can be read: {0}")]
pub struct FontError(String);

/// A glyph rasterized for one text, before it's drawn into the text's image.
struct PlacedGlyph {
    /// The left and top of its pixels, from the text's pen start and its first
    /// line's baseline, in whole pixels.
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

impl FontAtlas {
    /// Creates a [`FontAtlas`] for the TTF or OTF font in `font_bytes`.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
    pub fn new(font_bytes: &[u8]) -> Result<Self, FontError> {
        Self::from_blob(Blob::new(Arc::new(font_bytes.to_vec())))
    }

    /// Creates a [`FontAtlas`] for the embedded font, [`EMBEDDED_FONT_BYTES`]
    /// (Inter Bold).
    pub fn with_embedded_font() -> Self {
        Self::from_blob(Blob::new(Arc::new(EMBEDDED_FONT_BYTES)))
            .expect("the embedded font is a valid TTF")
    }

    fn from_blob(blob: Blob<u8>) -> Result<Self, FontError> {
        // Only the fonts given here: never the system's.
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let families = collection.register_fonts(blob, None);
        let family = families
            .first()
            .and_then(|(id, _)| collection.family_name(*id))
            .ok_or_else(|| FontError("no font family found in the bytes".into()))?
            .to_owned();
        Ok(Self {
            fonts: FontContext {
                collection,
                source_cache: SourceCache::new(SourceCacheOptions::default()),
            },
            layout: LayoutContext::new(),
            scale: ScaleContext::new(),
            family,
        })
    }

    /// Lays `text` out at `size_px`, with `letter_spacing_px` of extra space
    /// between glyphs (`0.0` for the font's normal spacing), and rasterizes it
    /// into an RGBA pixel buffer. A `\n` starts a new line. The image spans
    /// the glyphs' ink, so it has no padding after the last glyph.
    ///
    /// Returns `None` if `text` draws no pixels: it's empty, or only
    /// whitespace.
    pub fn rasterize_text(
        &mut self,
        text: &str,
        size_px: f32,
        letter_spacing_px: f32,
    ) -> Option<RasterizedGlyphs> {
        if text.is_empty() {
            return None;
        }
        let layout = self.lay_out(text, size_px, letter_spacing_px);

        // The image's height: from the top of the first line to the bottom of
        // the last, each part rounded up on its own, so that the baseline sits
        // on a whole pixel and descenders are never cut off.
        let mut lines = layout.lines();
        let first = *lines.next()?.metrics();
        let last = layout.lines().last().map_or(first, |l| *l.metrics());
        let ascent_px = (first.baseline - first.block_min_coord).ceil() as i32;
        let below_px = (last.block_max_coord - first.baseline).ceil() as i32;
        let height = ascent_px + below_px;
        if height <= 0 {
            return None;
        }

        let glyphs = self.rasterize_glyphs(&layout, first.baseline);

        // The image's width: from the leftmost ink, or the pen's start if
        // that's further left, to the rightmost ink. A glyph's ink can reach
        // left of the pen, as Inter's "j" does, or past its own advance.
        let min_left = glyphs.iter().map(|g| g.left).min()?.min(0);
        let max_right = glyphs.iter().map(|g| g.left + g.width as i32).max()?;
        let width = (max_right - min_left).max(0) as u32;
        if width == 0 {
            return None;
        }

        // White, with each glyph's coverage in alpha. Where glyphs overlap,
        // the greater coverage wins.
        let mut rgba = vec![0u8; (width * height as u32 * 4) as usize];
        for g in &glyphs {
            for gy in 0..g.height as i32 {
                let py = ascent_px + g.top + gy;
                if py < 0 || py >= height {
                    continue;
                }
                for gx in 0..g.width as i32 {
                    let px = g.left - min_left + gx;
                    let coverage = g.coverage[(gy * g.width as i32 + gx) as usize];
                    if coverage == 0 || px < 0 || px >= width as i32 {
                        continue;
                    }
                    let i = ((py as u32 * width + px as u32) * 4) as usize;
                    rgba[i..i + 3].fill(255);
                    rgba[i + 3] = rgba[i + 3].max(coverage);
                }
            }
        }

        Some(RasterizedGlyphs {
            width,
            height: height as u32,
            rgba_pixels: rgba,
        })
    }

    /// Shapes `text` and breaks it into lines at each `\n`.
    fn lay_out(&mut self, text: &str, size_px: f32, letter_spacing_px: f32) -> Layout<()> {
        // Fractional positions, rounded when the glyphs are rasterized.
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, text, 1.0, false);
        builder.push_default(StyleProperty::FontFamily(FontFamily::Single(
            FontFamilyName::Named(self.family.as_str().into()),
        )));
        builder.push_default(StyleProperty::FontSize(size_px));
        builder.push_default(StyleProperty::LetterSpacing(letter_spacing_px));
        builder.push_default(StyleProperty::LineHeight(LineHeight::MetricsRelative(1.0)));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout
    }

    /// Rasterizes every glyph in `layout`, placed relative to the pen's start
    /// and the first line's baseline, at `baseline`.
    fn rasterize_glyphs(&mut self, layout: &Layout<()>, baseline: f32) -> Vec<PlacedGlyph> {
        let mut glyphs = Vec::new();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let font = run.font();
                let Some(font_ref) =
                    swash::FontRef::from_index(font.data.as_ref(), font.index as usize)
                else {
                    continue;
                };
                let mut scaler = self
                    .scale
                    .builder(font_ref)
                    .size(run.font_size())
                    .hint(false)
                    .normalized_coords(run.normalized_coords().iter().map(|c| c.to_bits()))
                    .build();
                for g in glyph_run.positioned_glyphs() {
                    // Across, to the nearest quarter pixel; down, to the
                    // nearest pixel, so lines sit on whole pixels.
                    let x = (g.x * SUBPIXEL_STEPS).round() / SUBPIXEL_STEPS;
                    let pen_x = x.floor();
                    let pen_y = (g.y - baseline).round();
                    let Some(image) = Render::new(&[Source::Outline])
                        .format(Format::Alpha)
                        .offset(Vector::new(x - pen_x, 0.0))
                        .render(&mut scaler, g.id as u16)
                    else {
                        continue;
                    };
                    if image.placement.width == 0 || image.placement.height == 0 {
                        continue;
                    }
                    glyphs.push(PlacedGlyph {
                        left: pen_x as i32 + image.placement.left,
                        top: pen_y as i32 - image.placement.top,
                        width: image.placement.width,
                        height: image.placement.height,
                        coverage: image.data,
                    });
                }
            }
        }
        glyphs
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> FontAtlas {
        FontAtlas::with_embedded_font()
    }

    /// The columns with any coverage, as a range: where the ink is across.
    fn ink_columns(g: &RasterizedGlyphs) -> std::ops::Range<u32> {
        let has_ink =
            |x: u32| (0..g.height).any(|y| g.rgba_pixels[((y * g.width + x) * 4 + 3) as usize] > 0);
        let first = (0..g.width).find(|&x| has_ink(x)).unwrap();
        let last = (0..g.width).rev().find(|&x| has_ink(x)).unwrap();
        first..last + 1
    }

    // A glyph whose ink starts left of the pen, like Inter's "j", keeps its
    // left edge: the image starts at its ink, not at the pen.
    #[test]
    fn a_glyph_with_a_negative_left_bearing_isnt_clipped() {
        let mut fa = atlas();
        let j = fa.rasterize_text("j", 64.0, 0.0).unwrap();
        assert_eq!(
            ink_columns(&j).start,
            0,
            "the ink starts at the image's left edge"
        );
        let ij = fa.rasterize_text("ij", 64.0, 0.0).unwrap();
        assert!(ij.width > j.width);
    }

    // Descenders reach the image's last rows, and aren't cut off.
    #[test]
    fn descenders_reach_the_bottom_rows_without_being_cut_off() {
        let mut fa = atlas();
        let g = fa.rasterize_text("gyp", 48.0, 0.0).unwrap();
        let x_height = fa.rasterize_text("xxx", 48.0, 0.0).unwrap();
        assert_eq!(g.height, x_height.height, "every line has the same height");
        let row_has_ink =
            |y: u32| (0..g.width).any(|x| g.rgba_pixels[((y * g.width + x) * 4 + 3) as usize] > 0);
        let lowest = (0..g.height).rev().find(|&y| row_has_ink(y)).unwrap();
        assert!(lowest + 1 < g.height, "room is left below the lowest ink");
    }

    // The font's kerning pulls "A" and "V" together: laid out, "AV" advances
    // less than the font's advances for "A" and "V" added up, which is how
    // text was spaced before it was shaped.
    #[test]
    fn kerning_pulls_a_pair_together() {
        let mut fa = atlas();
        let shaped = fa.lay_out("AV", 64.0, 0.0).full_width();

        let font = swash::FontRef::from_index(EMBEDDED_FONT_BYTES, 0).unwrap();
        let metrics = font.glyph_metrics(&[]).scale(64.0);
        let advance = |c| metrics.advance_width(font.charmap().map(c));
        let unshaped = advance('A') + advance('V');

        assert!(
            shaped < unshaped - 2.0,
            "AV advances {shaped}; A and V add up to {unshaped}"
        );
    }

    // Text measures the same in logical pixels at every display scale, so it
    // lays out the same on every display. Glyph advances rounded up to whole
    // pixels made a line wider at a scale of 1 than at 2.
    #[test]
    fn a_line_is_as_wide_at_every_scale() {
        let mut fa = atlas();
        let text = "The quick brown fox jumps over the lazy dog";
        let at_1 = fa.rasterize_text(text, 12.0, 0.0).unwrap().width as f32;
        let at_2 = fa.rasterize_text(text, 24.0, 0.0).unwrap().width as f32 / 2.0;
        assert!(
            (at_1 - at_2).abs() <= 1.5,
            "{at_1} at a scale of 1, {at_2} at 2"
        );
    }

    #[test]
    fn rasterize_text_returns_non_empty_pixels() {
        let mut fa = atlas();
        let glyphs = fa
            .rasterize_text("Hello", 24.0, 0.0)
            .expect("rasterize_text returned None");
        assert!(!glyphs.rgba_pixels.is_empty());
        assert_eq!(
            glyphs.rgba_pixels.len(),
            (glyphs.width * glyphs.height * 4) as usize
        );
    }

    #[test]
    fn rasterize_text_pixels_are_white_with_alpha() {
        let mut fa = atlas();
        let glyphs = fa
            .rasterize_text("A", 48.0, 0.0)
            .expect("rasterize expected to succeed");
        for chunk in glyphs.rgba_pixels.chunks_exact(4) {
            if chunk[3] > 0 {
                assert_eq!(
                    &chunk[..3],
                    &[255, 255, 255],
                    "white where there's coverage"
                );
            }
        }
    }

    #[test]
    fn rasterize_empty_text_returns_none() {
        let mut fa = atlas();
        assert!(fa.rasterize_text("", 24.0, 0.0).is_none());
    }

    #[test]
    fn rasterize_whitespace_returns_none() {
        let mut fa = atlas();
        assert!(fa.rasterize_text("   ", 24.0, 0.0).is_none());
    }

    #[test]
    fn rasterize_text_sizes_12_to_48_succeed() {
        let mut fa = atlas();
        for size in [12.0_f32, 16.0, 24.0, 32.0, 48.0] {
            let r = fa.rasterize_text("Ag", size, 0.0);
            let r = r.unwrap_or_else(|| panic!("rasterize_text failed at {size}px"));
            assert!(r.width > 0 && r.height > 0, "zero-size glyphs at {size}px");
        }
    }

    #[test]
    fn rasterize_text_is_the_same_each_time() {
        let mut fa = atlas();
        let first = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        let again = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        assert_eq!(first.width, again.width);
        assert_eq!(first.height, again.height);
        assert_eq!(first.rgba_pixels, again.rgba_pixels);
    }

    #[test]
    fn rasterize_text_wider_spacing_widens_bounding_box() {
        let mut fa = atlas();
        let tight = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        let tracked = fa.rasterize_text("PROTEUS", 40.0, 10.0).unwrap();
        // 7 glyphs → 6 gaps of 10px extra.
        assert!(
            tracked.width >= tight.width + 55,
            "expected meaningfully wider run: tight={} tracked={}",
            tight.width,
            tracked.width
        );
        assert_eq!(
            tracked.height, tight.height,
            "letter spacing must not affect line height"
        );
    }

    #[test]
    fn rasterize_text_single_glyph_has_no_trailing_gap() {
        let mut fa = atlas();
        let no_spacing = fa.rasterize_text("A", 40.0, 0.0).unwrap();
        let with_spacing = fa.rasterize_text("A", 40.0, 20.0).unwrap();
        assert_eq!(
            no_spacing.width, with_spacing.width,
            "a single glyph has no gap to insert tracking into"
        );
    }

    // A `\n` starts a new line, below the first.
    #[test]
    fn a_newline_starts_a_new_line() {
        let mut fa = atlas();
        let one = fa.rasterize_text("Hello", 24.0, 0.0).unwrap();
        let two = fa.rasterize_text("Hello\nHello", 24.0, 0.0).unwrap();
        assert_eq!(two.width, one.width);
        assert!(
            two.height >= 2 * one.height - 1,
            "{} for two lines, {} for one",
            two.height,
            one.height
        );
    }

    #[test]
    fn bytes_that_arent_a_font_are_an_error() {
        assert!(FontAtlas::new(b"not a font").is_err());
        assert!(FontAtlas::new(&[]).is_err());
    }
}
