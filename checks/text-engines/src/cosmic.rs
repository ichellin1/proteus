//! cosmic-text, which shapes with harfrust, reads fonts with skrifa and
//! fontdb, and rasterizes with swash.

use cosmic_text::{
    fontdb, Align as CAlign, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style,
    SwashCache, SwashContent, Weight, Wrap,
};

use crate::{fonts, Align, Case, Engine, Face, GlyphImage, Mask, Support};

/// Fallback fonts: the app's own, never the system's.
struct AppFallback;

impl cosmic_text::Fallback for AppFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &["Noto Sans Devanagari"]
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, _script: unicode_script::Script, _locale: &str) -> &[&'static str] {
        &[]
    }
}

pub struct Cosmic {
    /// Only Inter Bold, as Proteus has today: static Inter Bold and variable
    /// Inter share a family name, so each has a font system of its own.
    bold: FontSystem,
    /// Variable Inter and Inter Italic, with Noto Sans Devanagari to fall
    /// back on.
    #[cfg(feature = "all-fonts")]
    inter: FontSystem,
    raster: SwashCache,
    cached: bool,
}

fn system(fonts: &[&'static [u8]]) -> FontSystem {
    let mut db = fontdb::Database::new();
    for bytes in fonts {
        db.load_font_data(bytes.to_vec());
    }
    FontSystem::new_with_locale_and_db_and_fallback("en-US".into(), db, AppFallback)
}

impl Default for Cosmic {
    fn default() -> Self {
        Self {
            bold: system(&[fonts::INTER_BOLD]),
            #[cfg(feature = "all-fonts")]
            inter: system(&[fonts::INTER, fonts::INTER_ITALIC, fonts::DEVANAGARI]),
            raster: SwashCache::new(),
            cached: false,
        }
    }
}

/// The font's own line spacing at `size`: cosmic-text needs a line height in
/// pixels, and has no default.
fn line_height(size: f32) -> f32 {
    let font = swash::FontRef::from_index(fonts::INTER_BOLD, 0).unwrap();
    let m = font.metrics(&[]);
    (m.ascent + m.descent + m.leading) * size / m.units_per_em as f32
}

impl Cosmic {
    fn lay_out(&mut self, case: &Case, scale: f32) -> Buffer {
        let size = case.size * scale;
        #[cfg(feature = "all-fonts")]
        let fs = if case.spans[0].face == Face::InterBold {
            &mut self.bold
        } else {
            &mut self.inter
        };
        #[cfg(not(feature = "all-fonts"))]
        let fs = &mut self.bold;
        let mut buffer = Buffer::new(fs, Metrics::new(size, line_height(size)));
        buffer.set_wrap(if case.width.is_some() {
            Wrap::Word
        } else {
            Wrap::None
        });
        buffer.set_size(case.width.map(|w| w * scale), None);
        let default = Attrs::new().family(Family::Name("Inter"));
        let spans = case.spans.iter().map(|s| {
            let mut attrs = default.clone().weight(Weight(s.weight));
            if s.italic {
                attrs = attrs.style(Style::Italic);
            }
            (s.text, attrs)
        });
        let align = match case.align {
            Align::Start => CAlign::Left,
            Align::Center => CAlign::Center,
            Align::End => CAlign::Right,
        };
        buffer.set_rich_text(spans, &default, Shaping::Advanced, Some(align));
        buffer.shape_until_scroll(fs, false);
        buffer
    }

    fn size(buffer: &Buffer, case: &Case, scale: f32) -> (f32, f32, usize) {
        let mut width = case.width.map_or(0.0, |w| w * scale);
        let mut height = 0.0f32;
        let mut lines = 0;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height = height.max(run.line_top + run.line_height);
            lines += 1;
        }
        (width, height, lines)
    }
}

impl Engine for Cosmic {
    fn name(&self) -> &'static str {
        "cosmic-text"
    }

    fn support(&self) -> Support {
        Support {
            shaping: "Yes: harfrust",
            kerning: "Yes: GPOS, through shaping",
            variable_weights: "Weight axis only",
            fallback: "Yes: an app list, plus per-script lists",
            spans: "Yes: rich text with attributes per span",
            align: "Left, right, center, end",
            justify: "Yes",
            ellipsis: "Yes: start, middle or end, by lines or height",
            inline_boxes: "No",
            measure_without_raster: "Yes",
        }
    }

    fn set_glyph_cache(&mut self, on: bool) {
        self.cached = on;
        self.raster = SwashCache::new();
    }

    fn measure(&mut self, case: &Case, scale: f32) -> (f32, f32) {
        let buffer = self.lay_out(case, scale);
        let (w, h, _) = Self::size(&buffer, case, scale);
        (w / scale, h / scale)
    }

    fn render(&mut self, case: &Case, scale: f32) -> Mask {
        let buffer = self.lay_out(case, scale);
        let (w, h, lines) = Self::size(&buffer, case, scale);
        #[cfg(feature = "all-fonts")]
        let fs = if case.spans[0].face == Face::InterBold {
            &mut self.bold
        } else {
            &mut self.inter
        };
        #[cfg(not(feature = "all-fonts"))]
        let fs = &mut self.bold;
        let mut images = Vec::new();
        let mut left = 0i32;
        let mut right = w.ceil() as i32;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                let image = if self.cached {
                    self.raster.get_image(fs, physical.cache_key).clone()
                } else {
                    self.raster.get_image_uncached(fs, physical.cache_key)
                };
                let Some(image) = image else {
                    continue;
                };
                if image.content != SwashContent::Mask || image.placement.width == 0 {
                    continue;
                }
                let x = physical.x + image.placement.left;
                let y = physical.y - image.placement.top;
                left = left.min(x);
                right = right.max(x + image.placement.width as i32);
                images.push((x, y, image));
            }
        }
        let mut mask = Mask::new((right - left) as u32, h.ceil() as u32, lines);
        for (x, y, image) in &images {
            mask.draw(&GlyphImage {
                left: x - left,
                top: *y,
                width: image.placement.width,
                height: image.placement.height,
                alpha: &image.data,
            });
        }
        mask
    }
}
