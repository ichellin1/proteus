//! Laying out and rasterizing text, on the CPU.
//!
//! [`FontAtlas`] lays a text out with parley, which shapes it, and rasterizes
//! its glyphs with swash into one RGBA image. The caller then allocates a
//! main-atlas region for it with [`crate::TextureRegistry::register_static`]
//! and uploads it with [`crate::QuadPipeline::write_to_main_atlas`].
//!
//! ## Shaping
//!
//! Shaping turns characters into glyphs placed by the font's own rules. It
//! applies the font's kerning, which pulls pairs such as "AV" closer. It also
//! joins ligatures, places accents on their letters, and joins the letters of
//! scripts that need it. Each glyph is placed to a quarter of a pixel, so text
//! is as wide as its font designs it, at every size and display scale.
//!
//! ## The glyph cache
//!
//! Each glyph's coverage is kept after it's rasterized, so a glyph that
//! several texts share is rasterized once. Past a budget in bytes, the cache
//! drops the glyphs used longest ago. The budget is
//! [`DEFAULT_GLYPH_CACHE_BYTES`] unless [`FontAtlas::set_glyph_cache_budget`]
//! changes it. [`FontAtlas::clear_glyph_cache`] drops every glyph. Text already
//! baked keeps its pixels, since they're in the atlas, not the cache.
//!
//! ## Lines
//!
//! A [`TextLayout`] says how to lay a text out. It sets the text's size and
//! letter spacing, the width its lines wrap at, how they align, their height,
//! and how many are shown. A text with more lines than that ends in an
//! ellipsis or is clipped after the last line shown. Lines are broken in
//! logical pixels and scaled as they're rasterized, so a text breaks at the
//! same words at every display scale.
//!
//! An ellipsis is laid out with the text, so it wraps as words do. The text
//! keeps as much of itself as fits, with the ellipsis after it, in its maximum
//! number of lines. It's cut between clusters, so never inside a character. An
//! ellipsis wider than a line takes lines of its own. An ellipsis too long to
//! fit on its own is clipped, with a warning.
//!
//! ## Text as a texture
//!
//! The whole text becomes one image, at its size in pixels:
//! - Red, green and blue are 255 everywhere, so the text can take any color.
//! - Alpha is each pixel's glyph coverage, antialiased.
//!
//! The text is then drawn like any other texture. It transitions, takes a
//! color, and is clipped by rounded corners, with no special handling.
//!
//! ## Fonts
//!
//! A [`FontAtlas`] has a default font, and any others loaded with
//! [`FontAtlas::load_font`], each under the family name inside it. A text names
//! a family, or takes the default, and asks for a weight and italic. A family
//! that lacks the weight uses its nearest, as in CSS. A family with no italic
//! face is slanted, and one with no bold face is thickened.
//!
//! A character a text's family lacks is looked for in the fallback families
//! set with [`FontAtlas::set_fallbacks`], in order, then in the default font. A
//! character none of them has is drawn as the text's family's missing-glyph
//! box, a rectangle, and reported in [`RasterizedGlyphs::missing`]. No system
//! font is ever used.
//!
//! ## The embedded font
//!
//! With the `embedded-font` feature, on by default, [`EMBEDDED_FONT_BYTES`] is
//! Inter (SIL Open Font License 1.1), variable in weight and optical size,
//! from Google Fonts. A text in it that sets no weight is drawn at 700, bold.
//! Without the feature, [`FontAtlas::empty`] has no font until one is loaded.

use std::collections::HashSet;
use std::sync::Arc;

use crate::glyph_cache::{CachedGlyph, GlyphCache, GlyphKey};

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache, SourceCacheOptions};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, FontStyle, FontWeight,
    Layout, LayoutContext, LineHeight, OverflowWrap, PositionedLayoutItem, StyleProperty,
};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Angle, Format, Transform, Vector};

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

/// Inter, variable in weight and optical size, embedded at compile time with
/// the `embedded-font` feature. See the module docs.
///
/// License: SIL Open Font License 1.1 (permissive, bundling allowed). See
/// `assets/LICENSE-Inter.txt`.
#[cfg(feature = "embedded-font")]
pub const EMBEDDED_FONT_BYTES: &[u8] = include_bytes!("../assets/Inter-Variable.ttf");

/// The weight a text in the embedded font is drawn at when it sets none.
const EMBEDDED_DEFAULT_WEIGHT: u16 = 700;

/// The slant of a synthesized italic, in degrees.
const SYNTHESIZED_SLANT: f32 = 14.0;

/// The number of horizontal positions within a pixel a glyph can start at. A
/// glyph is rasterized at the nearest quarter of a pixel to where it's placed.
const SUBPIXEL_STEPS: f32 = 4.0;

/// The glyph cache's budget, 4 MiB, unless
/// [`FontAtlas::set_glyph_cache_budget`] changes it. It holds from about 5,000
/// glyphs at 48 pixels to 25,000 at 16.
pub const DEFAULT_GLYPH_CACHE_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
// TextLayout
// ---------------------------------------------------------------------------

/// Where a text's lines sit across its box. The box is as wide as the wrap
/// width, or as the widest line without one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Against the start of the box. That's the left for left-to-right text.
    #[default]
    Start,
    /// In the middle of the box.
    Center,
    /// Against the end of the box. That's the right for left-to-right text.
    End,
}

/// How a text with more lines than its maximum ends.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TextOverflow {
    /// Ends with this text after the last line shown. The default is "…", and
    /// an app can set its own, such as `" more"`. "…" is drawn as "..." in a
    /// font that has no "…".
    Ellipsis(String),
    /// Leaves out the lines past the last shown, and adds no mark.
    Clip,
}

impl Default for TextOverflow {
    fn default() -> Self {
        Self::Ellipsis(DEFAULT_ELLIPSIS.to_owned())
    }
}

/// The ellipsis, "…", that a text ends in unless it sets its own.
pub const DEFAULT_ELLIPSIS: &str = "\u{2026}";

/// How to lay a text out, in logical pixels, from its size and font to how its
/// lines wrap and end. Make one with [`TextLayout::new`], then change the
/// fields it leaves at their defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayout<'a> {
    /// The text. A `\n` starts a new line.
    pub content: &'a str,
    /// The font's size.
    pub size_px: f32,
    /// The extra space between characters. A negative value tightens them.
    pub letter_spacing_px: f32,
    /// The width lines wrap at. `None` breaks lines only at `\n`.
    pub wrap_width: Option<f32>,
    /// Where lines sit across the text's box.
    pub align: TextAlign,
    /// The distance from one line's baseline to the next, as a multiple of
    /// the size, like CSS's unitless `line-height`. `None` uses the font's own
    /// line spacing.
    pub line_height: Option<f32>,
    /// The most lines shown. `None` shows them all.
    pub max_lines: Option<u32>,
    /// The ellipsis that ends a text with more lines than `max_lines`. `None`
    /// clips the text instead.
    pub ellipsis: Option<&'a str>,
    /// The font family. `None`, or a family that isn't loaded, uses the
    /// default font.
    pub family: Option<&'a str>,
    /// The weight, from 100 to 900, where 400 is regular and 700 is bold.
    /// `None` uses the font's own, or 700 in the embedded font.
    pub weight: Option<u16>,
    /// Whether the text is italic.
    pub italic: bool,
}

impl<'a> TextLayout<'a> {
    /// Creates a [`TextLayout`] for `content` at `size_px`. Its lines break
    /// only at `\n`, align to the start, and use the font's line spacing, with
    /// no maximum number of lines.
    pub fn new(content: &'a str, size_px: f32) -> Self {
        Self {
            content,
            size_px,
            letter_spacing_px: 0.0,
            wrap_width: None,
            align: TextAlign::Start,
            line_height: None,
            max_lines: None,
            ellipsis: Some(DEFAULT_ELLIPSIS),
            family: None,
            weight: None,
            italic: false,
        }
    }
}

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
    /// The characters no font had, each once, in the order they first appear.
    /// Each is drawn as the font's missing-glyph box.
    pub missing: Vec<char>,
}

// ---------------------------------------------------------------------------
// FontAtlas
// ---------------------------------------------------------------------------

/// A collection of fonts that lays out and rasterizes text on the CPU. It
/// doesn't allocate atlas space (see the module docs).
///
/// One is enough for an app. The renderer owns one and uses it for all text.
/// Call [`rasterize_text`] for each string and size, allocate a main-atlas
/// region for the result with `TextureRegistry::register_static`, then upload
/// it with `QuadPipeline::write_to_main_atlas`.
///
/// [`rasterize_text`]: FontAtlas::rasterize_text
pub struct FontAtlas {
    fonts: FontContext,
    layout: LayoutContext<()>,
    scale: ScaleContext,
    /// The default font's family name, or `None` with no font at all.
    default_family: Option<String>,
    /// The embedded font's family name, when it's the default.
    embedded_family: Option<String>,
    /// Every family loaded.
    families: HashSet<String>,
    /// Whether the default font has "…", for ending a text with it.
    has_ellipsis: bool,
    /// Whether the warning that there's no font to draw in was logged.
    warned_no_font: bool,
    /// The fallback families, in the order they're tried.
    fallbacks: Vec<String>,
    /// The characters no font had, for which a warning has been logged.
    warned_missing: HashSet<char>,
    glyphs: GlyphCache,
}

/// Font bytes that [`FontAtlas::new`] couldn't read as a TTF or OTF font.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not a TTF or OTF font that can be read: {0}")]
pub struct FontError(String);

/// Returns the family name inside a TTF or OTF font, the one
/// [`FontAtlas::load_font`] files it under, without loading it into an atlas.
///
/// # Errors
///
/// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
pub fn font_family_name(font_bytes: &[u8]) -> Result<String, FontError> {
    FontAtlas::empty().load_font(font_bytes)
}

/// Returns the characters in the first `line_count` lines of `layout`, which
/// was laid out from `content`, that no font had. Each appears once, in order.
fn missing_chars(layout: &Layout<()>, line_count: usize, content: &str) -> Vec<char> {
    let mut missing = Vec::new();
    for line in layout.lines().take(line_count) {
        for run in line.runs() {
            for cluster in run.clusters() {
                if cluster.is_hard_line_break() || !cluster.glyphs().any(|g| g.id == 0) {
                    continue;
                }
                for c in content[cluster.text_range()].chars() {
                    if !c.is_whitespace() && !missing.contains(&c) {
                        missing.push(c);
                    }
                }
            }
        }
    }
    missing
}

/// Returns whether the font has "…", for ending a text with it.
fn has_ellipsis(font_bytes: &[u8]) -> bool {
    swash::FontRef::from_index(font_bytes, 0)
        .is_some_and(|font| font.charmap().map('\u{2026}') != 0)
}

/// A text laid out within its maximum number of lines.
struct Fitted {
    layout: Layout<()>,
    /// How many of the layout's lines are drawn. That's fewer than the layout
    /// has when the text is clipped.
    line_count: usize,
    /// What was laid out in place of the text when it ends in an ellipsis.
    content: Option<String>,
}

/// A glyph rasterized for one text, before it's drawn into the text's image.
struct PlacedGlyph {
    /// The left and top of its pixels, in whole pixels, measured from the
    /// text's pen start and its first line's baseline.
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Arc<[u8]>,
}

impl FontAtlas {
    /// Creates a [`FontAtlas`] for the TTF or OTF font in `font_bytes`.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
    pub fn new(font_bytes: &[u8]) -> Result<Self, FontError> {
        let mut atlas = Self::empty();
        let family = atlas.load_font(font_bytes)?;
        atlas.has_ellipsis = has_ellipsis(font_bytes);
        atlas.default_family = Some(family);
        Ok(atlas)
    }

    /// Creates a [`FontAtlas`] for the embedded font, [`EMBEDDED_FONT_BYTES`]
    /// (Inter). A text in it that sets no weight is drawn at 700.
    #[cfg(feature = "embedded-font")]
    pub fn with_embedded_font() -> Self {
        let mut atlas = Self::empty();
        let family = atlas
            .register(Blob::new(Arc::new(EMBEDDED_FONT_BYTES)))
            .expect("the embedded font is a valid TTF");
        atlas.has_ellipsis = has_ellipsis(EMBEDDED_FONT_BYTES);
        atlas.embedded_family = Some(family.clone());
        atlas.default_family = Some(family);
        atlas
    }

    /// Creates a [`FontAtlas`] with no font, which draws no text until one is
    /// loaded with [`FontAtlas::load_font`]. The first font loaded becomes the
    /// default.
    pub fn empty() -> Self {
        Self {
            fonts: FontContext {
                // Only the fonts given here are used, never the system's.
                collection: Collection::new(CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                source_cache: SourceCache::new(SourceCacheOptions::default()),
            },
            layout: LayoutContext::new(),
            scale: ScaleContext::new(),
            default_family: None,
            embedded_family: None,
            families: HashSet::new(),
            has_ellipsis: false,
            warned_no_font: false,
            fallbacks: Vec::new(),
            warned_missing: HashSet::new(),
            glyphs: GlyphCache::new(DEFAULT_GLYPH_CACHE_BYTES),
        }
    }

    /// Adds the TTF or OTF font in `font_bytes`, and returns the family name
    /// inside it. Texts use the font by naming that family. A font whose family
    /// is already loaded adds a face to it, such as an italic. The first font
    /// loaded into an [`empty`](Self::empty) atlas becomes its default.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
    pub fn load_font(&mut self, font_bytes: &[u8]) -> Result<String, FontError> {
        let family = self.register(Blob::new(Arc::new(font_bytes.to_vec())))?;
        if self.default_family.is_none() {
            self.has_ellipsis = has_ellipsis(font_bytes);
            self.default_family = Some(family.clone());
        }
        Ok(family)
    }

    /// Sets the fallback families. For a character a text's own family lacks,
    /// they're tried in order, before the default font. A family that isn't
    /// loaded yet is skipped until it is. See the module docs.
    pub fn set_fallbacks(&mut self, families: &[String]) {
        self.fallbacks = families.to_vec();
    }

    /// Returns whether a font of `family` has been loaded.
    pub fn has_family(&self, family: &str) -> bool {
        self.families.contains(family)
    }

    /// Returns the default font's family name, or `None` with no font at all.
    pub fn default_family(&self) -> Option<&str> {
        self.default_family.as_deref()
    }

    fn register(&mut self, blob: Blob<u8>) -> Result<String, FontError> {
        let families = self.fonts.collection.register_fonts(blob, None);
        let family = families
            .first()
            .and_then(|(id, _)| self.fonts.collection.family_name(*id))
            .ok_or_else(|| FontError("no font family found in the bytes".into()))?
            .to_owned();
        self.families.insert(family.clone());
        Ok(family)
    }

    /// Sets the most bytes of glyphs the glyph cache keeps, and drops the
    /// glyphs used longest ago if it holds more. A budget of 0 keeps none, so
    /// every glyph is rasterized each time it's baked. See the module docs.
    pub fn set_glyph_cache_budget(&mut self, bytes: usize) {
        self.glyphs.set_budget(bytes);
    }

    /// Drops every glyph from the glyph cache, to free its memory. Texts
    /// baked afterward rasterize their glyphs again.
    pub fn clear_glyph_cache(&mut self) {
        self.glyphs.clear();
    }

    /// Returns the glyph cache's size as the number of glyphs it holds and
    /// their bytes, counted as the budget counts them.
    pub fn glyph_cache_size(&self) -> (usize, usize) {
        (self.glyphs.len(), self.glyphs.bytes())
    }

    /// Lays `text` out at `size_px`, with `letter_spacing_px` of extra space
    /// between glyphs (`0.0` for the font's normal spacing), and rasterizes it
    /// into an RGBA pixel buffer. A `\n` starts a new line. The image spans
    /// the glyphs' ink, so it has no padding after the last glyph. This is the
    /// same as [`FontAtlas::rasterize`] with a [`TextLayout::new`] and a scale
    /// of 1.
    ///
    /// Returns `None` if `text` draws no pixels, because it's empty or only
    /// whitespace.
    pub fn rasterize_text(
        &mut self,
        text: &str,
        size_px: f32,
        letter_spacing_px: f32,
    ) -> Option<RasterizedGlyphs> {
        let mut layout = TextLayout::new(text, size_px);
        layout.letter_spacing_px = letter_spacing_px;
        self.rasterize(&layout, 1.0)
    }

    /// Lays `text` out and rasterizes it into an RGBA pixel buffer, at `scale`
    /// pixels per logical pixel, the way a bake at the display's scale does.
    /// Lines break where they would at a scale of 1. See the module docs.
    ///
    /// The image is the text's box. It's as wide as the wrap width, or as the
    /// widest line's ink without one, and as tall as from the top of the first
    /// line to the bottom of the last. Ink that reaches past the box widens the
    /// image.
    ///
    /// Returns `None` if `text` draws no pixels, because it's empty or only
    /// whitespace.
    pub fn rasterize(&mut self, text: &TextLayout, scale: f32) -> Option<RasterizedGlyphs> {
        if text.content.is_empty() {
            return None;
        }
        if self.default_family.is_none() {
            if !self.warned_no_font {
                self.warned_no_font = true;
                log::warn!(
                    "there's no font to draw text in, so none is drawn: give the app a default \
                     font in `text.default_font`, or load one with `load_font`"
                );
            }
            return None;
        }
        let Fitted {
            layout,
            line_count,
            content,
        } = self.fit(text, scale);

        // The image's height runs from the top of the first line to the bottom
        // of the last shown. Each part is rounded up on its own, so the
        // baseline sits on a whole pixel and descenders are never cut off.
        let first = *layout.lines().next()?.metrics();
        let last = *layout.lines().nth(line_count - 1)?.metrics();
        let ascent_px = (first.baseline - first.block_min_coord).ceil() as i32;
        let below_px = (last.block_max_coord - first.baseline).ceil() as i32;
        let height = ascent_px + below_px;
        if height <= 0 {
            return None;
        }

        let glyphs = self.rasterize_glyphs(&layout, first.baseline, line_count);

        // The image's width is the wrap width, if there is one, widened to
        // take in ink that reaches past it. Without a wrap width, it runs from
        // the leftmost ink, or the pen's start if that's further left, to the
        // rightmost ink. A glyph's ink can reach left of the pen, as Inter's
        // "j" does, or past its own advance.
        let box_width = text.wrap_width.map_or(0, |w| (w * scale).ceil() as i32);
        let min_left = glyphs.iter().map(|g| g.left).min()?.min(0);
        let max_right = glyphs
            .iter()
            .map(|g| g.left + g.width as i32)
            .max()?
            .max(box_width);
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

        let missing = missing_chars(
            &layout,
            line_count,
            content.as_deref().unwrap_or(text.content),
        );
        for &c in &missing {
            if self.warned_missing.insert(c) {
                log::warn!(
                    "no font has {c:?} (U+{:04X}), in the text {:?}, so it's drawn as a box: add a \
                     font that has it to the fallback fonts",
                    c as u32,
                    text.content,
                );
            }
        }

        Some(RasterizedGlyphs {
            width,
            height: height as u32,
            rgba_pixels: rgba,
            missing,
        })
    }

    /// Lays `text` out within its maximum number of lines. Past the maximum,
    /// an ellipsis replaces the end of what fits, or the lines past it are
    /// left out.
    fn fit(&mut self, text: &TextLayout, scale: f32) -> Fitted {
        let full = self.lay_out(text.content, text, scale);
        let line_count = full.len();
        let fitted = |layout, line_count, content| Fitted {
            layout,
            line_count,
            content,
        };
        let Some(max) = text.max_lines.map(|n| n.max(1) as usize) else {
            return fitted(full, line_count, None);
        };
        if line_count <= max {
            return fitted(full, line_count, None);
        }
        let Some(ellipsis) = text.ellipsis else {
            return fitted(full, max, None);
        };
        let ellipsis = if ellipsis == DEFAULT_ELLIPSIS && !self.has_ellipsis {
            "..."
        } else {
            ellipsis
        };

        // The places the text can be cut, up to the end of the last line
        // shown. They're between clusters, so never inside a character.
        let end = full.lines().nth(max - 1).map_or(0, |l| l.text_range().end);
        let mut cuts: Vec<usize> = full
            .lines()
            .take(max)
            .flat_map(|line| line.runs().flat_map(|run| run.clusters()))
            .map(|cluster| cluster.text_range().start)
            .filter(|&at| at < end)
            .chain([0, end])
            .collect();
        cuts.sort_unstable();
        cuts.dedup();

        let with_ellipsis = |cut: usize| {
            let head = text.content[..cut].trim_end();
            if head.is_empty() {
                ellipsis.trim_start().to_owned()
            } else {
                format!("{head}{ellipsis}")
            }
        };
        // Lays out the text cut at `cut`, with the ellipsis, and returns it if
        // it fits in `max` lines. A binary search finds the longest cut that
        // fits, since a shorter cut never takes more lines.
        let fits = |atlas: &mut Self, cut: usize| {
            let content = with_ellipsis(cut);
            let layout = atlas.lay_out(&content, text, scale);
            (layout.len() <= max).then_some((layout, content))
        };
        let Some(mut best) = fits(self, cuts[0]) else {
            log::warn!(
                "the ellipsis {ellipsis:?} of the text {:?} needs more than its {max} line(s) \
                 on its own, at a wrap width of {:?}, so it's clipped: raise its maximum number \
                 of lines, widen its wrap width, or shorten its ellipsis",
                text.content,
                text.wrap_width,
            );
            let content = ellipsis.trim_start().to_owned();
            let layout = self.lay_out(&content, text, scale);
            return fitted(layout, max, Some(content));
        };
        let (mut low, mut high) = (0, cuts.len() - 1);
        while low < high {
            let mid = (low + high).div_ceil(2);
            match fits(self, cuts[mid]) {
                Some(found) => {
                    best = found;
                    low = mid;
                }
                None => high = mid - 1,
            }
        }
        let (layout, content) = best;
        let line_count = layout.len();
        fitted(layout, line_count, Some(content))
    }

    /// Shapes `content` in `text`'s style, breaks it into lines, and aligns
    /// them, at `scale`.
    fn lay_out(&mut self, content: &str, text: &TextLayout, scale: f32) -> Layout<()> {
        // Uses the text's family if it's loaded, and the default otherwise. A
        // text in the embedded font that sets no weight is bold.
        let family = text
            .family
            .filter(|f| self.families.contains(*f))
            .or(self.default_family.as_deref())
            .unwrap_or_default()
            .to_owned();
        let weight = text.weight.or_else(|| {
            (self.embedded_family.as_ref() == Some(&family)).then_some(EMBEDDED_DEFAULT_WEIGHT)
        });
        // Sizes are in logical pixels, and parley scales them. Positions are
        // fractional, and rounded when the glyphs are rasterized.
        let mut builder = self
            .layout
            .ranged_builder(&mut self.fonts, content, scale, false);
        // The stack is the text's family, then the fallbacks in order, then
        // the default font. Each is tried in turn for a character the ones
        // before it lack.
        let mut stack = vec![family.clone()];
        let loaded = self.fallbacks.iter().filter(|f| self.families.contains(*f));
        for name in loaded.chain(&self.default_family) {
            if !stack.contains(name) {
                stack.push(name.clone());
            }
        }
        let names: Vec<FontFamilyName<'_>> = stack
            .into_iter()
            .map(|name| FontFamilyName::Named(name.into()))
            .collect();
        builder.push_default(StyleProperty::FontFamily(FontFamily::List(names.into())));
        if let Some(weight) = weight {
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight as f32)));
        }
        if text.italic {
            builder.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
        builder.push_default(StyleProperty::FontSize(text.size_px));
        builder.push_default(StyleProperty::LetterSpacing(text.letter_spacing_px));
        builder.push_default(StyleProperty::LineHeight(match text.line_height {
            Some(multiple) => LineHeight::FontSizeRelative(multiple),
            None => LineHeight::MetricsRelative(1.0),
        }));
        // A word wider than the wrap width breaks between characters, rather
        // than running out of the box.
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        let mut layout = builder.build(content);
        // Set before breaking lines, because breaking lines aligns them.
        let alignment = match text.align {
            TextAlign::Start => Alignment::Start,
            TextAlign::Center => Alignment::Center,
            TextAlign::End => Alignment::End,
        };
        layout.set_alignment(alignment, AlignmentOptions::default());
        layout.break_all_lines(text.wrap_width.map(|w| w * scale));
        layout
    }

    /// Rasterizes every glyph in the first `line_count` lines of `layout`.
    /// Each is placed relative to the pen's start and the first line's
    /// baseline, which is at `baseline`.
    fn rasterize_glyphs(
        &mut self,
        layout: &Layout<()>,
        baseline: f32,
        line_count: usize,
    ) -> Vec<PlacedGlyph> {
        let mut glyphs = Vec::new();
        for line in layout.lines().take(line_count) {
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
                let coords: Box<[i16]> = run
                    .normalized_coords()
                    .iter()
                    .map(|c| c.to_bits())
                    .collect();
                let mut scaler = self
                    .scale
                    .builder(font_ref)
                    .size(run.font_size())
                    .hint(false)
                    .normalized_coords(coords.iter())
                    .build();
                // A face standing in for one the family lacks is slanted for
                // italic and thickened for bold. The thickening is about a
                // thirtieth of the font size.
                let synthesis = run.synthesis();
                let slant = synthesis.skew().map(|_| SYNTHESIZED_SLANT);
                let embolden = synthesis.embolden().then(|| run.font_size() / 30.0);
                for g in glyph_run.positioned_glyphs() {
                    // Horizontally, a glyph is placed to the nearest quarter
                    // pixel. Vertically, it's placed to the nearest pixel, so
                    // lines sit on whole pixels.
                    let x = (g.x * SUBPIXEL_STEPS).round() / SUBPIXEL_STEPS;
                    let pen_x = x.floor();
                    let pen_y = (g.y - baseline).round();
                    let key = GlyphKey {
                        font: font.data.id(),
                        index: font.index,
                        glyph: g.id,
                        size_bits: run.font_size().to_bits(),
                        coords: coords.clone(),
                        quarter: ((x - pen_x) * SUBPIXEL_STEPS) as u8,
                        slanted: slant.is_some(),
                        emboldened: embolden.is_some(),
                    };
                    let glyph = self.glyphs.get_or_rasterize(key, || {
                        let image = Render::new(&[Source::Outline])
                            .format(Format::Alpha)
                            .offset(Vector::new(x - pen_x, 0.0))
                            .transform(slant.map(|degrees| {
                                Transform::skew(Angle::from_degrees(degrees), Angle::ZERO)
                            }))
                            .embolden(embolden.unwrap_or(0.0))
                            .render(&mut scaler, g.id as u16)?;
                        let p = image.placement;
                        (p.width > 0 && p.height > 0).then(|| CachedGlyph {
                            left: p.left,
                            top: -p.top,
                            width: p.width,
                            height: p.height,
                            coverage: image.data.into(),
                        })
                    });
                    let Some(glyph) = glyph else {
                        continue;
                    };
                    glyphs.push(PlacedGlyph {
                        left: pen_x as i32 + glyph.left,
                        top: pen_y as i32 + glyph.top,
                        width: glyph.width,
                        height: glyph.height,
                        coverage: glyph.coverage,
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

    // Returns the range of columns that have any coverage, which is where the
    // ink is.
    fn ink_columns(g: &RasterizedGlyphs) -> std::ops::Range<u32> {
        let has_ink =
            |x: u32| (0..g.height).any(|y| g.rgba_pixels[((y * g.width + x) * 4 + 3) as usize] > 0);
        let first = (0..g.width).find(|&x| has_ink(x)).unwrap();
        let last = (0..g.width).rev().find(|&x| has_ink(x)).unwrap();
        first..last + 1
    }

    // A glyph whose ink starts left of the pen, like Inter's "j", must keep
    // its left edge. The image starts at the ink, not at the pen, or the glyph
    // is clipped.
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

    // The font's kerning must pull "A" and "V" together. Laid out, "AV"
    // advances less than the font's advances for "A" and "V" added up.
    // Spacing each glyph by its own advance, with no shaping, misses the
    // kerning.
    #[test]
    fn kerning_pulls_a_pair_together() {
        let mut fa = atlas();
        let shaped = fa
            .lay_out("AV", &TextLayout::new("AV", 64.0), 1.0)
            .full_width();

        // Uses the weight the text is drawn at.
        let font = swash::FontRef::from_index(EMBEDDED_FONT_BYTES, 0).unwrap();
        let coords: Vec<i16> = font
            .variations()
            .normalized_coords([(swash::tag_from_bytes(b"wght"), 700.0)])
            .collect();
        let metrics = font.glyph_metrics(&coords).scale(64.0);
        let advance = |c| metrics.advance_width(font.charmap().map(c));
        let unshaped = advance('A') + advance('V');

        assert!(
            shaped < unshaped - 2.0,
            "AV advances {shaped}; A and V add up to {unshaped}"
        );
    }

    // A line must be the same width in logical pixels at every display scale,
    // or text wraps differently on different displays. Rounding each glyph's
    // advance up to a whole pixel breaks this, making a line wider at a scale
    // of 1 than at 2.
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

    // Baking a text again must rasterize nothing new, since its glyphs are
    // cached.
    #[test]
    fn baking_a_text_again_adds_nothing_to_the_glyph_cache() {
        let mut fa = atlas();
        fa.rasterize_text("Settings", 16.0, 0.0).unwrap();
        let after_first = fa.glyph_cache_size();
        assert!(after_first.0 > 0);
        fa.rasterize_text("Settings", 16.0, 0.0).unwrap();
        assert_eq!(fa.glyph_cache_size(), after_first);
    }

    // The cache must change only how often glyphs are rasterized, never the
    // pixels. A text is the same with the cache, without it, and after it's
    // cleared.
    #[test]
    fn cached_glyphs_draw_the_same_pixels() {
        let text = "The quick brown fox, office -> waffle";
        let mut uncached = atlas();
        uncached.set_glyph_cache_budget(0);
        let expected = uncached.rasterize_text(text, 21.0, 0.5).unwrap();
        assert_eq!(uncached.glyph_cache_size(), (0, 0));

        let mut fa = atlas();
        for _ in 0..2 {
            let cached = fa.rasterize_text(text, 21.0, 0.5).unwrap();
            assert_eq!(cached.rgba_pixels, expected.rgba_pixels);
        }
        fa.clear_glyph_cache();
        assert_eq!(fa.glyph_cache_size(), (0, 0));
        let again = fa.rasterize_text(text, 21.0, 0.5).unwrap();
        assert_eq!(again.rgba_pixels, expected.rgba_pixels);
    }

    #[test]
    fn the_glyph_cache_stays_within_its_budget() {
        let mut fa = atlas();
        fa.set_glyph_cache_budget(20_000);
        for size in 10..60 {
            fa.rasterize_text("ABCDEFGHIJKLMNOPQRSTUVWXYZ", size as f32, 0.0);
            assert!(fa.glyph_cache_size().1 <= 20_000);
        }
    }

    const PARAGRAPH: &str = "Proteus draws every component as a textured, rounded rectangle, \
        in one call. A split turns one component into several, and a merge turns several into one.";

    fn wrapped(width: f32) -> TextLayout<'static> {
        let mut text = TextLayout::new(PARAGRAPH, 16.0);
        text.wrap_width = Some(width);
        text
    }

    // Returns how many columns from the left have no ink.
    fn left_margin(g: &RasterizedGlyphs) -> u32 {
        ink_columns(g).start
    }

    #[test]
    fn text_wraps_at_its_width_and_its_box_is_that_wide() {
        let mut fa = atlas();
        let fitted = fa.fit(&wrapped(200.0), 1.0);
        assert!(fitted.line_count >= 5, "{} lines", fitted.line_count);
        // Trailing spaces hang past the width, as in CSS, so they don't count.
        for line in fitted.layout.lines() {
            let m = line.metrics();
            assert!(m.advance - m.hanging_advance <= 200.5, "{}", m.advance);
        }
        let g = fa.rasterize(&wrapped(200.0), 1.0).unwrap();
        assert_eq!(g.width, 200);
    }

    // Lines must break in logical pixels, or different words start each line
    // at different display scales.
    #[test]
    fn text_breaks_at_the_same_words_at_every_scale() {
        let mut fa = atlas();
        let starts = |fa: &mut FontAtlas, scale| -> Vec<usize> {
            let fitted = fa.fit(&wrapped(230.0), scale);
            fitted
                .layout
                .lines()
                .map(|l| l.text_range().start)
                .collect()
        };
        let at_1 = starts(&mut fa, 1.0);
        assert_eq!(at_1, starts(&mut fa, 2.0));
        assert_eq!(at_1, starts(&mut fa, 1.5));
    }

    #[test]
    fn a_word_wider_than_the_wrap_width_breaks_between_characters() {
        let mut fa = atlas();
        let mut text = TextLayout::new("Supercalifragilisticexpialidocious", 24.0);
        text.wrap_width = Some(120.0);
        let fitted = fa.fit(&text, 1.0);
        assert!(fitted.line_count > 1);
        for line in fitted.layout.lines() {
            let m = line.metrics();
            assert!(m.advance - m.hanging_advance <= 120.5, "{}", m.advance);
        }
    }

    // Centered and end-aligned lines start further from the box's left edge
    // than lines aligned to the start.
    #[test]
    fn lines_align_within_the_box() {
        let mut fa = atlas();
        let mut text = TextLayout::new("Short line", 16.0);
        text.wrap_width = Some(300.0);
        let start = fa.rasterize(&text, 1.0).unwrap();
        text.align = TextAlign::Center;
        let center = fa.rasterize(&text, 1.0).unwrap();
        text.align = TextAlign::End;
        let end = fa.rasterize(&text, 1.0).unwrap();
        assert!(left_margin(&start) <= 2);
        let ink = ink_columns(&start).len() as u32;
        let centered = (300 - ink) / 2;
        assert!(
            left_margin(&center).abs_diff(centered) <= 2,
            "{}",
            left_margin(&center)
        );
        assert!(
            left_margin(&end).abs_diff(300 - ink) <= 2,
            "{}",
            left_margin(&end)
        );
        assert_eq!(start.width, 300);
    }

    #[test]
    fn line_height_spaces_the_lines() {
        let mut fa = atlas();
        let mut text = TextLayout::new("One\nTwo\nThree", 20.0);
        let natural = fa.rasterize(&text, 1.0).unwrap().height as f32;
        text.line_height = Some(2.0);
        let double = fa.rasterize(&text, 1.0).unwrap().height as f32;
        assert!(
            (double - 3.0 * 40.0).abs() <= 2.0,
            "{double} for three lines at 40"
        );
        assert!(double > natural);
    }

    #[test]
    fn clipped_text_shows_its_maximum_number_of_lines() {
        let mut fa = atlas();
        let all = fa.rasterize(&wrapped(200.0), 1.0).unwrap();
        let mut text = wrapped(200.0);
        text.max_lines = Some(2);
        text.ellipsis = None;
        let fitted = fa.fit(&text, 1.0);
        assert_eq!(fitted.line_count, 2);
        assert_eq!(fitted.content, None);
        let two = fa.rasterize(&text, 1.0).unwrap();
        let one_line = all.height as f32 / fa.fit(&wrapped(200.0), 1.0).line_count as f32;
        assert!((two.height as f32 - 2.0 * one_line).abs() <= 2.0);
    }

    // The ellipsis ends the last line shown, after as much of the text as fits
    // beside it.
    #[test]
    fn an_ellipsis_ends_the_last_line_shown() {
        let mut fa = atlas();
        let mut text = wrapped(200.0);
        text.max_lines = Some(2);
        let fitted = fa.fit(&text, 1.0);
        assert_eq!(fitted.line_count, 2);
        let content = fitted.content.unwrap();
        assert!(content.ends_with('\u{2026}'), "{content:?}");
        let head = content.trim_end_matches('\u{2026}');
        assert!(PARAGRAPH.starts_with(head));
        assert!(!head.ends_with(' '), "no space before the ellipsis");
        // One more word wouldn't have fitted.
        let next = PARAGRAPH[head.len()..].trim_start();
        let one_more = next.split(' ').next().unwrap();
        let longer = format!("{head} {one_more}\u{2026}");
        assert!(fa.lay_out(&longer, &text, 1.0).len() > 2);
    }

    // The app's own ellipsis is used. It wraps onto a line of its own when it
    // doesn't fit beside the text.
    #[test]
    fn an_ellipsis_wider_than_a_line_wraps_onto_its_own() {
        let mut fa = atlas();
        let mut text = TextLayout::new(PARAGRAPH, 16.0);
        text.wrap_width = Some(90.0);
        text.max_lines = Some(3);
        text.ellipsis = Some(" read the whole article");
        let fitted = fa.fit(&text, 1.0);
        let content = fitted.content.unwrap();
        assert!(content.ends_with(" read the whole article"), "{content:?}");
        assert!(
            content.len() > " read the whole article".len(),
            "some text is kept"
        );
        assert!(fitted.line_count <= 3);
    }

    // An ellipsis that can't fit in the lines allowed on its own is clipped.
    #[test]
    fn an_ellipsis_too_long_on_its_own_is_clipped() {
        let mut fa = atlas();
        let mut text = TextLayout::new(PARAGRAPH, 16.0);
        text.wrap_width = Some(60.0);
        text.max_lines = Some(1);
        text.ellipsis = Some(" read the whole article");
        let fitted = fa.fit(&text, 1.0);
        assert_eq!(fitted.content.as_deref(), Some("read the whole article"));
        assert_eq!(fitted.line_count, 1);
        assert!(fa.rasterize(&text, 1.0).is_some());
    }

    // The text is cut between clusters, so "e" and a combining accent stay
    // together.
    #[test]
    fn an_ellipsis_never_splits_a_character() {
        let mut fa = atlas();
        let accented = "cafe\u{301} ".repeat(30);
        let mut text = TextLayout::new(&accented, 16.0);
        text.wrap_width = Some(100.0);
        text.max_lines = Some(2);
        for width in [97.0, 100.0, 103.0, 106.0, 109.0] {
            text.wrap_width = Some(width);
            let content = fa.fit(&text, 1.0).content.unwrap();
            let head = content.trim_end_matches('\u{2026}');
            assert!(
                head.ends_with('\u{301}') || !head.ends_with('e'),
                "{head:?} at {width}"
            );
        }
    }

    #[test]
    fn a_font_without_an_ellipsis_ends_in_three_dots() {
        let mut fa = atlas();
        fa.has_ellipsis = false;
        let mut text = wrapped(200.0);
        text.max_lines = Some(1);
        let content = fa.fit(&text, 1.0).content.unwrap();
        assert!(content.ends_with("..."), "{content:?}");
        text.ellipsis = Some(" more");
        let content = fa.fit(&text, 1.0).content.unwrap();
        assert!(content.ends_with(" more"), "the app's own ellipsis is kept");
    }

    #[test]
    fn text_within_its_maximum_isnt_changed() {
        let mut fa = atlas();
        let mut text = TextLayout::new("Two\nlines", 16.0);
        text.max_lines = Some(2);
        assert_eq!(fa.fit(&text, 1.0).content, None);
    }

    // Lora, roman and italic, is a second family. It's variable from 400 to
    // 700.
    use proteus_test_fonts::{SECOND_FAMILY as LORA, SECOND_FAMILY_ITALIC as LORA_ITALIC};

    fn pixels(fa: &mut FontAtlas, text: &TextLayout) -> RasterizedGlyphs {
        fa.rasterize(text, 1.0).unwrap()
    }

    #[test]
    fn a_loaded_font_is_named_by_its_family() {
        let mut fa = atlas();
        assert_eq!(fa.load_font(LORA).unwrap(), "Lora");
        assert!(fa.has_family("Lora"));
        assert_eq!(fa.default_family(), Some("Inter"), "the default stays");
        assert_eq!(font_family_name(LORA_ITALIC).unwrap(), "Lora");
        assert!(font_family_name(b"not a font").is_err());
    }

    #[test]
    fn a_text_in_a_loaded_family_is_drawn_in_it() {
        let mut fa = atlas();
        fa.load_font(LORA).unwrap();
        let mut text = TextLayout::new("Hamburgefonstiv", 32.0);
        let inter = pixels(&mut fa, &text);
        text.family = Some("Lora");
        let lora = pixels(&mut fa, &text);
        assert_ne!(inter.rgba_pixels, lora.rgba_pixels);
        // A family that isn't loaded is the default.
        text.family = Some("Garamond");
        assert_eq!(pixels(&mut fa, &text).rgba_pixels, inter.rgba_pixels);
    }

    // A text that sets no weight is bold in the embedded font, and the weights
    // of one variable font differ from each other.
    #[test]
    fn weights_come_from_the_variable_font() {
        let mut fa = atlas();
        let mut text = TextLayout::new("Hamburgefonstiv", 32.0);
        let unset = pixels(&mut fa, &text);
        text.weight = Some(700);
        assert_eq!(pixels(&mut fa, &text).rgba_pixels, unset.rgba_pixels);
        text.weight = Some(100);
        let thin = pixels(&mut fa, &text);
        text.weight = Some(900);
        let black = pixels(&mut fa, &text);
        assert!(
            thin.width < black.width,
            "{} thin, {} black",
            thin.width,
            black.width
        );
    }

    // A weight a family doesn't have uses its nearest. Lora has no 900, so it
    // uses 700.
    #[test]
    fn a_missing_weight_uses_the_nearest() {
        let mut fa = atlas();
        fa.load_font(LORA).unwrap();
        let mut text = TextLayout::new("Hamburgefonstiv", 32.0);
        text.family = Some("Lora");
        text.weight = Some(700);
        let bold = pixels(&mut fa, &text);
        text.weight = Some(900);
        let black = pixels(&mut fa, &text);
        assert_eq!(bold.width, black.width);
    }

    // Italic uses a family's italic face. When the family has none, as Inter
    // here, it slants the upright face.
    #[test]
    fn italic_uses_the_italic_face_or_slants() {
        let mut fa = atlas();
        fa.load_font(LORA).unwrap();
        fa.load_font(LORA_ITALIC).unwrap();
        let mut text = TextLayout::new("Hamburgefonstiv", 32.0);
        text.family = Some("Lora");
        let upright = pixels(&mut fa, &text);
        text.italic = true;
        let italic = pixels(&mut fa, &text);
        assert_ne!(upright.width, italic.width, "Lora Italic is its own design");

        let mut inter = TextLayout::new("IIII", 32.0);
        let straight = pixels(&mut fa, &inter);
        inter.italic = true;
        let slanted = pixels(&mut fa, &inter);
        assert!(
            slanted.width > straight.width,
            "a slant widens straight strokes"
        );
    }

    // Crimson Pro is a third family. Noto Serif Devanagari is for a script
    // Inter lacks.
    use proteus_test_fonts::{DEVANAGARI, THIRD_FAMILY as CRIMSON};

    // A script no loaded font has is drawn as boxes and reported. With a
    // fallback font that has it, the script is drawn in that font.
    #[test]
    fn a_fallback_font_draws_a_script_the_font_lacks() {
        let mut fa = atlas();
        let text = TextLayout::new("Hello नमस्ते", 32.0);
        let boxes = pixels(&mut fa, &text);
        assert_eq!(boxes.missing, vec!['न', 'म', 'स', '्', 'त', 'े']);

        // A fallback named before it's loaded is skipped until it is.
        fa.set_fallbacks(&["Noto Serif Devanagari".to_owned()]);
        assert!(!pixels(&mut fa, &text).missing.is_empty());

        assert_eq!(fa.load_font(DEVANAGARI).unwrap(), "Noto Serif Devanagari");
        let drawn = pixels(&mut fa, &text);
        assert!(drawn.missing.is_empty());
        assert_ne!(drawn.rgba_pixels, boxes.rgba_pixels);
    }

    // The fallbacks are tried in order, before the default font. Lora lacks
    // "ĸ" and these letters, and Crimson Pro and Inter both have them. With
    // Crimson Pro as the fallback, it draws them.
    #[test]
    fn the_fallbacks_come_before_the_default_font() {
        let mut fa = atlas();
        fa.load_font(LORA).unwrap();
        fa.load_font(CRIMSON).unwrap();
        let mut text = TextLayout::new("ĸǅǆǈǉǋǌ", 48.0);
        text.weight = Some(400);
        // The ink's width tells which font drew the glyphs. The line's height
        // is the text's own family's either way.
        let mut in_family = |fa: &mut FontAtlas, family: &'static str| {
            text.family = Some(family);
            ink_columns(&pixels(fa, &text)).len()
        };
        let crimson = in_family(&mut fa, "Crimson Pro");
        let inter = in_family(&mut fa, "Inter");
        assert_ne!(crimson, inter);

        assert_eq!(
            in_family(&mut fa, "Lora"),
            inter,
            "no fallbacks: the default font"
        );
        fa.set_fallbacks(&["Crimson Pro".to_owned()]);
        assert_eq!(in_family(&mut fa, "Lora"), crimson, "the fallback first");
    }

    // A character no font has is drawn as a box, not left out.
    #[test]
    fn a_character_no_font_has_is_drawn_as_a_box() {
        let mut fa = atlas();
        let ab = pixels(&mut fa, &TextLayout::new("ab", 32.0));
        let with_box = pixels(&mut fa, &TextLayout::new("a中b", 32.0));
        assert_eq!(with_box.missing, vec!['中']);
        assert!(with_box.width > ab.width + 10, "room for the box");
        let ink = |g: &RasterizedGlyphs| g.rgba_pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(ink(&with_box) > ink(&ab), "the box is drawn");
    }

    #[test]
    fn an_empty_atlas_draws_nothing_until_a_font_is_loaded() {
        let mut fa = FontAtlas::empty();
        assert!(fa.rasterize_text("Hello", 16.0, 0.0).is_none());
        assert_eq!(fa.load_font(LORA).unwrap(), "Lora");
        assert_eq!(
            fa.default_family(),
            Some("Lora"),
            "the first font is the default"
        );
        assert!(fa.rasterize_text("Hello", 16.0, 0.0).is_some());
    }

    #[test]
    fn bytes_that_arent_a_font_are_an_error() {
        assert!(FontAtlas::new(b"not a font").is_err());
        assert!(FontAtlas::new(&[]).is_err());
    }
}
