//! parley, which shapes with harfrust and reads fonts with skrifa and
//! fontique, rasterized with swash, as parley's own examples do. parley lays
//! text out and leaves rasterizing to the app.

use std::sync::Arc;

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache, SourceCacheOptions};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, InlineBox,
    InlineBoxKind, Layout, LayoutContext, PositionedLayoutItem, StyleProperty, VerticalAlign,
};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};

use crate::{fonts, Align, Case, Engine, Face, GlyphImage, Mask, Support};

pub struct Parley {
    /// Only Inter Bold, as Proteus has today: static Inter Bold and variable
    /// Inter share a family name, so each has a font context of its own.
    bold: FontContext,
    /// Variable Inter and Inter Italic, and Noto Sans Devanagari.
    #[cfg(feature = "all-fonts")]
    inter: FontContext,
    layout: LayoutContext<()>,
    scale: ScaleContext,
    /// Glyph images by font, glyph, size, variation and a quarter-pixel
    /// horizontal offset, as cosmic-text's cache keys them.
    cache: Option<std::collections::HashMap<GlyphKey, Option<swash::scale::image::Image>>>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u64,
    index: u32,
    glyph: u32,
    size_bits: u32,
    coords: Vec<i16>,
    quarter: u8,
}

fn context(fonts: &[&'static [u8]]) -> FontContext {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    for bytes in fonts {
        collection.register_fonts(Blob::new(Arc::new(*bytes)), None);
    }
    FontContext {
        collection,
        source_cache: SourceCache::new(SourceCacheOptions::default()),
    }
}

impl Default for Parley {
    fn default() -> Self {
        Self {
            bold: context(&[fonts::INTER_BOLD]),
            #[cfg(feature = "all-fonts")]
            inter: context(&[fonts::INTER, fonts::INTER_ITALIC, fonts::DEVANAGARI]),
            layout: LayoutContext::new(),
            scale: ScaleContext::new(),
            cache: None,
        }
    }
}

impl Parley {
    fn lay_out(&mut self, case: &Case, scale: f32) -> Layout<()> {
        #[cfg(feature = "all-fonts")]
        let fcx = if case.spans[0].face == Face::InterBold {
            &mut self.bold
        } else {
            &mut self.inter
        };
        #[cfg(not(feature = "all-fonts"))]
        let fcx = &mut self.bold;
        let _ = Face::Inter;
        let text = case.text();
        let mut builder = self.layout.ranged_builder(fcx, &text, scale, true);
        // The app's ordered fallback list, as a CSS font stack.
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(
            "Inter, Noto Sans Devanagari".into(),
        )));
        builder.push_default(StyleProperty::FontSize(case.size));
        let mut start = 0;
        for span in &case.spans {
            let range = start..start + span.text.len();
            builder.push(
                StyleProperty::FontWeight(FontWeight::new(span.weight as f32)),
                range.clone(),
            );
            if span.italic {
                builder.push(StyleProperty::FontStyle(FontStyle::Italic), range.clone());
            }
            start = range.end;
        }
        if let (Some(side), Some(index)) = (case.inline_box, text.find('\u{FFFC}')) {
            builder.push_inline_box(InlineBox {
                id: 0,
                kind: InlineBoxKind::InFlow,
                index,
                width: side * scale,
                height: side * scale,
                baseline: None,
                vertical_align: VerticalAlign::BASELINE,
            });
        }
        let mut layout: Layout<()> = builder.build(&text);
        layout.break_all_lines(case.width.map(|w| w * scale));
        let alignment = match case.align {
            Align::Start => Alignment::Start,
            Align::Center => Alignment::Center,
            Align::End => Alignment::End,
        };
        layout.set_alignment(alignment, AlignmentOptions::default());
        layout
    }

    fn size(layout: &Layout<()>, case: &Case, scale: f32) -> (f32, f32) {
        let width = case.width.map_or(layout.full_width(), |w| w * scale);
        (width, layout.height())
    }
}

impl Engine for Parley {
    fn name(&self) -> &'static str {
        "parley + swash"
    }

    fn support(&self) -> Support {
        Support {
            shaping: "Yes: harfrust",
            kerning: "Yes: GPOS, through shaping",
            variable_weights: "Yes: any axis",
            fallback: "Yes: a CSS-style font stack, plus per-script lists",
            spans: "Yes: ranged or tree builders",
            align: "Start, end, left, right, center",
            justify: "Yes",
            ellipsis: "No",
            inline_boxes: "Yes",
            measure_without_raster: "Yes",
        }
    }

    fn set_glyph_cache(&mut self, on: bool) {
        self.cache = on.then(Default::default);
    }

    fn measure(&mut self, case: &Case, scale: f32) -> (f32, f32) {
        let layout = self.lay_out(case, scale);
        let (w, h) = Self::size(&layout, case, scale);
        (w / scale, h / scale)
    }

    fn render(&mut self, case: &Case, scale: f32) -> Mask {
        let layout = self.lay_out(case, scale);
        let (w, h) = Self::size(&layout, case, scale);
        let mut images = Vec::new();
        let mut boxes = Vec::new();
        let mut left = 0i32;
        let mut right = w.ceil() as i32;
        for line in layout.lines() {
            for item in line.items() {
                let glyph_run = match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => glyph_run,
                    PositionedLayoutItem::InlineBox(b) => {
                        boxes.push((b.x, b.y, b.width, b.height));
                        continue;
                    }
                };
                let run = glyph_run.run();
                let font = run.font();
                let font_ref =
                    swash::FontRef::from_index(font.data.as_ref(), font.index as usize).unwrap();
                let mut scaler = self
                    .scale
                    .builder(font_ref)
                    .size(run.font_size())
                    .hint(false)
                    .normalized_coords(run.normalized_coords().iter().map(|c| c.to_bits()))
                    .build();
                let coords: Vec<i16> = run
                    .normalized_coords()
                    .iter()
                    .map(|c| c.to_bits())
                    .collect();
                for g in glyph_run.positioned_glyphs() {
                    let mut render = |offset: Vector| {
                        Render::new(&[Source::Outline])
                            .format(Format::Alpha)
                            .offset(offset)
                            .render(&mut scaler, g.id as u16)
                    };
                    let image = match &mut self.cache {
                        Some(cache) => {
                            let quarter = (g.x.fract() * 4.0).floor() as u8;
                            let key = GlyphKey {
                                font: font.data.id(),
                                index: font.index,
                                glyph: g.id,
                                size_bits: run.font_size().to_bits(),
                                coords: coords.clone(),
                                quarter,
                            };
                            cache
                                .entry(key)
                                .or_insert_with(|| render(Vector::new(quarter as f32 / 4.0, 0.0)))
                                .clone()
                        }
                        None => render(Vector::new(g.x.fract(), g.y.fract())),
                    };
                    let Some(image) = image else {
                        continue;
                    };
                    if image.placement.width == 0 {
                        continue;
                    }
                    let x = g.x.floor() as i32 + image.placement.left;
                    let y = g.y.floor() as i32 - image.placement.top;
                    left = left.min(x);
                    right = right.max(x + image.placement.width as i32);
                    images.push((x, y, image));
                }
            }
        }
        let mut mask = Mask::new((right - left) as u32, h.ceil() as u32, layout.len());
        for (x, y, image) in &images {
            mask.draw(&GlyphImage {
                left: x - left,
                top: *y,
                width: image.placement.width,
                height: image.placement.height,
                alpha: &image.data,
            });
        }
        for (x, y, bw, bh) in boxes {
            mask.fill_box(x as i32 - left, y as i32, bw as u32, bh as u32);
        }
        mask
    }
}
