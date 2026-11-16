//! `fontdue`'s own layout module: the smallest change from today, which uses
//! `fontdue` to rasterize a character at a time.
//!
//! It has no shaping, so no ligatures or joined letters; it reads only a
//! font's `kern` table, which Inter doesn't have (its kerning is in `GPOS`);
//! and it can't vary a variable font, so every weight draws as the font's
//! default. It has no fallback either: this module picks a font for each
//! character itself, the first in the list that has it.

use fontdue::layout::{
    CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle, WrapStyle,
};
use fontdue::{Font, FontSettings};

use crate::{fonts, Align, Case, Engine, Face, GlyphImage, Mask, Support};

pub struct Fontdue {
    /// Inter Bold, then (with every font) variable Inter, Inter Italic and
    /// Noto Sans Devanagari.
    fonts: Vec<Font>,
    layout: Layout,
    cache: Option<std::collections::HashMap<fontdue::layout::GlyphRasterConfig, Vec<u8>>>,
}

impl Default for Fontdue {
    fn default() -> Self {
        let load = |bytes: &[u8]| Font::from_bytes(bytes, FontSettings::default()).unwrap();
        #[allow(unused_mut)]
        let mut list = vec![load(fonts::INTER_BOLD)];
        #[cfg(feature = "all-fonts")]
        list.extend([
            load(fonts::INTER),
            load(fonts::INTER_ITALIC),
            load(fonts::DEVANAGARI),
        ]);
        Self {
            fonts: list,
            layout: Layout::new(CoordinateSystem::PositiveYDown),
            cache: None,
        }
    }
}

impl Fontdue {
    fn lay_out(&mut self, case: &Case, scale: f32) {
        self.layout.reset(&LayoutSettings {
            max_width: case.width.map(|w| w * scale),
            horizontal_align: match case.align {
                Align::Start => HorizontalAlign::Left,
                Align::Center => HorizontalAlign::Center,
                Align::End => HorizontalAlign::Right,
            },
            wrap_style: WrapStyle::Word,
            ..LayoutSettings::default()
        });
        for span in &case.spans {
            let first = match (span.face, span.italic) {
                (Face::InterBold, _) => 0,
                (Face::Inter, false) => 1,
                (Face::Inter, true) => 2,
            };
            // A run for each change of font: the span's own, or the first
            // after it that has the character.
            let mut start = 0;
            let mut current = None;
            for (i, c) in span.text.char_indices() {
                let font = std::iter::once(first)
                    .chain(3..self.fonts.len())
                    .find(|&f| self.fonts[f].lookup_glyph_index(c) != 0)
                    .unwrap_or(first);
                if current.is_some_and(|f| f != font) {
                    let style =
                        TextStyle::new(&span.text[start..i], case.size * scale, current.unwrap());
                    self.layout.append(&self.fonts, &style);
                    start = i;
                }
                current = Some(font);
            }
            if let Some(font) = current {
                let style = TextStyle::new(&span.text[start..], case.size * scale, font);
                self.layout.append(&self.fonts, &style);
            }
        }
    }

    /// The text's size in pixels, and how far the ink starts left of 0.
    fn bounds(&self, case: &Case, scale: f32) -> (f32, f32, f32) {
        let mut left = 0.0f32;
        let mut right = case.width.map_or(0.0, |w| w * scale);
        for g in self.layout.glyphs() {
            if g.width > 0 {
                left = left.min(g.x);
                right = right.max(g.x + g.width as f32);
            }
        }
        (right - left, self.layout.height(), -left)
    }
}

impl Engine for Fontdue {
    fn name(&self) -> &'static str {
        "fontdue layout"
    }

    fn support(&self) -> Support {
        Support {
            shaping: "No: a glyph per character",
            kerning: "Only a `kern` table, which Inter lacks",
            variable_weights: "No: the font's default instance",
            fallback: "No: done by hand here",
            spans: "Yes: a style per appended run",
            align: "Start, center, end",
            justify: "No",
            ellipsis: "No",
            inline_boxes: "No",
            measure_without_raster: "Yes",
        }
    }

    fn set_glyph_cache(&mut self, on: bool) {
        self.cache = on.then(Default::default);
    }

    fn measure(&mut self, case: &Case, scale: f32) -> (f32, f32) {
        self.lay_out(case, scale);
        let (w, h, _) = self.bounds(case, scale);
        (w / scale, h / scale)
    }

    fn render(&mut self, case: &Case, scale: f32) -> Mask {
        self.lay_out(case, scale);
        let (w, h, shift) = self.bounds(case, scale);
        let lines = self.layout.lines().map_or(0, |l| l.len());
        let mut mask = Mask::new(w.ceil() as u32, h.ceil() as u32, lines);
        for g in self.layout.glyphs() {
            if g.width == 0 {
                continue;
            }
            let fonts = &self.fonts;
            let fresh;
            let bitmap: &[u8] = match &mut self.cache {
                Some(cache) => cache
                    .entry(g.key)
                    .or_insert_with(|| fonts[g.font_index].rasterize_config(g.key).1),
                None => {
                    fresh = fonts[g.font_index].rasterize_config(g.key).1;
                    &fresh
                }
            };
            mask.draw(&GlyphImage {
                left: (g.x + shift).round() as i32,
                top: g.y.round() as i32,
                width: g.width as u32,
                height: g.height as u32,
                alpha: bitmap,
            });
        }
        mask
    }
}
