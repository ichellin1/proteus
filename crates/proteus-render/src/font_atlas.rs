//! CPU-side font rasterizer for pre-baked text rendering.
//!
//! [`FontAtlas`] rasterizes text strings into RGBA pixel buffers using [`fontdue`]. The caller
//! registers a `main_atlas` region for those pixels via
//! [`crate::TextureRegistry::register_static`], then uploads them via
//! [`crate::QuadPipeline::write_to_main_atlas`].
//!
//! ## Approach — text-as-texture
//!
//! A complete text string (e.g. "Hello World") is rasterized at its declared pixel size into a
//! single RGBA image:
//! - R=G=B=255 throughout (white — enables color tinting via `QuadInstance::color`)
//! - A = per-pixel glyph coverage from fontdue's anti-aliased rasterizer
//!
//! That image is written into a `main_atlas` region and the entity's `QuadInstance` UV fields are
//! pointed at that region. The result is treated identically to any other textured quad —
//! transitions, color tinting, and corner-radius rounding all work without special-casing text.
//!
//! ## No allocation here (M11)
//!
//! Before M11, `FontAtlas` owned an append-only shelf packer and allocated its own `main_atlas`
//! regions (`bake_text`/`bake_image`/`reserve_region`) — the only reason `bake_image`, which
//! decodes nothing itself, existed at all was to share that one packer's cursor with `bake_text`
//! so the two never silently overlapped. M11 replaced that packer with
//! [`crate::TextureRegistry`]'s real etagere-backed allocator, so `FontAtlas` narrows to what it's
//! actually unique at — font rasterization — and `bake_image` is gone: a decoded image's pixels go
//! straight from [`crate::decode_image`] to `TextureRegistry::register_static`, no `FontAtlas`
//! involvement needed.
//!
//! ## Embedded font
//!
//! [`EMBEDDED_FONT_BYTES`] holds DejaVu Sans Regular (Bitstream Vera / public domain license)
//! embedded at compile time. Callers can supply their own font bytes to [`FontAtlas::new`] for
//! custom typography.

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

/// DejaVu Sans Regular TTF, embedded at compile time.
///
/// License: Bitstream Vera (permissive, bundling allowed) + public domain
/// additions. See `assets/LICENSE-DejaVuSans.txt`.
pub const EMBEDDED_FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

// ---------------------------------------------------------------------------
// RasterizedGlyphs
// ---------------------------------------------------------------------------

/// The result of one [`FontAtlas::rasterize_text`] call — just pixels, no atlas placement.
/// Register a `main_atlas` region for them via [`crate::TextureRegistry::register_static`], then
/// upload via [`crate::QuadPipeline::write_to_main_atlas`].
#[derive(Debug, Clone)]
pub struct RasterizedGlyphs {
    /// Width of the rasterized text image in pixels.
    pub width: u32,
    /// Height of the rasterized text image in pixels.
    pub height: u32,
    /// RGBA pixel data. Length is `width * height * 4`.
    /// Premultiplied-alpha is NOT used — alpha is the raw glyph coverage.
    pub rgba_pixels: Vec<u8>,
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// CPU-side glyph rasterizer — no atlas allocation (see the module docs).
///
/// Create one per application session; share it across all text entities. A real bevy ECS
/// `Resource` (see the impl below) — inserted into the `World` once at shell startup rather than
/// kept as a plain shell-owned field — so `bake_pending_text`/`bake_system` can reach it from
/// inside the ECS schedule the same way `GpuContext`/`QuadPipeline` already can.
///
/// Call [`rasterize_text`] for each unique (string, size) pair, register a `main_atlas` region for
/// the result via `TextureRegistry::register_static`, then upload via
/// `QuadPipeline::write_to_main_atlas`.
///
/// [`rasterize_text`]: FontAtlas::rasterize_text
pub struct FontAtlas {
    font: fontdue::Font,
}

impl bevy_ecs::prelude::Resource for FontAtlas {}

impl FontAtlas {
    /// Create a new [`FontAtlas`] backed by the given TTF/OTF bytes.
    ///
    /// # Panics
    ///
    /// Panics if `font_bytes` cannot be parsed as a valid TTF or OTF file.
    pub fn new(font_bytes: &[u8]) -> Self {
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .expect("FontAtlas: failed to parse font bytes — ensure the data is a valid TTF/OTF");
        Self { font }
    }

    /// Create a [`FontAtlas`] using the [`EMBEDDED_FONT_BYTES`] (DejaVu Sans Regular).
    pub fn with_embedded_font() -> Self {
        Self::new(EMBEDDED_FONT_BYTES)
    }

    /// Rasterize `text` at `size_px` into an RGBA pixel buffer.
    ///
    /// Returns `None` if:
    /// - `text` is empty (or contains only whitespace that contributes no pixels).
    /// - The font has no usable line metrics at `size_px` (should not happen for valid fonts).
    pub fn rasterize_text(&mut self, text: &str, size_px: f32) -> Option<RasterizedGlyphs> {
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
        //    Height: font ascent + |descent| (in pixels). We query
        //    `horizontal_line_metrics` for the current px size.
        //
        //    Width: sum of all glyph advance widths (integer ceiling).
        // ------------------------------------------------------------------

        let line_metrics = self.font.horizontal_line_metrics(size_px)?;

        // Ascent is positive (above baseline), descent is negative (below).
        let ascent_px = line_metrics.ascent.ceil() as i32;
        let text_height = (line_metrics.ascent - line_metrics.descent).ceil() as u32;
        if text_height == 0 {
            return None;
        }

        let text_width: u32 = rasterized
            .iter()
            .map(|(m, _)| m.advance_width.ceil() as u32)
            .sum();
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

        for (metrics, bitmap) in &rasterized {
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
            .rasterize_text("Hello", 24.0)
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
            .rasterize_text("A", 48.0)
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
            .rasterize_text("X", 32.0)
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
        assert!(fa.rasterize_text("", 24.0).is_none());
    }

    #[test]
    fn rasterize_text_sizes_12_to_48_succeed() {
        let mut fa = atlas();
        for size in [12.0_f32, 16.0, 24.0, 32.0, 48.0] {
            let r = fa.rasterize_text("Ag", size);
            assert!(r.is_some(), "rasterize_text failed at {size}px");
            let r = r.unwrap();
            assert!(r.width > 0 && r.height > 0, "zero-size glyphs at {size}px");
        }
    }
}
