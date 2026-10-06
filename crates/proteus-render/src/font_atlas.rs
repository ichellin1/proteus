//! Rasterizing text into pixels, on the CPU.
//!
//! [`FontAtlas`] rasterizes a line of text into an RGBA image with `fontdue`.
//! The caller then allocates a main-atlas region for it with
//! [`crate::TextureRegistry::register_static`] and uploads it with
//! [`crate::QuadPipeline::write_to_main_atlas`].
//!
//! ## Text as a texture
//!
//! The whole string becomes one image, at its size in pixels:
//! - red, green and blue are 255 everywhere, so the text can take any color;
//! - alpha is each pixel's glyph coverage, antialiased.
//!
//! The text is then drawn like any other texture: it transitions, takes a
//! color, and is clipped by rounded corners, with no special handling.
//!
//! ## Embedded font
//!
//! [`EMBEDDED_FONT_BYTES`] is Inter Bold (SIL Open Font License 1.1). To use
//! another font, pass its bytes to [`FontAtlas::new`].
//!
//! `fontdue` can't use a variable font's weight axis, and Google Fonts only
//! ships Inter as a variable font. So `assets/Inter-Bold.ttf` is a static
//! instance at weight 700 and optical size 14, made with `fonttools`:
//! `fonttools varLib.instancer --update-name-table Inter-Variable.ttf wght=700 opsz=14`.

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

/// Inter Bold TTF (static instance, see the module docs), embedded at compile time.
///
/// License: SIL Open Font License 1.1 (permissive, bundling allowed). See
/// `assets/LICENSE-Inter.txt`.
pub const EMBEDDED_FONT_BYTES: &[u8] = include_bytes!("../assets/Inter-Bold.ttf");

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

/// Rasterizes text with one font, on the CPU. It doesn't allocate atlas space
/// (see the module docs).
///
/// One is enough for an app: the renderer owns one and uses it for all text.
/// Call [`rasterize_text`] for each string and size, allocate a main-atlas
/// region for the result with `TextureRegistry::register_static`, then upload
/// it with `QuadPipeline::write_to_main_atlas`.
///
/// [`rasterize_text`]: FontAtlas::rasterize_text
pub struct FontAtlas {
    font: fontdue::Font,
}

impl FontAtlas {
    /// Creates a [`FontAtlas`] for the TTF or OTF font in `font_bytes`.
    ///
    /// # Panics
    ///
    /// Panics if `font_bytes` cannot be parsed as a valid TTF or OTF file.
    pub fn new(font_bytes: &[u8]) -> Self {
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .expect("FontAtlas: failed to parse font bytes — ensure the data is a valid TTF/OTF");
        Self { font }
    }

    /// Creates a [`FontAtlas`] for the embedded font, [`EMBEDDED_FONT_BYTES`]
    /// (Inter Bold).
    pub fn with_embedded_font() -> Self {
        Self::new(EMBEDDED_FONT_BYTES)
    }

    /// Rasterizes `text` at `size_px` into an RGBA pixel buffer, with
    /// `letter_spacing_px` of extra space between glyphs (`0.0` for the font's
    /// normal spacing). No space is added after the last glyph, so the result
    /// has no trailing padding.
    ///
    /// Returns `None` if `text` draws no pixels (it's empty, or only
    /// whitespace), or if the font has no line height at `size_px`.
    pub fn rasterize_text(
        &mut self,
        text: &str,
        size_px: f32,
        letter_spacing_px: f32,
    ) -> Option<RasterizedGlyphs> {
        if text.is_empty() {
            return None;
        }

        // ------------------------------------------------------------------
        // 1. Rasterize each glyph and collect metrics.
        // ------------------------------------------------------------------

        let chars: Vec<char> = text.chars().collect();
        let rasterized: Vec<(fontdue::Metrics, Vec<u8>)> = chars
            .iter()
            .map(|&c| self.font.rasterize(c, size_px))
            .collect();

        // ------------------------------------------------------------------
        // 2. Compute the total bounding box for the text run.
        //
        //    Height: the font's ascent plus its descent, in pixels, from
        //    `horizontal_line_metrics` at this size.
        //
        //    Width: from the leftmost pen position to the rightmost ink pixel,
        //    with `letter_spacing_px` between each pair of glyphs (n - 1 gaps
        //    for n glyphs, so there's no trailing gap).
        // ------------------------------------------------------------------

        let line_metrics = self.font.horizontal_line_metrics(size_px)?;

        // Ascent is positive (above baseline), descent is negative (below).
        // `ascent_px`/`descent_px` are ceiled *independently*, not
        // `(ascent - descent).ceil()` as a single sum — ceiling isn't
        // linear, so rounding the combined height could allocate up to
        // ~1px less room below the baseline than the font's true
        // (unrounded) descent needs. `glyph_top`'s placement below is
        // anchored to `ascent_px` alone (see its own doc), so any shortfall
        // there always eats into the space *below* it — i.e. the bottom
        // row of descenders (g/y/p/q/j) or below-baseline diacritics gets
        // silently dropped by the bounds check a few lines down. Ceiling
        // each term on its own guarantees `text_height - ascent_px` is
        // always >= the true descent magnitude.
        let ascent_px = line_metrics.ascent.ceil() as i32;
        let descent_px = (-line_metrics.descent).ceil() as i32;
        let text_height = (ascent_px + descent_px) as u32;
        if text_height == 0 {
            return None;
        }

        let glyph_count = rasterized.len();

        // Width: a glyph's ink can extend past its own advance box (common
        // for many glyphs — decorative caps, descenders like "y"/"j", or
        // just ordinary overhang — and most visible on the *last* glyph,
        // since there's no following glyph's space for it to overlap
        // into). Sizing from the sum of advance widths would clip that
        // overhang at the right edge, so this walks the same pen advances as
        // the compositing loop below and tracks the rightmost ink pixel any
        // glyph reaches, not just where the pen ends up.
        let mut pen_x_probe: i32 = 0;
        let mut max_right: i32 = 0;
        for (i, (metrics, _)) in rasterized.iter().enumerate() {
            let glyph_left = pen_x_probe + metrics.xmin;
            max_right = max_right.max(glyph_left + metrics.width as i32);
            pen_x_probe += metrics.advance_width.ceil() as i32;
            if i + 1 < glyph_count {
                pen_x_probe += letter_spacing_px.round() as i32;
            }
        }
        let text_width = max_right.max(0) as u32;
        if text_width == 0 {
            return None;
        }

        // ------------------------------------------------------------------
        // 3. Composite glyph bitmaps into a single RGBA pixel buffer.
        //
        //    Layout: white (R=G=B=255), coverage in alpha channel.
        //    This lets the shader tint text via `QuadInstance::color` without
        //    any special-casing.
        // ------------------------------------------------------------------

        let mut rgba = vec![0u8; (text_width * text_height * 4) as usize];

        let mut pen_x: i32 = 0;

        for (i, (metrics, bitmap)) in rasterized.iter().enumerate() {
            // glyph_left: horizontal offset of the glyph's left edge from pen_x.
            // metrics.xmin is the bearing from pen position to the left edge of the
            // visible glyph pixels. For most Latin characters this is ≥ 0.
            let glyph_left: i32 = pen_x + metrics.xmin;

            // glyph_top (in Y-down image coords): fontdue uses Y-up for ymin/height.
            //   ymin = pixels from baseline to glyph BOTTOM (Y-up, may be negative for descenders)
            //   Glyph top (Y-up from baseline) = ymin + height
            //   Glyph top (Y-down from buffer top) = ascent_px - (ymin + height as i32)
            let glyph_top: i32 = ascent_px - (metrics.ymin + metrics.height as i32);

            for gy in 0..metrics.height {
                for gx in 0..metrics.width {
                    let px = glyph_left + gx as i32;
                    let py = glyph_top + gy as i32;

                    // Skip pixels that land outside the text bounding box.
                    if px < 0 || py < 0 || px >= text_width as i32 || py >= text_height as i32 {
                        continue;
                    }

                    let coverage = bitmap[gy * metrics.width + gx];
                    if coverage == 0 {
                        continue; // transparent — skip to avoid zeroing any overlapping bg
                    }

                    let idx = ((py as u32 * text_width + px as u32) * 4) as usize;
                    rgba[idx] = 255; // R — white base
                    rgba[idx + 1] = 255; // G
                    rgba[idx + 2] = 255; // B
                    rgba[idx + 3] = coverage; // A = glyph coverage
                }
            }

            pen_x += metrics.advance_width.ceil() as i32;
            if i + 1 < glyph_count {
                pen_x += letter_spacing_px.round() as i32;
            }
        }

        Some(RasterizedGlyphs {
            width: text_width,
            height: text_height,
            rgba_pixels: rgba,
        })
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
        // Every non-transparent pixel must have R=G=B=255.
        for chunk in glyphs.rgba_pixels.chunks_exact(4) {
            let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
            if a > 0 {
                assert_eq!(r, 255, "R should be 255 where alpha > 0");
                assert_eq!(g, 255, "G should be 255 where alpha > 0");
                assert_eq!(b, 255, "B should be 255 where alpha > 0");
            }
        }
    }

    #[test]
    fn rasterize_text_has_some_opaque_pixels() {
        let mut fa = atlas();
        let glyphs = fa
            .rasterize_text("X", 32.0, 0.0)
            .expect("rasterize should succeed");
        let has_visible = glyphs.rgba_pixels.chunks_exact(4).any(|c| c[3] > 0);
        assert!(
            has_visible,
            "rasterized glyph should have at least one visible pixel"
        );
    }

    #[test]
    fn rasterize_empty_text_returns_none() {
        let mut fa = atlas();
        assert!(fa.rasterize_text("", 24.0, 0.0).is_none());
    }

    #[test]
    fn rasterize_text_sizes_12_to_48_succeed() {
        let mut fa = atlas();
        for size in [12.0_f32, 16.0, 24.0, 32.0, 48.0] {
            let r = fa.rasterize_text("Ag", size, 0.0);
            assert!(r.is_some(), "rasterize_text failed at {size}px");
            let r = r.unwrap();
            assert!(r.width > 0 && r.height > 0, "zero-size glyphs at {size}px");
        }
    }

    #[test]
    fn rasterize_text_zero_spacing_matches_rasterize_text() {
        let mut fa = atlas();
        let tracked = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        let plain = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        assert_eq!(tracked.width, plain.width);
        assert_eq!(tracked.height, plain.height);
        assert_eq!(tracked.rgba_pixels, plain.rgba_pixels);
    }

    #[test]
    fn rasterize_text_wider_spacing_widens_bounding_box() {
        let mut fa = atlas();
        let tight = fa.rasterize_text("PROTEUS", 40.0, 0.0).unwrap();
        let tracked = fa.rasterize_text("PROTEUS", 40.0, 10.0).unwrap();
        // 7 glyphs → 6 gaps of ~10px extra.
        assert!(
            tracked.width > tight.width + 50,
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
}
