//! CPU-side font atlas for pre-baked text rendering.
//!
//! [`FontAtlas`] rasterizes text strings into RGBA pixel buffers using
//! [`fontdue`], then the caller uploads those pixels to a region in the GPU's
//! `main_atlas` via [`QuadPipeline::write_to_main_atlas`].
//!
//! ## Approach — text-as-texture
//!
//! A complete text string (e.g. "Hello World") is rasterized at its declared
//! pixel size into a single RGBA image:
//! - R=G=B=255 throughout (white — enables color tinting via `QuadInstance::color`)
//! - A = per-pixel glyph coverage from fontdue's anti-aliased rasterizer
//!
//! That image is written into a region of `main_atlas` and the entity's
//! `QuadInstance` UV fields are pointed at that region. The result is treated
//! identically to any other textured quad — transitions, color tinting, and
//! corner-radius rounding all work without special-casing text.
//!
//! ## Atlas packing
//!
//! A simple shelf (row) packer is used. Each call to [`FontAtlas::bake_text`]
//! claims a new horizontal run of pixels. Rows are never freed during a session.
//! This is sufficient for M4 (Phase 1 text). A proper LRU + etagere packer
//! is planned for a later milestone when text changes dynamically at runtime.
//!
//! ## Embedded font
//!
//! [`EMBEDDED_FONT_BYTES`] holds DejaVu Sans Regular (Bitstream Vera / public
//! domain license) embedded at compile time. Callers can supply their own font
//! bytes to [`FontAtlas::new`] for custom typography.

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

/// DejaVu Sans Regular TTF, embedded at compile time.
///
/// License: Bitstream Vera (permissive, bundling allowed) + public domain
/// additions. See `assets/LICENSE-DejaVuSans.txt`.
pub const EMBEDDED_FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

// ---------------------------------------------------------------------------
// BakedRegion
// ---------------------------------------------------------------------------

/// The result of one [`FontAtlas::bake_text`] call — pixel data plus the
/// region within `main_atlas` where those pixels should be uploaded.
#[derive(Debug, Clone)]
pub struct BakedRegion {
    /// X origin of this region within the atlas (pixel coordinates).
    pub x: u32,
    /// Y origin of this region within the atlas (pixel coordinates).
    pub y: u32,
    /// Width of the rasterized text image in pixels.
    pub width: u32,
    /// Height of the rasterized text image in pixels.
    pub height: u32,
    /// RGBA pixel data. Length is `width * height * 4`.
    /// Premultiplied-alpha is NOT used — alpha is the raw glyph coverage.
    pub rgba_pixels: Vec<u8>,
}

impl BakedRegion {
    /// UV offset into `main_atlas` for the top-left corner of this region.
    ///
    /// Divide by the atlas size (from [`super::pipeline::QuadPipeline::MAIN_ATLAS_SIZE`])
    /// to obtain normalised UVs suitable for `QuadInstance::uv_offset`.
    #[inline]
    pub fn uv_offset(&self, atlas_size: u32) -> [f32; 2] {
        let s = atlas_size as f32;
        [self.x as f32 / s, self.y as f32 / s]
    }

    /// UV scale that maps the unit quad's [0,1]×[0,1] UV space to this region.
    ///
    /// Assign to `QuadInstance::uv_scale`.
    #[inline]
    pub fn uv_scale(&self, atlas_size: u32) -> [f32; 2] {
        let s = atlas_size as f32;
        [self.width as f32 / s, self.height as f32 / s]
    }
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// CPU-side font atlas: glyph rasterizer + shelf packer.
///
/// Create one per application session; share it across all text entities.
/// Call [`bake_text`] for each unique (string, size) pair, then upload the
/// returned [`BakedRegion`] to the GPU atlas via
/// [`QuadPipeline::write_to_main_atlas`].
///
/// [`bake_text`]: FontAtlas::bake_text
pub struct FontAtlas {
    font: fontdue::Font,

    // Shelf packer state — simple row-based allocation.
    //
    // The atlas is logically divided into horizontal rows ("shelves"). Each
    // new allocation is appended to the current row; when it doesn't fit the
    // packer opens a new row.
    //
    // `cursor_x` / `cursor_y`: top-left corner of the next free slot.
    // `row_height`: tallest allocation on the current row (determines where
    //               the next row starts).
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,

    atlas_width: u32,
    atlas_height: u32,
}

impl FontAtlas {
    /// Pixel gap between atlas allocations to prevent linear-sampler bleed.
    const GAP: u32 = 1;

    /// Create a new [`FontAtlas`] backed by the given TTF/OTF bytes.
    ///
    /// `atlas_width` and `atlas_height` must match the dimensions of the GPU
    /// atlas texture this `FontAtlas` is packing into (typically
    /// [`QuadPipeline::MAIN_ATLAS_SIZE`] × [`QuadPipeline::MAIN_ATLAS_SIZE`]).
    ///
    /// `y_offset` — number of rows at the top of the atlas to skip. Pass a
    /// small value (e.g. `2`) to leave room for any sentinel pixels that were
    /// baked into the atlas at startup (such as the white-pixel at origin used
    /// by [`QuadPipeline`]).
    ///
    /// # Panics
    ///
    /// Panics if `font_bytes` cannot be parsed as a valid TTF or OTF file.
    ///
    /// [`QuadPipeline`]: super::pipeline::QuadPipeline
    pub fn new(font_bytes: &[u8], atlas_width: u32, atlas_height: u32, y_offset: u32) -> Self {
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .expect("FontAtlas: failed to parse font bytes — ensure the data is a valid TTF/OTF");

        Self {
            font,
            cursor_x: 0,
            cursor_y: y_offset,
            row_height: 0,
            atlas_width,
            atlas_height,
        }
    }

    /// Create a [`FontAtlas`] using the [`EMBEDDED_FONT_BYTES`] (DejaVu Sans Regular).
    pub fn with_embedded_font(atlas_width: u32, atlas_height: u32) -> Self {
        Self::new(EMBEDDED_FONT_BYTES, atlas_width, atlas_height, 2)
    }

    /// Rasterize `text` at `size_px` and pack it into the atlas.
    ///
    /// Returns `Some(BakedRegion)` on success. The caller must:
    /// 1. Upload `region.rgba_pixels` to the GPU atlas at `(region.x, region.y)`.
    /// 2. Compute `uv_offset = region.uv_offset(ATLAS_SIZE)` and
    ///    `uv_scale = region.uv_scale(ATLAS_SIZE)` and store them on the entity.
    ///
    /// Returns `None` if:
    /// - `text` is empty (or contains only whitespace that contributes no pixels).
    /// - The atlas is full (no region large enough for this text at this size).
    pub fn bake_text(&mut self, text: &str, size_px: f32) -> Option<BakedRegion> {
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
        // 3. Allocate a region in the atlas shelf packer.
        // ------------------------------------------------------------------

        let (atlas_x, atlas_y) = self.allocate(text_width, text_height)?;

        // ------------------------------------------------------------------
        // 4. Composite glyph bitmaps into a single RGBA pixel buffer.
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

        Some(BakedRegion {
            x: atlas_x,
            y: atlas_y,
            width: text_width,
            height: text_height,
            rgba_pixels: rgba,
        })
    }

    // ---------------------------------------------------------------------------
    // Shelf packer
    // ---------------------------------------------------------------------------

    /// Allocate a `width` × `height` region in the atlas.
    ///
    /// Returns the `(x, y)` top-left corner of the allocated region, or `None`
    /// if the atlas is exhausted.
    fn allocate(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        // Advance to the next row if this allocation doesn't fit horizontally.
        if self.cursor_x + width > self.atlas_width {
            self.cursor_y += self.row_height + Self::GAP;
            self.cursor_x = 0;
            self.row_height = 0;
        }

        // Fail if we've run out of vertical space.
        if self.cursor_y + height > self.atlas_height {
            log::warn!(
                "FontAtlas: atlas full — cannot allocate {}×{} at y={}",
                width,
                height,
                self.cursor_y,
            );
            return None;
        }

        let x = self.cursor_x;
        let y = self.cursor_y;

        self.cursor_x += width + Self::GAP;
        self.row_height = self.row_height.max(height);

        Some((x, y))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn atlas() -> FontAtlas {
        FontAtlas::with_embedded_font(2048, 2048)
    }

    #[test]
    fn bake_text_returns_non_empty_pixels() {
        let mut fa = atlas();
        let region = fa
            .bake_text("Hello", 24.0)
            .expect("bake_text returned None");
        assert!(!region.rgba_pixels.is_empty());
        assert_eq!(
            region.rgba_pixels.len(),
            (region.width * region.height * 4) as usize
        );
    }

    #[test]
    fn bake_text_pixels_are_white_with_alpha() {
        let mut fa = atlas();
        let region = fa.bake_text("A", 48.0).expect("bake expected to succeed");
        // Every non-transparent pixel must have R=G=B=255.
        for chunk in region.rgba_pixels.chunks_exact(4) {
            let (r, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
            if a > 0 {
                assert_eq!(r, 255, "R should be 255 where alpha > 0");
                assert_eq!(g, 255, "G should be 255 where alpha > 0");
                assert_eq!(b, 255, "B should be 255 where alpha > 0");
            }
        }
    }

    #[test]
    fn bake_text_has_some_opaque_pixels() {
        let mut fa = atlas();
        let region = fa.bake_text("X", 32.0).expect("bake should succeed");
        let has_visible = region.rgba_pixels.chunks_exact(4).any(|c| c[3] > 0);
        assert!(
            has_visible,
            "rasterized glyph should have at least one visible pixel"
        );
    }

    #[test]
    fn bake_text_uv_in_unit_range() {
        const ATLAS: u32 = 2048;
        let mut fa = atlas();
        let r = fa.bake_text("Test", 20.0).expect("bake should succeed");
        let [ox, oy] = r.uv_offset(ATLAS);
        let [sx, sy] = r.uv_scale(ATLAS);
        assert!((0.0..=1.0).contains(&ox), "uv_offset.x out of range: {ox}");
        assert!((0.0..=1.0).contains(&oy), "uv_offset.y out of range: {oy}");
        assert!(sx > 0.0 && sx <= 1.0, "uv_scale.x out of range: {sx}");
        assert!(sy > 0.0 && sy <= 1.0, "uv_scale.y out of range: {sy}");
        assert!(ox + sx <= 1.0, "UV region exceeds atlas width");
        assert!(oy + sy <= 1.0, "UV region exceeds atlas height");
    }

    #[test]
    fn bake_text_successive_allocations_do_not_overlap() {
        let mut fa = atlas();
        let r1 = fa.bake_text("Hello", 24.0).unwrap();
        let r2 = fa.bake_text("World", 24.0).unwrap();

        // Regions must not overlap.
        fn overlaps(a: &BakedRegion, b: &BakedRegion) -> bool {
            let ax2 = a.x + a.width;
            let ay2 = a.y + a.height;
            let bx2 = b.x + b.width;
            let by2 = b.y + b.height;
            a.x < bx2 && ax2 > b.x && a.y < by2 && ay2 > b.y
        }

        assert!(
            !overlaps(&r1, &r2),
            "successive allocations overlap: {r1:?} vs {r2:?}"
        );
    }

    #[test]
    fn bake_empty_text_returns_none() {
        let mut fa = atlas();
        assert!(fa.bake_text("", 24.0).is_none());
    }

    #[test]
    fn bake_text_sizes_12_to_48_succeed() {
        let mut fa = atlas();
        for size in [12.0_f32, 16.0, 24.0, 32.0, 48.0] {
            let r = fa.bake_text("Ag", size);
            assert!(r.is_some(), "bake_text failed at {size}px");
            let r = r.unwrap();
            assert!(r.width > 0 && r.height > 0, "zero-size region at {size}px");
        }
    }
}
