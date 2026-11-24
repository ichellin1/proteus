//! Laying out, measuring and rasterizing text, on the CPU.
//!
//! Two types share the work:
//! - [`Typesetter`] holds the fonts. It lays a text out with parley, which
//!   shapes it and breaks it into lines, and measures it without drawing it.
//!   `proteus-ui`'s `Fonts` resource holds one, so the app and the renderer
//!   use the same fonts.
//! - [`TextRasterizer`] draws a text the typesetter lays out, rasterizing its
//!   glyphs with swash into one RGBA image, and keeps each glyph's pixels in
//!   its glyph cache. The renderer holds one. The caller then allocates a
//!   main-atlas region for the image with
//!   [`crate::TextureRegistry::register_static`] and uploads it with
//!   [`crate::QuadPipeline::write_to_main_atlas`].
//!
//! ## Shaping
//!
//! Shaping turns characters into glyphs placed by the font's own rules. It
//! applies the font's kerning, which pulls pairs such as "AV" closer. It also
//! joins ligatures, places accents on their letters, and joins the letters of
//! scripts that need it. Each glyph is placed to a quarter of a pixel, so text
//! is as wide as its font designs it, at every size and display scale.
//!
//! ## Measuring
//!
//! [`Typesetter::measure`] gives the size of the image
//! [`TextRasterizer::rasterize`] makes, without drawing it. The image's height
//! comes from the text's lines, and its width from each glyph's pixels. A
//! glyph's pixels are its outline's bounds rounded out to whole pixels, as
//! swash works them out before drawing, so measuring reads the outlines and
//! draws nothing. One function sizes the image from the glyphs' pixels for
//! both, so they agree.
//!
//! ## The glyph cache
//!
//! Each glyph's coverage is kept after it's rasterized, so a glyph that
//! several texts share is rasterized once. Past a budget in bytes, the cache
//! drops the glyphs used longest ago. The budget is
//! [`DEFAULT_GLYPH_CACHE_BYTES`] unless
//! [`TextRasterizer::set_glyph_cache_budget`] changes it.
//! [`TextRasterizer::clear_glyph_cache`] drops every glyph. Text already baked
//! keeps its pixels, since they're in the atlas, not the cache.
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
//! ## Spans
//!
//! A text can be made of [`Span`]s, each a part of it with a style of its own,
//! such as a bold word. A span takes whatever it doesn't set from its text.
//! An ellipsis can be spans too.
//!
//! ## Text as a texture
//!
//! The whole text becomes one image, at its size in pixels. A text with no
//! colored span is white:
//! - Red, green and blue are 255 everywhere, so the text can take any color.
//! - Alpha is each pixel's glyph coverage, antialiased.
//!
//! The renderer tints it with the text's color as it draws it, so the color
//! can change without baking the text again. One image takes one tint, so a
//! text with a colored span is drawn in its colors instead. Each span is in its
//! own color, or in the text's color if it sets none, and its alpha is
//! multiplied by its color's alpha. See [`TextLayout::is_colored`].
//!
//! The text is then drawn like any other texture. It transitions, and is
//! clipped by rounded corners, with no special handling.
//!
//! ## Fonts
//!
//! A [`Typesetter`] has a default font, and any others loaded with
//! [`Typesetter::load_font`], each under the family name inside it. A text
//! names a family, or takes the default, and asks for a weight and italic. A
//! family that lacks the weight uses its nearest, as in CSS. A family with no
//! italic face is slanted, and one with no bold face is thickened.
//!
//! A character a text's family lacks is looked for in the fallback families
//! set with [`Typesetter::set_fallbacks`], in order, then in the default font.
//! A character none of them has is drawn as the text's family's missing-glyph
//! box, a rectangle, and reported in [`RasterizedGlyphs::missing`]. No system
//! font is ever used.
//!
//! ## The embedded font
//!
//! With the `embedded-font` feature, on by default, [`EMBEDDED_FONT_BYTES`] is
//! Inter (SIL Open Font License 1.1), variable in weight and optical size,
//! from Google Fonts. A text in it that sets no weight is drawn at 700, bold.
//! Without the feature, [`Typesetter::empty`] has no font until one is loaded.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use bevy_ecs::entity::Entity;
use glam::{Vec2, Vec4};

use crate::glyph_cache::{CachedGlyph, GlyphCache, GlyphKey};

use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache, SourceCacheOptions};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontData, FontFamily, FontFamilyName, FontStyle,
    FontWeight, InlineBox, InlineBoxKind, Layout, LayoutContext, LineHeight, OverflowWrap,
    PositionedInlineBox, PositionedLayoutItem, StyleProperty,
};
use swash::scale::outline::Outline;
use swash::scale::{Render, ScaleContext, Scaler, Source};
use swash::zeno::{self, Angle, Fill, Format, Transform, Vector};

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

/// The most glyph boxes a [`Typesetter`] keeps for measuring, about 1 MiB of
/// them. Past it, it empties them and starts again.
const MAX_GLYPH_BOXES: usize = 16_384;

/// The glyph cache's budget, 4 MiB, unless
/// [`TextRasterizer::set_glyph_cache_budget`] changes it. It holds from about 5,000
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

/// Where a text's box's top and bottom edges are, as CSS's `text-box-edge`
/// says. The box is what's measured and placed by the anchor, so a label in
/// a button is centered by it. The text's image is the same whatever the box:
/// ink outside the box, such as descenders and accents, still draws, outside
/// it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextBox {
    /// The line box: from the first line's top to the last line's bottom,
    /// with the room the font reserves above and below its letters, and any
    /// line spacing.
    #[default]
    Line,
    /// From the first line's x-height, the top of its lowercase letters, to
    /// the last line's baseline. It centers lowercase text.
    XHeight,
    /// From the first line's cap height, the top of its capitals, to the last
    /// line's baseline. It centers capitals, title case and digits.
    CapHeight,
}

/// How a text with more lines than its maximum ends.
#[derive(Clone, Debug, PartialEq)]
pub enum TextOverflow {
    /// Ends with these spans after the last line shown. The default is "…",
    /// and an app can set its own, such as `" more"`. An ellipsis made of one
    /// span that sets nothing is in the text's own style. "…" is drawn as
    /// "..." in a font that has no "…".
    Ellipsis(Vec<Span>),
    /// Leaves out the lines past the last shown, and adds no mark.
    Clip,
}

impl Default for TextOverflow {
    fn default() -> Self {
        Self::Ellipsis(default_ellipsis().to_vec())
    }
}

/// The ellipsis, "…", that a text ends in unless it sets its own.
pub const DEFAULT_ELLIPSIS: &str = "\u{2026}";

/// Returns the ellipsis a text ends in unless it sets its own, "…" in the
/// text's own style.
pub fn default_ellipsis() -> &'static [Span] {
    static ELLIPSIS: LazyLock<[Span; 1]> = LazyLock::new(|| [Span::new(DEFAULT_ELLIPSIS)]);
    &*ELLIPSIS
}

/// A part of a text with a style of its own, such as a bold word. Anything a
/// span doesn't set comes from its text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    /// The span's text. A `\n` starts a new line.
    pub content: String,
    /// The font family, such as `"Lora"`. `None` takes the text's family.
    pub family: Option<String>,
    /// The weight, from 100 to 900, where 400 is regular and 700 is bold.
    /// `None` takes the text's weight.
    pub weight: Option<u16>,
    /// Whether the span is italic. `None` takes the text's style.
    pub italic: Option<bool>,
    /// The font size, in logical pixels. `None` takes the text's size.
    pub size_px: Option<f32>,
    /// The span's color, RGBA. `None` takes the text's color. A text with a
    /// colored span is baked in its colors. See the module docs.
    pub color: Option<Vec4>,
    /// Where the span sits up or down in its line. The default is on the
    /// baseline.
    pub vertical_align: VerticalAlign,
    /// The component the span holds in place of text, if any. See
    /// [`Span::component`].
    pub component: Option<InlineComponent>,
}

/// A component a [`Span`] holds in place of text, and the size of the gap the
/// text keeps for it in its line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineComponent {
    /// The component's entity.
    pub entity: Entity,
    /// The gap's width and height, in logical pixels.
    pub size: Vec2,
}

/// The character that stands for a component in a text's content. parley
/// keeps a gap for it.
const OBJECT_REPLACEMENT: &str = "\u{FFFC}";

/// Where a span sits up or down in its line, as CSS's `vertical-align` says. A
/// span moved up or down makes its line taller if it needs the room, so it
/// never overlaps the lines around it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum VerticalAlign {
    /// On the text's baseline.
    #[default]
    Baseline,
    /// Centered on the middle of the text's lowercase letters, its x-height.
    /// Suits an icon beside text.
    Middle,
    /// Raised to the font's superscript position, such as for a footnote mark.
    /// The span's size isn't changed.
    Super,
    /// Lowered to the font's subscript position, such as for the 2 in H₂O. The
    /// span's size isn't changed.
    Sub,
    /// Raised by this many logical pixels, or lowered if it's negative.
    Raise(f32),
}

impl Span {
    /// Creates a span of `content` that sets nothing, so it's in its text's
    /// style.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ..Self::default()
        }
    }

    /// Sets the font family, such as `"Lora"`.
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }

    /// Sets the weight, from 100 to 900, where 400 is regular and 700 is bold.
    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Makes the span bold, a weight of 700.
    pub fn bold(self) -> Self {
        self.with_weight(700)
    }

    /// Makes the span italic.
    pub fn italic(mut self) -> Self {
        self.italic = Some(true);
        self
    }

    /// Sets the font size, in logical pixels.
    pub fn with_size(mut self, size_px: f32) -> Self {
        self.size_px = Some(size_px);
        self
    }

    /// Sets the span's color, RGBA.
    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets where the span sits up or down in its line.
    pub fn with_vertical_align(mut self, align: VerticalAlign) -> Self {
        self.vertical_align = align;
        self
    }

    /// Creates a span that holds the component `entity` in place of text. The
    /// text keeps a gap `size` logical pixels in its line, which wraps as a
    /// word does, and the component is placed in it. The span's vertical
    /// alignment places the gap: on the baseline, its bottom sits on it.
    pub fn component(entity: Entity, size: Vec2) -> Self {
        Self {
            component: Some(InlineComponent { entity, size }),
            ..Self::default()
        }
    }

    /// Returns whether the span draws nothing: it has no text and holds no
    /// component.
    fn is_empty(&self) -> bool {
        self.content.is_empty() && self.component.is_none()
    }
}

/// How to lay a text out, in logical pixels, from its size and font to how its
/// lines wrap and end. Make one with [`TextLayout::new`], then change the
/// fields it leaves at their defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLayout<'a> {
    /// The text. A `\n` starts a new line. Unused when `spans` has any.
    pub content: &'a str,
    /// The text's spans, in order. When there are any, they're the text, and
    /// `content` is unused.
    pub spans: &'a [Span],
    /// The color of spans that set none, in a text with a colored span. See
    /// [`TextLayout::is_colored`].
    pub color: Vec4,
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
    pub ellipsis: Option<&'a [Span]>,
    /// The font family. `None`, or a family that isn't loaded, uses the
    /// default font.
    pub family: Option<&'a str>,
    /// The weight, from 100 to 900, where 400 is regular and 700 is bold.
    /// `None` uses the font's own, or 700 in the embedded font.
    pub weight: Option<u16>,
    /// Whether the text is italic.
    pub italic: bool,
    /// Where the text's box's top and bottom edges are.
    pub text_box: TextBox,
}

impl<'a> TextLayout<'a> {
    /// Creates a [`TextLayout`] for `content` at `size_px`. Its lines break
    /// only at `\n`, align to the start, and use the font's line spacing, with
    /// no maximum number of lines.
    pub fn new(content: &'a str, size_px: f32) -> Self {
        Self {
            content,
            spans: &[],
            color: Vec4::ONE,
            size_px,
            letter_spacing_px: 0.0,
            wrap_width: None,
            align: TextAlign::Start,
            line_height: None,
            max_lines: None,
            ellipsis: Some(default_ellipsis()),
            family: None,
            weight: None,
            italic: false,
            text_box: TextBox::Line,
        }
    }

    /// Returns whether a span of the text or of its ellipsis sets a color.
    /// Such a text is baked in its colors, with spans that set none in
    /// [`TextLayout::color`]. Any other text is baked white, for the renderer to
    /// tint.
    pub fn is_colored(&self) -> bool {
        self.spans
            .iter()
            .chain(self.ellipsis.into_iter().flatten())
            .any(|span| span.color.is_some())
    }

    /// Returns the families the text and its spans name, including its
    /// ellipsis's spans, each once.
    pub fn families(&self) -> Vec<&'a str> {
        let mut families: Vec<&'a str> = Vec::new();
        let spans = self.spans.iter().chain(self.ellipsis.into_iter().flatten());
        for family in self
            .family
            .into_iter()
            .chain(spans.filter_map(|s| s.family.as_deref()))
        {
            if !families.contains(&family) {
                families.push(family);
            }
        }
        families
    }
}

// ---------------------------------------------------------------------------
// TextSize and RasterizedGlyphs
// ---------------------------------------------------------------------------

/// A text's size in pixels, from [`Typesetter::measure`]. It's the size
/// [`TextRasterizer::rasterize`] makes the text's image at the same scale.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSize {
    /// The image's width in pixels, which is also its box's.
    pub width: u32,
    /// The image's height in pixels.
    pub height: u32,
    /// How far the text's box starts below the image's top, in pixels. See
    /// [`TextBox`].
    pub box_top: u32,
    /// The text's box's height, in pixels.
    pub box_height: u32,
    /// The characters no font had, each once, in the order they first appear.
    pub missing: Vec<char>,
    /// Where each component the text shows goes in its image. A component
    /// whose gap is cut off isn't here.
    pub components: Vec<PlacedComponent>,
}

/// Where a span's component goes in a text's image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedComponent {
    /// The component's entity.
    pub entity: Entity,
    /// The gap's top-left corner, in pixels from the image's top-left corner,
    /// with y down.
    pub position: Vec2,
    /// The gap's size, in pixels.
    pub size: Vec2,
}

/// The pixels from one [`TextRasterizer::rasterize`] call, with no atlas
/// position yet. Allocate a region with
/// [`crate::TextureRegistry::register_static`], then upload with
/// [`crate::QuadPipeline::write_to_main_atlas`].
#[derive(Debug, Clone)]
pub struct RasterizedGlyphs {
    /// Width of the rasterized text image in pixels.
    pub width: u32,
    /// Height of the rasterized text image in pixels.
    pub height: u32,
    /// How far the text's box starts below the image's top, in pixels, as
    /// [`TextSize::box_top`] says.
    pub box_top: u32,
    /// The text's box's height, in pixels.
    pub box_height: u32,
    /// RGBA pixels, `width * height * 4` bytes, not premultiplied. A text with
    /// no colored span is white, with each pixel's glyph coverage in alpha. A
    /// text with one is in its spans' colors, with the coverage multiplied by
    /// each color's alpha. See [`TextLayout::is_colored`].
    pub rgba_pixels: Vec<u8>,
    /// The characters no font had, each once, in the order they first appear.
    /// Each is drawn as the font's missing-glyph box.
    pub missing: Vec<char>,
    /// Where each component the text shows goes in its image, as
    /// [`TextSize::components`] says.
    pub components: Vec<PlacedComponent>,
}

// ---------------------------------------------------------------------------
// Typesetter
// ---------------------------------------------------------------------------

/// The fonts an app has loaded, which lays text out in them and measures it.
/// It draws nothing: [`TextRasterizer`] draws a text from its layout. See the
/// module docs.
///
/// One is enough for an app. `proteus-ui`'s `Fonts` resource holds it, so the
/// app measures text with the same fonts the renderer draws it in.
pub struct Typesetter {
    fonts: FontContext,
    layout: LayoutContext<Rgba8>,
    /// Reads glyph outlines, for measuring.
    scale: ScaleContext,
    /// A glyph's outline, kept to reuse its memory.
    outline: Outline,
    /// Each glyph's pixels, as their left edge from the pen and their width,
    /// once measured, or `None` for a glyph with none. A glyph measured again
    /// isn't read again.
    boxes: HashMap<GlyphKey, Option<(i32, u32)>>,
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
}

impl std::fmt::Debug for Typesetter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Typesetter")
            .field("default_family", &self.default_family)
            .field("families", &self.families)
            .field("fallbacks", &self.fallbacks)
            .finish_non_exhaustive()
    }
}

impl Default for Typesetter {
    fn default() -> Self {
        Self::empty()
    }
}

/// Font bytes that [`Typesetter::load_font`] couldn't read as a TTF or OTF
/// font.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not a TTF or OTF font that can be read: {0}")]
pub struct FontError(String);

/// Returns the family name inside a TTF or OTF font, the one
/// [`Typesetter::load_font`] files it under, without loading it.
///
/// # Errors
///
/// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
pub fn font_family_name(font_bytes: &[u8]) -> Result<String, FontError> {
    Typesetter::empty().load_font(font_bytes)
}

/// Returns the characters in the first `line_count` lines of `layout`, which
/// was laid out from `content`, that no font had. Each appears once, in order.
fn missing_chars(layout: &Layout<Rgba8>, line_count: usize, content: &str) -> Vec<char> {
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

/// A color as 8-bit RGBA, not premultiplied: the brush parley lays each part
/// of a text out with, so each glyph knows its color.
type Rgba8 = [u8; 4];

/// Returns `color` as 8-bit RGBA, each channel clamped to 0 to 1 first.
fn rgba8(color: Vec4) -> Rgba8 {
    let c = (color.clamp(Vec4::ZERO, Vec4::ONE) * 255.0).round();
    [c.x as u8, c.y as u8, c.z as u8, c.w as u8]
}

/// A text's content, with the byte range of each span in it.
struct Styled<'a> {
    content: Cow<'a, str>,
    spans: Vec<(Range<usize>, &'a Span)>,
}

impl<'a> Styled<'a> {
    /// Joins `spans` into one content, or takes `content` if there are none.
    fn new(content: &'a str, spans: &'a [Span]) -> Self {
        if spans.is_empty() {
            return Self {
                content: Cow::Borrowed(content),
                spans: Vec::new(),
            };
        }
        let mut joined = String::new();
        let mut ranges = Vec::with_capacity(spans.len());
        for span in spans {
            let start = joined.len();
            if span.component.is_some() {
                joined.push_str(OBJECT_REPLACEMENT);
            } else {
                joined.push_str(&span.content);
            }
            ranges.push((start..joined.len(), span));
        }
        Self {
            content: Cow::Owned(joined),
            spans: ranges,
        }
    }

    /// Returns the first `len` bytes, with the spans cut to fit them, and
    /// `tail` after them.
    fn cut_then(&self, len: usize, tail: &Styled<'a>) -> Styled<'a> {
        let mut content = self.content[..len].to_owned();
        content.push_str(&tail.content);
        let head = self
            .spans
            .iter()
            .filter(|(range, _)| range.start < len)
            .map(|(range, span)| (range.start..range.end.min(len), *span));
        let tail_spans = tail
            .spans
            .iter()
            .map(|(range, span)| (range.start + len..range.end + len, *span));
        Styled {
            content: Cow::Owned(content),
            spans: head.chain(tail_spans).collect(),
        }
    }

    /// Returns the text without its leading whitespace.
    fn trim_start(&self) -> Styled<'a> {
        let skip = self.content.len() - self.content.trim_start().len();
        Styled {
            content: Cow::Owned(self.content[skip..].to_owned()),
            spans: self
                .spans
                .iter()
                .filter(|(range, _)| range.end > skip)
                .map(|(range, span)| (range.start.max(skip) - skip..range.end - skip, *span))
                .collect(),
        }
    }
}

/// A text laid out within its maximum number of lines.
struct Fitted {
    layout: Layout<Rgba8>,
    /// How many of the layout's lines are drawn. That's fewer than the layout
    /// has when the text is clipped.
    line_count: usize,
    /// What was laid out: the text, or its spans joined, or, when it ends in
    /// an ellipsis, as much of that as fits with the ellipsis after it.
    content: String,
}

/// A text's lines, laid out and fitted, with its image's height.
struct Lines {
    layout: Layout<Rgba8>,
    /// How many of the layout's lines are drawn.
    line_count: usize,
    /// What was laid out. See [`Fitted::content`].
    content: String,
    /// Where the first line's baseline is in the layout.
    baseline: f32,
    /// How far the image reaches above the first line's baseline, in whole
    /// pixels.
    ascent_px: i32,
    /// The image's height in pixels.
    height: u32,
    /// How far the box starts below the image's top, in pixels.
    box_top: u32,
    /// The box's height, in pixels.
    box_height: u32,
}

/// The glyphs of one run, a part of a line in one font, size and style,
/// placed for one text.
struct PlacedRun {
    font: FontData,
    size: f32,
    /// The variable font's axes, such as its weight, normalized.
    coords: Box<[i16]>,
    /// The slant of a synthesized italic, in degrees.
    slant: Option<f32>,
    /// How much a synthesized bold is thickened, in pixels.
    embolden: Option<f32>,
    color: Rgba8,
    glyphs: Vec<GlyphSpot>,
}

/// Where a glyph is drawn: its pen position, in whole pixels from the text's
/// pen start and its first line's baseline, and the quarter pixel it's
/// rasterized at.
struct GlyphSpot {
    id: u32,
    pen_x: i32,
    pen_y: i32,
    quarter: u8,
}

impl GlyphSpot {
    /// How far right of `pen_x` the glyph starts, in pixels.
    fn offset(&self) -> f32 {
        self.quarter as f32 / SUBPIXEL_STEPS
    }
}

impl PlacedRun {
    /// Returns a scaler for the run's font, at its size and axes, or `None`
    /// if swash can't read the font.
    fn scaler<'a>(&'a self, context: &'a mut ScaleContext) -> Option<Scaler<'a>> {
        let font = swash::FontRef::from_index(self.font.data.as_ref(), self.font.index as usize)?;
        Some(
            context
                .builder(font)
                .size(self.size)
                .hint(false)
                .normalized_coords(self.coords.iter())
                .build(),
        )
    }

    /// Returns the slant of a synthesized italic as a transform.
    fn transform(&self) -> Option<Transform> {
        self.slant
            .map(|degrees| Transform::skew(Angle::from_degrees(degrees), Angle::ZERO))
    }

    /// Returns the glyph cache's key for `spot`'s glyph.
    fn key(&self, spot: &GlyphSpot) -> GlyphKey {
        GlyphKey {
            font: self.font.data.id(),
            index: self.font.index,
            glyph: spot.id,
            size_bits: self.size.to_bits(),
            coords: self.coords.clone(),
            quarter: spot.quarter,
            slanted: self.slant.is_some(),
            emboldened: self.embolden.is_some(),
        }
    }
}

/// Places every glyph in the first `line_count` lines of `layout`, relative to
/// the pen's start and the first line's baseline, which is at `baseline`.
fn place_runs(layout: &Layout<Rgba8>, baseline: f32, line_count: usize) -> Vec<PlacedRun> {
    let mut runs = Vec::new();
    for line in layout.lines().take(line_count) {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            // A face standing in for one the family lacks is slanted for
            // italic and thickened for bold. The thickening is about a
            // thirtieth of the font size.
            let synthesis = run.synthesis();
            let glyphs = glyph_run
                .positioned_glyphs()
                .map(|g| {
                    // Horizontally, a glyph is placed to the nearest quarter
                    // pixel. Vertically, it's placed to the nearest pixel, so
                    // lines sit on whole pixels.
                    let x = (g.x * SUBPIXEL_STEPS).round() / SUBPIXEL_STEPS;
                    let pen_x = x.floor();
                    GlyphSpot {
                        id: g.id,
                        pen_x: pen_x as i32,
                        pen_y: (g.y - baseline).round() as i32,
                        quarter: ((x - pen_x) * SUBPIXEL_STEPS) as u8,
                    }
                })
                .collect();
            runs.push(PlacedRun {
                font: run.font().clone(),
                size: run.font_size(),
                coords: run
                    .normalized_coords()
                    .iter()
                    .map(|c| c.to_bits())
                    .collect(),
                slant: synthesis.skew().map(|_| SYNTHESIZED_SLANT),
                embolden: synthesis.embolden().then(|| run.font_size() / 30.0),
                color: glyph_run.style().brush,
                glyphs,
            });
        }
    }
    runs
}

/// Returns how far `text_box`'s top edge is above the first line's baseline,
/// in pixels: the tallest x-height or cap height of the fonts that draw the
/// first line, at their sizes. Returns `None` for [`TextBox::Line`], and for a
/// first line with no text in it, whose box is the line box.
fn first_line_height(layout: &Layout<Rgba8>, text_box: TextBox) -> Option<f32> {
    if text_box == TextBox::Line {
        return None;
    }
    let mut tallest: Option<f32> = None;
    for item in layout.lines().next()?.items() {
        let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
            continue;
        };
        let run = glyph_run.run();
        let font = run.font();
        let Some(font_ref) = swash::FontRef::from_index(font.data.as_ref(), font.index as usize)
        else {
            continue;
        };
        let coords: Vec<i16> = run
            .normalized_coords()
            .iter()
            .map(|c| c.to_bits())
            .collect();
        let metrics = font_ref.metrics(&coords).scale(run.font_size());
        let height = match text_box {
            TextBox::XHeight => metrics.x_height,
            _ => metrics.cap_height,
        };
        tallest = Some(tallest.map_or(height, |t| t.max(height)));
    }
    tallest
}

/// Returns parley's vertical alignment for `align`, or `None` on the baseline.
/// A raise in logical pixels is multiplied by `scale`.
fn parley_align(align: VerticalAlign, scale: f32) -> Option<parley::VerticalAlign> {
    match align {
        VerticalAlign::Baseline => None,
        VerticalAlign::Middle => Some(parley::VerticalAlign::MIDDLE),
        VerticalAlign::Super => Some(parley::VerticalAlign::SUPER),
        VerticalAlign::Sub => Some(parley::VerticalAlign::SUB),
        VerticalAlign::Raise(px) => Some(parley::VerticalAlign::new(
            parley::AlignmentBaseline::Baseline,
            parley::BaselineShift::Length(px * scale),
        )),
    }
}

/// Returns the gaps kept for components in the first `line_count` lines of
/// `layout`.
fn gaps(layout: &Layout<Rgba8>, line_count: usize) -> Vec<PositionedInlineBox> {
    let mut gaps = Vec::new();
    for line in layout.lines().take(line_count) {
        for item in line.items() {
            if let PositionedLayoutItem::InlineBox(gap) = item {
                gaps.push(gap);
            }
        }
    }
    gaps
}

/// Returns each gap's pixels as their left edge from the pen's start and their
/// width, as [`image_width`] takes a glyph's, so a gap widens the image as ink
/// does.
fn gap_extents(gaps: &[PositionedInlineBox]) -> impl Iterator<Item = (i32, u32)> + '_ {
    gaps.iter().map(|gap| {
        let left = gap.x.floor();
        (left as i32, ((gap.x + gap.width).ceil() - left) as u32)
    })
}

/// Returns where each gap's component goes in a text's image, which starts
/// `min_left` pixels from the pen's start, at the top of `lines`' first line.
fn place_components(
    gaps: &[PositionedInlineBox],
    lines: &Lines,
    min_left: i32,
) -> Vec<PlacedComponent> {
    let top = lines.baseline - lines.ascent_px as f32;
    gaps.iter()
        .map(|gap| PlacedComponent {
            entity: Entity::from_bits(gap.id),
            position: Vec2::new(gap.x - min_left as f32, gap.y - top),
            size: Vec2::new(gap.width, gap.height),
        })
        .collect()
}

/// Returns where a glyph's pixels start, from its pen position, and how wide
/// they are, without drawing them, or `None` for a glyph with no pixels. They
/// are as swash places them: the glyph's outline, thickened then slanted,
/// offset to its quarter pixel, and rounded out to whole pixels. `outline` is
/// kept to reuse its memory.
fn outline_box(
    scaler: &mut Scaler,
    outline: &mut Outline,
    run: &PlacedRun,
    spot: &GlyphSpot,
) -> Option<(i32, u32)> {
    outline.clear();
    if !scaler.scale_outline_into(spot.id as u16, outline) {
        return None;
    }
    if let Some(strength) = run.embolden {
        outline.embolden(strength, strength);
    }
    if let Some(transform) = run.transform() {
        outline.transform(&transform);
    }
    let bounds = zeno::bounds(outline.path(), Fill::NonZero, None);
    let left = (bounds.min.x + spot.offset()).floor();
    let right = (bounds.max.x + spot.offset()).ceil();
    let height = bounds.max.y.ceil() - bounds.min.y.floor();
    (right > left && height > 0.0).then_some((left as i32, (right - left) as u32))
}

/// Returns where a text's image starts, in pixels from the pen's start, and
/// how wide it is, from each glyph's pixels as their left edge and width.
/// Measuring and rasterizing both size the image with it, so they agree.
///
/// The image is the wrap width, if there is one, widened to take in ink that
/// reaches past it. Without a wrap width, it runs from the leftmost ink, or the
/// pen's start if that's further left, to the rightmost ink. A glyph's ink can
/// reach left of the pen, as Inter's "j" does, or past its own advance.
fn image_width(glyphs: &[(i32, u32)], wrap_width: Option<f32>, scale: f32) -> Option<(i32, u32)> {
    let box_width = wrap_width.map_or(0, |w| (w * scale).ceil() as i32);
    let min_left = glyphs.iter().map(|g| g.0).min()?.min(0);
    let max_right = glyphs
        .iter()
        .map(|g| g.0 + g.1 as i32)
        .max()?
        .max(box_width);
    let width = (max_right - min_left).max(0) as u32;
    (width > 0).then_some((min_left, width))
}

impl Typesetter {
    /// Creates a [`Typesetter`] for the TTF or OTF font in `font_bytes`.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
    pub fn new(font_bytes: &[u8]) -> Result<Self, FontError> {
        let mut typesetter = Self::empty();
        typesetter.set_default_font(font_bytes)?;
        Ok(typesetter)
    }

    /// Creates a [`Typesetter`] for the embedded font, [`EMBEDDED_FONT_BYTES`]
    /// (Inter). A text in it that sets no weight is drawn at 700.
    #[cfg(feature = "embedded-font")]
    pub fn with_embedded_font() -> Self {
        let mut typesetter = Self::empty();
        typesetter.use_embedded_font();
        typesetter
    }

    /// Creates a [`Typesetter`] with no font, which lays out no text until one
    /// is loaded with [`Typesetter::load_font`]. The first font loaded becomes
    /// the default.
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
            outline: Outline::new(),
            boxes: HashMap::new(),
            default_family: None,
            embedded_family: None,
            families: HashSet::new(),
            has_ellipsis: false,
            warned_no_font: false,
            fallbacks: Vec::new(),
            warned_missing: HashSet::new(),
        }
    }

    /// Adds the TTF or OTF font in `font_bytes`, and returns the family name
    /// inside it. Texts use the font by naming that family. A font whose family
    /// is already loaded adds a face to it, such as an italic. The first font
    /// loaded into an [`empty`](Self::empty) typesetter becomes its default.
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

    /// Adds the TTF or OTF font in `font_bytes` and makes it the default, in
    /// place of any default before it. Returns the family name inside it.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `font_bytes` isn't a TTF or OTF font that can be read.
    /// The default is then unchanged.
    pub fn set_default_font(&mut self, font_bytes: &[u8]) -> Result<String, FontError> {
        let family = self.register(Blob::new(Arc::new(font_bytes.to_vec())))?;
        self.has_ellipsis = has_ellipsis(font_bytes);
        self.default_family = Some(family.clone());
        self.embedded_family = None;
        Ok(family)
    }

    /// Makes the embedded font, [`EMBEDDED_FONT_BYTES`] (Inter), the default,
    /// in place of any default before it. It's added only once.
    #[cfg(feature = "embedded-font")]
    pub fn use_embedded_font(&mut self) {
        let family = match &self.embedded_family {
            Some(family) => family.clone(),
            None => self
                .register(Blob::new(Arc::new(EMBEDDED_FONT_BYTES)))
                .expect("the embedded font is a valid TTF"),
        };
        self.has_ellipsis = has_ellipsis(EMBEDDED_FONT_BYTES);
        self.embedded_family = Some(family.clone());
        self.default_family = Some(family);
    }

    /// Sets the fallback families. For a character a text's own family lacks,
    /// they're tried in order, before the default font. A family that isn't
    /// loaded yet is skipped until it is. See the module docs.
    pub fn set_fallbacks(&mut self, families: &[String]) {
        self.fallbacks = families.to_vec();
    }

    /// Returns the fallback families, in the order they're tried.
    pub fn fallbacks(&self) -> &[String] {
        &self.fallbacks
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

    /// Returns the size of the image [`TextRasterizer::rasterize`] makes of
    /// `text` at `scale`, without drawing it. A glyph's pixels are its
    /// outline's bounds rounded out to whole pixels, as swash works them out
    /// before drawing, so measuring reads outlines and draws nothing.
    ///
    /// Returns `None` if `text` draws no pixels, because it's empty or only
    /// whitespace, or there's no font.
    pub fn measure(&mut self, text: &TextLayout, scale: f32) -> Option<TextSize> {
        let lines = self.lines(text, scale)?;
        let runs = place_runs(&lines.layout, lines.baseline, lines.line_count);
        let mut glyphs = Vec::new();
        for run in &runs {
            let Some(mut scaler) = run.scaler(&mut self.scale) else {
                continue;
            };
            for spot in &run.glyphs {
                let key = run.key(spot);
                let glyph_box = match self.boxes.get(&key) {
                    Some(glyph_box) => *glyph_box,
                    None => {
                        let glyph_box = outline_box(&mut scaler, &mut self.outline, run, spot);
                        if self.boxes.len() >= MAX_GLYPH_BOXES {
                            self.boxes.clear();
                        }
                        self.boxes.insert(key, glyph_box);
                        glyph_box
                    }
                };
                if let Some((left, width)) = glyph_box {
                    glyphs.push((spot.pen_x + left, width));
                }
            }
        }
        let gaps = gaps(&lines.layout, lines.line_count);
        glyphs.extend(gap_extents(&gaps));
        let (min_left, width) = image_width(&glyphs, text.wrap_width, scale)?;
        Some(TextSize {
            width,
            height: lines.height,
            box_top: lines.box_top,
            box_height: lines.box_height,
            missing: missing_chars(&lines.layout, lines.line_count, &lines.content),
            components: place_components(&gaps, &lines, min_left),
        })
    }

    /// Lays `text` out and fits it, and works out its image's height. Returns
    /// `None` for a text that draws nothing, or with no font, which is warned
    /// about once.
    fn lines(&mut self, text: &TextLayout, scale: f32) -> Option<Lines> {
        let empty = if text.spans.is_empty() {
            text.content.is_empty()
        } else {
            text.spans.iter().all(Span::is_empty)
        };
        if empty {
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
        // A box to the x-height or cap height ends at the last line's
        // baseline, which is rounded to a whole pixel as glyphs are placed.
        let (box_top, box_height) = match first_line_height(&layout, text.text_box) {
            Some(above) => {
                let top = (ascent_px - (above.round() as i32)).max(0);
                let bottom = ascent_px + (last.baseline - first.baseline).round() as i32;
                (top as u32, (bottom - top).clamp(0, height) as u32)
            }
            None => (0, height as u32),
        };
        Some(Lines {
            layout,
            line_count,
            content,
            baseline: first.baseline,
            ascent_px,
            height: height as u32,
            box_top,
            box_height,
        })
    }

    /// Logs a warning for each of `missing`, the characters no font had in
    /// `content`, the first time it's drawn.
    fn warn_missing(&mut self, missing: &[char], content: &str) {
        for &c in missing {
            if self.warned_missing.insert(c) {
                log::warn!(
                    "no font has {c:?} (U+{:04X}), in the text {:?}, so it's drawn as a box: add a \
                     font that has it to the fallback fonts",
                    c as u32,
                    content,
                );
            }
        }
    }

    /// Lays `text` out within its maximum number of lines. Past the maximum,
    /// an ellipsis replaces the end of what fits, or the lines past it are
    /// left out.
    fn fit(&mut self, text: &TextLayout, scale: f32) -> Fitted {
        let styled = Styled::new(text.content, text.spans);
        let full = self.lay_out(&styled, text, scale);
        let line_count = full.len();
        let fitted = |layout, line_count, content| Fitted {
            layout,
            line_count,
            content,
        };
        let whole = || styled.content.clone().into_owned();
        let Some(max) = text.max_lines.map(|n| n.max(1) as usize) else {
            return fitted(full, line_count, whole());
        };
        if line_count <= max {
            return fitted(full, line_count, whole());
        }
        let Some(ellipsis) = text.ellipsis else {
            return fitted(full, max, whole());
        };
        // "…" is drawn as "..." in a font that has no "…", in the same style.
        let dots;
        let ellipsis = match ellipsis {
            [only] if only.content == DEFAULT_ELLIPSIS && !self.has_ellipsis => {
                dots = [Span {
                    content: "...".to_owned(),
                    ..only.clone()
                }];
                &dots[..]
            }
            _ => ellipsis,
        };
        let ellipsis = Styled::new("", ellipsis);

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
            let head = styled.content[..cut].trim_end().len();
            if head == 0 {
                ellipsis.trim_start()
            } else {
                styled.cut_then(head, &ellipsis)
            }
        };
        // Lays out the text cut at `cut`, with the ellipsis, and returns it if
        // it fits in `max` lines. A binary search finds the longest cut that
        // fits, since a shorter cut never takes more lines.
        let fits = |typesetter: &mut Self, cut: usize| {
            let cut = with_ellipsis(cut);
            let layout = typesetter.lay_out(&cut, text, scale);
            (layout.len() <= max).then(|| (layout, cut.content.into_owned()))
        };
        let Some(mut best) = fits(self, cuts[0]) else {
            log::warn!(
                "the ellipsis {:?} of the text {:?} needs more than its {max} line(s) on its \
                 own, at a wrap width of {:?}, so it's clipped: raise its maximum number of \
                 lines, widen its wrap width, or shorten its ellipsis",
                ellipsis.content,
                styled.content,
                text.wrap_width,
            );
            let alone = ellipsis.trim_start();
            let layout = self.lay_out(&alone, text, scale);
            return fitted(layout, max, alone.content.into_owned());
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
        fitted(layout, line_count, content)
    }

    /// Returns the font stack for a text or span in `family`, and the weight
    /// it's drawn at. The stack is the family, then the fallbacks in order,
    /// then the default font. Each is tried in turn for a character the ones
    /// before it lack. A family that isn't loaded is left out, so the default
    /// font draws it. A text in the embedded font that sets no weight is bold.
    fn font_stack(
        &self,
        family: Option<&str>,
        weight: Option<u16>,
    ) -> (FontFamily<'static>, Option<u16>) {
        let family = family
            .filter(|f| self.families.contains(*f))
            .or(self.default_family.as_deref())
            .unwrap_or_default()
            .to_owned();
        let weight = weight.or_else(|| {
            (self.embedded_family.as_ref() == Some(&family)).then_some(EMBEDDED_DEFAULT_WEIGHT)
        });
        let mut stack = vec![family];
        let loaded = self.fallbacks.iter().filter(|f| self.families.contains(*f));
        for name in loaded.chain(&self.default_family) {
            if !stack.contains(name) {
                stack.push(name.clone());
            }
        }
        let names: Vec<FontFamilyName<'static>> = stack
            .into_iter()
            .map(|name| FontFamilyName::Named(name.into()))
            .collect();
        (FontFamily::List(names.into()), weight)
    }

    /// Shapes `styled` in `text`'s style, with each span's own, breaks it into
    /// lines, and aligns them, at `scale`.
    fn lay_out(&mut self, styled: &Styled, text: &TextLayout, scale: f32) -> Layout<Rgba8> {
        let (stack, weight) = self.font_stack(text.family, text.weight);
        // Each span that sets a family or a weight gets its own stack and
        // weight. One that sets neither takes the text's.
        let span_fonts: Vec<_> = styled
            .spans
            .iter()
            .map(|(_, span)| {
                (span.family.is_some() || span.weight.is_some()).then(|| {
                    self.font_stack(
                        span.family.as_deref().or(text.family),
                        span.weight.or(text.weight),
                    )
                })
            })
            .collect();
        // A text with no colored span is white, for the renderer to tint. In
        // one with a colored span, a span that sets no color is in the text's,
        // and the text's alpha is left to the renderer.
        let brush = if text.is_colored() {
            rgba8(text.color.with_w(1.0))
        } else {
            [255; 4]
        };

        // Sizes are in logical pixels, and parley scales them. Positions are
        // fractional, and rounded when the glyphs are rasterized.
        let mut builder =
            self.layout
                .ranged_builder(&mut self.fonts, &styled.content, scale, false);
        builder.push_default(StyleProperty::FontFamily(stack));
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
        builder.push_default(StyleProperty::Brush(brush));
        // A word wider than the wrap width breaks between characters, rather
        // than running out of the box.
        builder.push_default(StyleProperty::OverflowWrap(OverflowWrap::Anywhere));
        for ((range, span), font) in styled.spans.iter().zip(span_fonts) {
            // A component's gap has no style of its own: parley aligns it by
            // its own vertical alignment, against the text's style. Its size
            // is in logical pixels, and parley doesn't scale it.
            if let Some(component) = span.component {
                builder.push_inline_box(InlineBox {
                    id: component.entity.to_bits(),
                    kind: InlineBoxKind::InFlow,
                    index: range.start,
                    width: component.size.x * scale,
                    height: component.size.y * scale,
                    baseline: None,
                    vertical_align: parley_align(span.vertical_align, scale)
                        .unwrap_or(parley::VerticalAlign::BASELINE),
                });
                continue;
            }
            if let Some((stack, weight)) = font {
                builder.push(StyleProperty::FontFamily(stack), range.clone());
                // A span with no weight is drawn at its font's own.
                let weight = weight.map_or(FontWeight::NORMAL, |w| FontWeight::new(w as f32));
                builder.push(StyleProperty::FontWeight(weight), range.clone());
            }
            if let Some(italic) = span.italic {
                let style = if italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
                builder.push(StyleProperty::FontStyle(style), range.clone());
            }
            if let Some(size_px) = span.size_px {
                builder.push(StyleProperty::FontSize(size_px), range.clone());
            }
            if let Some(color) = span.color {
                builder.push(StyleProperty::Brush(rgba8(color)), range.clone());
            }
            // A length in logical pixels, which parley scales in a style.
            if let Some(align) = parley_align(span.vertical_align, 1.0) {
                builder.push(StyleProperty::VerticalAlign(align), range.clone());
            }
        }
        let mut layout = builder.build(&styled.content);
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
}

// ---------------------------------------------------------------------------
// TextRasterizer
// ---------------------------------------------------------------------------

/// Draws a text a [`Typesetter`] lays out into an RGBA image, keeping each
/// glyph's pixels in its glyph cache. It allocates no atlas space (see the
/// module docs).
///
/// The renderer owns one. Call [`TextRasterizer::rasterize`] for each text,
/// allocate a main-atlas region for the result with
/// `TextureRegistry::register_static`, then upload it with
/// `QuadPipeline::write_to_main_atlas`.
pub struct TextRasterizer {
    scale: ScaleContext,
    glyphs: GlyphCache,
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
    /// The color it's drawn in.
    color: Rgba8,
}

impl Default for TextRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextRasterizer {
    /// Creates a [`TextRasterizer`] with an empty glyph cache, whose budget is
    /// [`DEFAULT_GLYPH_CACHE_BYTES`].
    pub fn new() -> Self {
        Self {
            scale: ScaleContext::new(),
            glyphs: GlyphCache::new(DEFAULT_GLYPH_CACHE_BYTES),
        }
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

    /// Lays `text` out at `size_px` with `typesetter`, with
    /// `letter_spacing_px` of extra space between glyphs (`0.0` for the font's
    /// normal spacing), and rasterizes it into an RGBA pixel buffer. A `\n`
    /// starts a new line. The image spans the glyphs' ink, so it has no
    /// padding after the last glyph. This is the same as
    /// [`TextRasterizer::rasterize`] with a [`TextLayout::new`] and a scale of
    /// 1.
    ///
    /// Returns `None` if `text` draws no pixels, because it's empty or only
    /// whitespace.
    pub fn rasterize_text(
        &mut self,
        typesetter: &mut Typesetter,
        text: &str,
        size_px: f32,
        letter_spacing_px: f32,
    ) -> Option<RasterizedGlyphs> {
        let mut layout = TextLayout::new(text, size_px);
        layout.letter_spacing_px = letter_spacing_px;
        self.rasterize(typesetter, &layout, 1.0)
    }

    /// Lays `text` out with `typesetter` and rasterizes it into an RGBA pixel
    /// buffer, at `scale` pixels per logical pixel, the way a bake at the
    /// display's scale does. Lines break where they would at a scale of 1. See
    /// the module docs.
    ///
    /// The image is the text's box. It's as wide as the wrap width, or as the
    /// widest line's ink without one, and as tall as from the top of the first
    /// line to the bottom of the last. Ink that reaches past the box widens the
    /// image. [`Typesetter::measure`] gives its size without drawing it.
    ///
    /// Returns `None` if `text` draws no pixels, because it's empty or only
    /// whitespace.
    pub fn rasterize(
        &mut self,
        typesetter: &mut Typesetter,
        text: &TextLayout,
        scale: f32,
    ) -> Option<RasterizedGlyphs> {
        let lines = typesetter.lines(text, scale)?;
        let runs = place_runs(&lines.layout, lines.baseline, lines.line_count);
        let glyphs = self.rasterize_glyphs(&runs);
        let gaps = gaps(&lines.layout, lines.line_count);
        let boxes: Vec<_> = glyphs
            .iter()
            .map(|g| (g.left, g.width))
            .chain(gap_extents(&gaps))
            .collect();
        let (min_left, width) = image_width(&boxes, text.wrap_width, scale)?;
        let height = lines.height as i32;
        let ascent_px = lines.ascent_px;

        // Each glyph in its color, with its coverage times the color's alpha in
        // alpha. Where glyphs overlap, the greater alpha wins. A text with no
        // colored span is white, with the coverage in alpha.
        let mut rgba = vec![0u8; (width * lines.height * 4) as usize];
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
                    let alpha = ((coverage as u32 * g.color[3] as u32 + 127) / 255) as u8;
                    if alpha > rgba[i + 3] {
                        rgba[i..i + 3].copy_from_slice(&g.color[..3]);
                        rgba[i + 3] = alpha;
                    }
                }
            }
        }

        let missing = missing_chars(&lines.layout, lines.line_count, &lines.content);
        typesetter.warn_missing(&missing, &lines.content);
        Some(RasterizedGlyphs {
            width,
            height: lines.height,
            box_top: lines.box_top,
            box_height: lines.box_height,
            rgba_pixels: rgba,
            missing,
            components: place_components(&gaps, &lines, min_left),
        })
    }

    /// Rasterizes every glyph of `runs`, or takes it from the glyph cache.
    fn rasterize_glyphs(&mut self, runs: &[PlacedRun]) -> Vec<PlacedGlyph> {
        let mut glyphs = Vec::new();
        for run in runs {
            let Some(mut scaler) = run.scaler(&mut self.scale) else {
                continue;
            };
            for spot in &run.glyphs {
                let glyph = self.glyphs.get_or_rasterize(run.key(spot), || {
                    let image = Render::new(&[Source::Outline])
                        .format(Format::Alpha)
                        .offset(Vector::new(spot.offset(), 0.0))
                        .transform(run.transform())
                        .embolden(run.embolden.unwrap_or(0.0))
                        .render(&mut scaler, spot.id as u16)?;
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
                    left: spot.pen_x + glyph.left,
                    top: spot.pen_y + glyph.top,
                    width: glyph.width,
                    height: glyph.height,
                    coverage: glyph.coverage,
                    color: run.color,
                });
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

    /// A typesetter and a rasterizer together, as the app and the renderer
    /// hold them. Every text it rasterizes is measured too, and must measure
    /// the size it's rasterized at.
    struct Kit {
        typesetter: Typesetter,
        rasterizer: TextRasterizer,
    }

    impl Kit {
        fn empty() -> Self {
            Self {
                typesetter: Typesetter::empty(),
                rasterizer: TextRasterizer::new(),
            }
        }

        fn rasterize(&mut self, text: &TextLayout, scale: f32) -> Option<RasterizedGlyphs> {
            let measured = self.typesetter.measure(text, scale);
            let glyphs = self.rasterizer.rasterize(&mut self.typesetter, text, scale);
            let size = glyphs.as_ref().map(|g| TextSize {
                width: g.width,
                height: g.height,
                box_top: g.box_top,
                box_height: g.box_height,
                missing: g.missing.clone(),
                components: g.components.clone(),
            });
            assert_eq!(
                measured, size,
                "measured as rasterized, at a scale of {scale}"
            );
            glyphs
        }

        fn rasterize_text(
            &mut self,
            text: &str,
            size_px: f32,
            letter_spacing_px: f32,
        ) -> Option<RasterizedGlyphs> {
            let mut layout = TextLayout::new(text, size_px);
            layout.letter_spacing_px = letter_spacing_px;
            self.rasterize(&layout, 1.0)
        }

        fn set_glyph_cache_budget(&mut self, bytes: usize) {
            self.rasterizer.set_glyph_cache_budget(bytes);
        }

        fn clear_glyph_cache(&mut self) {
            self.rasterizer.clear_glyph_cache();
        }

        fn glyph_cache_size(&self) -> (usize, usize) {
            self.rasterizer.glyph_cache_size()
        }
    }

    impl std::ops::Deref for Kit {
        type Target = Typesetter;
        fn deref(&self) -> &Typesetter {
            &self.typesetter
        }
    }

    impl std::ops::DerefMut for Kit {
        fn deref_mut(&mut self) -> &mut Typesetter {
            &mut self.typesetter
        }
    }

    fn atlas() -> Kit {
        Kit {
            typesetter: Typesetter::with_embedded_font(),
            rasterizer: TextRasterizer::new(),
        }
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
            .lay_out(&Styled::new("AV", &[]), &TextLayout::new("AV", 64.0), 1.0)
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
        let starts = |fa: &mut Kit, scale| -> Vec<usize> {
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
        assert_eq!(fitted.content, text.content, "clipped, not changed");
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
        let content = fitted.content;
        assert!(content.ends_with('\u{2026}'), "{content:?}");
        let head = content.trim_end_matches('\u{2026}');
        assert!(PARAGRAPH.starts_with(head));
        assert!(!head.ends_with(' '), "no space before the ellipsis");
        // One more word wouldn't have fitted.
        let next = PARAGRAPH[head.len()..].trim_start();
        let one_more = next.split(' ').next().unwrap();
        let longer = format!("{head} {one_more}\u{2026}");
        assert!(fa.lay_out(&Styled::new(&longer, &[]), &text, 1.0).len() > 2);
    }

    // The app's own ellipsis is used. It wraps onto a line of its own when it
    // doesn't fit beside the text.
    #[test]
    fn an_ellipsis_wider_than_a_line_wraps_onto_its_own() {
        let mut fa = atlas();
        let mut text = TextLayout::new(PARAGRAPH, 16.0);
        text.wrap_width = Some(90.0);
        text.max_lines = Some(3);
        let ellipsis = [Span::new(" read the whole article")];
        text.ellipsis = Some(&ellipsis);
        let fitted = fa.fit(&text, 1.0);
        let content = fitted.content;
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
        let ellipsis = [Span::new(" read the whole article")];
        text.ellipsis = Some(&ellipsis);
        let fitted = fa.fit(&text, 1.0);
        assert_eq!(fitted.content, "read the whole article");
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
            let content = fa.fit(&text, 1.0).content;
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
        let content = fa.fit(&text, 1.0).content;
        assert!(content.ends_with("..."), "{content:?}");
        let more = [Span::new(" more")];
        text.ellipsis = Some(&more);
        let content = fa.fit(&text, 1.0).content;
        assert!(content.ends_with(" more"), "the app's own ellipsis is kept");
    }

    #[test]
    fn text_within_its_maximum_isnt_changed() {
        let mut fa = atlas();
        let mut text = TextLayout::new("Two\nlines", 16.0);
        text.max_lines = Some(2);
        assert_eq!(fa.fit(&text, 1.0).content, "Two\nlines");
    }

    // Lora, roman and italic, is a second family. It's variable from 400 to
    // 700.
    use proteus_test_fonts::{SECOND_FAMILY as LORA, SECOND_FAMILY_ITALIC as LORA_ITALIC};

    fn pixels(fa: &mut Kit, text: &TextLayout) -> RasterizedGlyphs {
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
        let mut in_family = |fa: &mut Kit, family: &'static str| {
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
        let mut fa = Kit::empty();
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
        assert!(Typesetter::new(b"not a font").is_err());
        assert!(Typesetter::new(&[]).is_err());
    }

    // Spans.

    const RED: Vec4 = Vec4::new(1.0, 0.0, 0.0, 1.0);
    const BLUE: Vec4 = Vec4::new(0.0, 0.0, 1.0, 1.0);

    // Returns how much ink an image has: the sum of its alpha.
    fn ink(g: &RasterizedGlyphs) -> u64 {
        g.rgba_pixels.chunks(4).map(|p| p[3] as u64).sum()
    }

    // Returns the colors of the pixels with any ink, each once.
    fn colors(g: &RasterizedGlyphs) -> Vec<[u8; 3]> {
        let mut colors: Vec<[u8; 3]> = Vec::new();
        for p in g.rgba_pixels.chunks(4).filter(|p| p[3] > 0) {
            let rgb = [p[0], p[1], p[2]];
            if !colors.contains(&rgb) {
                colors.push(rgb);
            }
        }
        colors
    }

    // Spans that set nothing take everything from the text, so they draw the
    // same pixels as the text's content would.
    #[test]
    fn spans_that_set_nothing_draw_as_the_text_does() {
        let mut fa = atlas();
        let plain = pixels(&mut fa, &TextLayout::new("Hello world", 24.0));
        let spans = [Span::new("Hello "), Span::new("world")];
        let mut text = TextLayout::new("", 24.0);
        text.spans = &spans;
        assert_eq!(pixels(&mut fa, &text).rgba_pixels, plain.rgba_pixels);
        assert!(!text.is_colored());
    }

    // A span's weight, italic, size and family each change its glyphs, and
    // only its own.
    #[test]
    fn a_span_sets_its_own_weight_italic_size_and_family() {
        let mut fa = atlas();
        fa.load_font(LORA).unwrap();
        let draw = |fa: &mut Kit, span: Span| {
            let spans = [Span::new("Hamburge "), span];
            let mut text = TextLayout::new("", 32.0);
            text.family = Some("Lora");
            text.spans = &spans;
            pixels(fa, &text)
        };
        let plain = draw(&mut fa, Span::new("fonstiv"));
        let bold = draw(&mut fa, Span::new("fonstiv").bold());
        assert!(ink(&bold) > ink(&plain), "bold has more ink");
        let italic = draw(&mut fa, Span::new("fonstiv").italic());
        assert_ne!(italic.rgba_pixels, plain.rgba_pixels, "italic");
        let large = draw(&mut fa, Span::new("fonstiv").with_size(48.0));
        assert!(
            large.height > plain.height && large.width > plain.width,
            "larger"
        );
        let inter = draw(&mut fa, Span::new("fonstiv").with_family("Inter"));
        assert_ne!(inter.rgba_pixels, plain.rgba_pixels, "in another family");

        // The span's text before it is unchanged: its first columns match.
        let left = |g: &RasterizedGlyphs| -> Vec<u8> {
            (0..g.height.min(plain.height))
                .flat_map(|y| {
                    let row = (y * g.width * 4) as usize;
                    g.rgba_pixels[row..row + 40 * 4].to_vec()
                })
                .collect()
        };
        assert_eq!(left(&bold), left(&plain), "the first span is unchanged");
    }

    // Spans of different sizes share a line, which is as tall as the largest
    // needs.
    #[test]
    fn spans_of_different_sizes_share_a_line() {
        let mut fa = atlas();
        let spans = [Span::new("small "), Span::new("Large").with_size(48.0)];
        let mut text = TextLayout::new("", 16.0);
        text.spans = &spans;
        assert_eq!(fa.fit(&text, 1.0).line_count, 1);
        let small = pixels(&mut fa, &TextLayout::new("small Large", 16.0));
        assert!(pixels(&mut fa, &text).height > small.height * 2);
    }

    // In a text with a colored span, each span is drawn in its own color, and
    // one that sets none in the text's color. A text with no colored span is
    // white, for the renderer to tint.
    #[test]
    fn a_colored_span_is_drawn_in_its_color_and_the_rest_in_the_texts() {
        let mut fa = atlas();
        let spans = [Span::new("ab "), Span::new("cd").with_color(RED)];
        let mut text = TextLayout::new("", 32.0);
        text.spans = &spans;
        text.color = BLUE.with_w(0.5);
        assert!(text.is_colored());
        let g = pixels(&mut fa, &text);
        assert_eq!(colors(&g), vec![[0, 0, 255], [255, 0, 0]]);
        // The text's own alpha is left to the renderer, so its spans are
        // baked opaque.
        assert_eq!(g.rgba_pixels.chunks(4).map(|p| p[3]).max(), Some(255));

        let plain = [Span::new("ab "), Span::new("cd")];
        text.spans = &plain;
        assert!(!text.is_colored());
        assert_eq!(colors(&pixels(&mut fa, &text)), vec![[255, 255, 255]]);
    }

    // A span's alpha multiplies its coverage, so a span at half alpha is
    // baked at half the alpha of an opaque one.
    #[test]
    fn a_spans_alpha_multiplies_its_coverage() {
        let mut fa = atlas();
        let draw = |fa: &mut Kit, alpha: f32| {
            let spans = [Span::new("Hamburgefonstiv").with_color(RED.with_w(alpha))];
            let mut text = TextLayout::new("", 32.0);
            text.spans = &spans;
            pixels(fa, &text)
        };
        let opaque = draw(&mut fa, 1.0);
        let half = draw(&mut fa, 0.5);
        // Alpha 0.5 is 128 of 255.
        for (o, h) in opaque.rgba_pixels.chunks(4).zip(half.rgba_pixels.chunks(4)) {
            let expected = (o[3] as u32 * 128 + 127) / 255;
            assert_eq!(h[3] as u32, expected, "{o:?} {h:?}");
        }
    }

    // The text is cut inside a span, and the spans before the cut keep their
    // style.
    #[test]
    fn an_ellipsis_can_cut_inside_a_span() {
        let mut fa = atlas();
        let spans = [Span::new("Short. "), Span::new(PARAGRAPH).with_color(RED)];
        let mut text = TextLayout::new("", 16.0);
        text.spans = &spans;
        text.wrap_width = Some(200.0);
        text.max_lines = Some(2);
        let fitted = fa.fit(&text, 1.0);
        assert!(
            fitted.content.starts_with("Short. Proteus"),
            "{:?}",
            fitted.content
        );
        assert!(fitted.content.ends_with('\u{2026}'), "{:?}", fitted.content);
        assert_eq!(fitted.line_count, 2);
        let g = pixels(&mut fa, &text);
        assert!(
            colors(&g).contains(&[255, 0, 0]),
            "the cut span keeps its color"
        );
    }

    // An ellipsis of spans is drawn in their style. A plain one, such as the
    // default "…", is in the text's own style, not the style of the span it
    // follows.
    #[test]
    fn an_ellipsis_of_spans_is_drawn_in_their_style() {
        let mut fa = atlas();
        let spans = [Span::new(PARAGRAPH).with_color(RED)];
        let mut text = TextLayout::new("", 16.0);
        text.spans = &spans;
        text.color = BLUE;
        text.wrap_width = Some(200.0);
        text.max_lines = Some(1);
        // Returns the color of the last glyph laid out, which is the
        // ellipsis's.
        let last_color = |fa: &mut Kit, text: &TextLayout| {
            let fitted = fa.fit(text, 1.0);
            let line = fitted.layout.lines().next().unwrap();
            line.items()
                .filter_map(|item| match item {
                    PositionedLayoutItem::GlyphRun(run) => Some(run.style().brush),
                    _ => None,
                })
                .last()
                .unwrap()
        };
        assert_eq!(
            last_color(&mut fa, &text),
            [0, 0, 255, 255],
            "the text's color"
        );

        let more = [Span::new(" more")
            .with_color(Vec4::new(0.0, 1.0, 0.0, 1.0))
            .bold()];
        text.ellipsis = Some(&more);
        assert_eq!(
            last_color(&mut fa, &text),
            [0, 255, 0, 255],
            "the ellipsis's color"
        );
        let content = fa.fit(&text, 1.0).content;
        assert!(content.ends_with(" more"), "{content:?}");

        // An ellipsis's colored span makes the text colored on its own.
        let plain = [Span::new(PARAGRAPH)];
        text.spans = &plain;
        assert!(text.is_colored());
    }

    // The families a text names include its spans' and its ellipsis's, each
    // once, for knowing which fonts it waits for.
    #[test]
    fn a_texts_families_include_its_spans() {
        let spans = [
            Span::new("a").with_family("Lora"),
            Span::new("b").with_family("Inter"),
            Span::new("c").with_family("Lora"),
        ];
        let icons = [Span::new("arrow").with_family("Material Icons")];
        let mut text = TextLayout::new("", 16.0);
        text.family = Some("Inter");
        text.spans = &spans;
        text.ellipsis = Some(&icons);
        assert_eq!(text.families(), vec!["Inter", "Lora", "Material Icons"]);
    }

    // Returns how far `span`, drawn in red after a blue "x", sits above the
    // "x", in pixels: the rows between the bottoms of their ink. Both are "x",
    // so on the same baseline they're level.
    fn raised_by(fa: &mut Kit, align: VerticalAlign, scale: f32) -> i32 {
        let spans = [
            Span::new("x").with_color(BLUE),
            Span::new("x").with_color(RED).with_vertical_align(align),
        ];
        let mut text = TextLayout::new("", 24.0);
        text.spans = &spans;
        let g = fa.rasterize(&text, scale).unwrap();
        let bottom = |rgb: [u8; 3]| {
            g.rgba_pixels
                .chunks(4)
                .enumerate()
                .filter(|(_, p)| p[3] > 128 && p[..3] == rgb)
                .map(|(i, _)| i as i32 / g.width as i32)
                .max()
                .unwrap()
        };
        bottom([0, 0, 255]) - bottom([255, 0, 0])
    }

    // Each vertical alignment moves the span: up for superscript and a
    // positive raise, down for subscript, and to the x-height's middle for
    // `Middle`. A raise is in logical pixels, so it's twice as many pixels at a
    // display scale of 2.
    #[test]
    fn a_spans_vertical_alignment_moves_it() {
        let mut fa = atlas();
        assert_eq!(raised_by(&mut fa, VerticalAlign::Baseline, 1.0), 0);
        assert!(raised_by(&mut fa, VerticalAlign::Super, 1.0) > 0, "super");
        assert!(raised_by(&mut fa, VerticalAlign::Sub, 1.0) < 0, "sub");
        assert_ne!(raised_by(&mut fa, VerticalAlign::Middle, 1.0), 0, "middle");
        assert_eq!(raised_by(&mut fa, VerticalAlign::Raise(4.0), 1.0), 4);
        assert_eq!(raised_by(&mut fa, VerticalAlign::Raise(-4.0), 1.0), -4);
        assert_eq!(raised_by(&mut fa, VerticalAlign::Raise(4.0), 2.0), 8);
    }

    // A span moved up or down makes its line taller, so it never overlaps the
    // lines around it.
    #[test]
    fn a_moved_span_makes_its_line_taller() {
        let mut fa = atlas();
        let height = |fa: &mut Kit, align: VerticalAlign| {
            let spans = [
                Span::new("Hamburge"),
                Span::new("fonstiv").with_vertical_align(align),
            ];
            let mut text = TextLayout::new("", 24.0);
            text.spans = &spans;
            pixels(fa, &text).height
        };
        let level = height(&mut fa, VerticalAlign::Baseline);
        assert!(height(&mut fa, VerticalAlign::Raise(10.0)) >= level + 10);
        assert!(height(&mut fa, VerticalAlign::Raise(-10.0)) >= level + 10);
        assert!(height(&mut fa, VerticalAlign::Super) > level);
    }

    // ---------------------------------------------------------------------
    // Components in text
    // ---------------------------------------------------------------------

    // An entity for a span to hold.
    fn entity() -> Entity {
        bevy_ecs::world::World::new().spawn_empty().id()
    }

    // Returns `spans` as a text at 24 pixels.
    fn rich(spans: &[Span]) -> TextLayout<'_> {
        let mut text = TextLayout::new("", 24.0);
        text.spans = spans;
        text
    }

    // A component's gap is the size asked for, between the text before it and
    // the text after it, with its bottom on the baseline by default. Every
    // rasterize here also checks that measuring places it the same.
    #[test]
    fn a_component_keeps_a_gap_its_size_in_the_line() {
        let mut fa = atlas();
        let icon = entity();
        let ab = fa.rasterize(&rich(&[Span::new("AB")]), 1.0).unwrap();
        let spans = [
            Span::new("A"),
            Span::component(icon, Vec2::new(30.0, 10.0)),
            Span::new("B"),
        ];
        let g = fa.rasterize(&rich(&spans), 1.0).unwrap();
        assert!(
            g.width >= ab.width + 28 && g.width <= ab.width + 32,
            "{} with the gap, {} without",
            g.width,
            ab.width
        );
        let [placed] = g.components[..] else {
            panic!("one component: {:?}", g.components);
        };
        assert_eq!(placed.entity, icon);
        assert_eq!(placed.size, Vec2::new(30.0, 10.0));
        assert!(placed.position.x > 5.0 && placed.position.x + 30.0 < g.width as f32);
        // The gap's bottom is on the baseline, where the "B" sits.
        let baseline = ab.height as f32 - 6.0;
        assert!(
            (placed.position.y + 10.0 - baseline).abs() < 2.0,
            "{placed:?} in an image {} tall",
            g.height
        );
        assert!(g.missing.is_empty(), "the gap isn't a missing character");
    }

    // A gap wraps as a word does, onto the next line.
    #[test]
    fn a_component_wraps_with_the_text() {
        let mut fa = atlas();
        let spans = [
            Span::new("Words before the gap "),
            Span::component(entity(), Vec2::new(20.0, 20.0)),
        ];
        let mut text = rich(&spans);
        let one_line = fa.rasterize(&text, 1.0).unwrap().components[0];
        text.wrap_width = Some(150.0);
        let wrapped = fa.rasterize(&text, 1.0).unwrap().components[0];
        assert!(
            wrapped.position.y > one_line.position.y + 15.0,
            "{wrapped:?}"
        );
        assert!(wrapped.position.x < one_line.position.x, "{wrapped:?}");
    }

    // A gap at the end of a line widens the image, as ink does, and a text that
    // is only a gap is the gap's size across.
    #[test]
    fn a_component_widens_the_image_at_the_end_of_a_line() {
        let mut fa = atlas();
        let hi = fa.rasterize(&rich(&[Span::new("Hi")]), 1.0).unwrap();
        let spans = [
            Span::new("Hi"),
            Span::component(entity(), Vec2::new(50.0, 10.0)),
        ];
        let g = fa.rasterize(&rich(&spans), 1.0).unwrap();
        assert!(g.width >= hi.width + 48, "{} and {}", g.width, hi.width);
        let alone = [Span::component(entity(), Vec2::new(40.0, 12.0))];
        let g = fa.rasterize(&rich(&alone), 1.0).unwrap();
        assert_eq!(g.width, 40);
    }

    // A gap taller than its line makes the line taller, so it never overlaps
    // the lines around it.
    #[test]
    fn a_tall_component_makes_its_line_taller() {
        let mut fa = atlas();
        let line = fa.rasterize(&rich(&[Span::new("Line")]), 1.0).unwrap();
        let spans = [
            Span::new("Line"),
            Span::component(entity(), Vec2::new(20.0, 80.0)),
        ];
        let g = fa.rasterize(&rich(&spans), 1.0).unwrap();
        assert!(g.height >= 80 && g.height > line.height, "{}", g.height);
        let placed = g.components[0];
        assert!(placed.position.y >= 0.0, "inside the image: {placed:?}");
    }

    // Returns how far up `align` moves a 10 × 10 gap from the baseline, in
    // pixels, at `scale`.
    fn gap_raised_by(fa: &mut Kit, align: VerticalAlign, scale: f32) -> f32 {
        let top = |fa: &mut Kit, align| {
            let spans = [
                Span::new("x"),
                Span::component(entity(), Vec2::new(10.0, 10.0)).with_vertical_align(align),
            ];
            let g = fa.rasterize(&rich(&spans), scale).unwrap();
            // From the bottom of the image, so a taller line doesn't move it.
            g.height as f32 - g.components[0].position.y
        };
        top(fa, align) - top(fa, VerticalAlign::Baseline)
    }

    // A span's vertical alignment moves its gap as it moves text, and a raise
    // is in logical pixels, so it's twice as many pixels at a scale of 2.
    #[test]
    fn a_components_vertical_alignment_moves_its_gap() {
        let mut fa = atlas();
        assert!(
            gap_raised_by(&mut fa, VerticalAlign::Super, 1.0) > 0.0,
            "super"
        );
        assert!(gap_raised_by(&mut fa, VerticalAlign::Sub, 1.0) < 0.0, "sub");
        assert_ne!(
            gap_raised_by(&mut fa, VerticalAlign::Middle, 1.0),
            0.0,
            "middle"
        );
        let raise = |fa: &mut Kit, scale| gap_raised_by(fa, VerticalAlign::Raise(4.0), scale);
        assert!(
            (raise(&mut fa, 1.0) - 4.0).abs() < 1.0,
            "{}",
            raise(&mut fa, 1.0)
        );
        assert!(
            (raise(&mut fa, 2.0) - 8.0).abs() < 1.0,
            "{}",
            raise(&mut fa, 2.0)
        );
    }

    // A gap in a line past the maximum isn't placed, and a gap in an ellipsis
    // is placed only when the ellipsis is shown.
    #[test]
    fn a_component_cut_off_or_in_an_unshown_ellipsis_isnt_placed() {
        let mut fa = atlas();
        let (inline, in_ellipsis) = (entity(), entity());
        let spans = [
            Span::new("One\nTwo "),
            Span::component(inline, Vec2::new(10.0, 10.0)),
        ];
        let ellipsis = [
            Span::new(" more "),
            Span::component(in_ellipsis, Vec2::new(12.0, 12.0)),
        ];
        let mut text = rich(&spans);
        text.ellipsis = Some(&ellipsis);
        let placed = |fa: &mut Kit, text: &TextLayout| -> Vec<Entity> {
            let g = fa.rasterize(text, 1.0).unwrap();
            g.components.iter().map(|c| c.entity).collect()
        };
        assert_eq!(placed(&mut fa, &text), [inline], "fits: no ellipsis");
        text.max_lines = Some(1);
        assert_eq!(placed(&mut fa, &text), [in_ellipsis], "cut: the ellipsis's");
        text.ellipsis = None;
        assert_eq!(placed(&mut fa, &text), [], "clipped");
    }

    // ---------------------------------------------------------------------
    // Text box edges
    // ---------------------------------------------------------------------

    // Returns `content` at `size_px` with `text_box`, rasterized at `scale`.
    fn boxed(
        fa: &mut Kit,
        content: &str,
        size_px: f32,
        text_box: TextBox,
        scale: f32,
    ) -> RasterizedGlyphs {
        let mut text = TextLayout::new(content, size_px);
        text.text_box = text_box;
        fa.rasterize(&text, scale).unwrap()
    }

    // Returns the first and last rows with ink, and the row with the most.
    fn ink_rows(g: &RasterizedGlyphs) -> (u32, u32, u32) {
        let row = |y: u32| -> u32 {
            (0..g.width)
                .map(|x| g.rgba_pixels[((y * g.width + x) * 4 + 3) as usize] as u32)
                .sum()
        };
        let inked: Vec<u32> = (0..g.height).filter(|&y| row(y) > 0).collect();
        let densest = (0..g.height).max_by_key(|&y| row(y)).unwrap();
        (inked[0], *inked.last().unwrap(), densest)
    }

    // The line box is the whole image, as before.
    #[test]
    fn a_line_box_is_the_whole_image() {
        let mut fa = atlas();
        for scale in [1.0, 2.0] {
            let g = boxed(&mut fa, "Hxp", 24.0, TextBox::Line, scale);
            assert_eq!((g.box_top, g.box_height), (0, g.height));
        }
    }

    // A box to the cap height hugs the capitals, and one to the x-height the
    // lowercase letters: from their tops to the baseline, at every scale. The
    // image is the same whatever the box.
    #[test]
    fn a_trimmed_box_hugs_its_letters() {
        let mut fa = atlas();
        for scale in [1.0, 2.0] {
            let line = boxed(&mut fa, "HH", 24.0, TextBox::Line, scale);
            for (content, text_box) in [("HH", TextBox::CapHeight), ("xx", TextBox::XHeight)] {
                let g = boxed(&mut fa, content, 24.0, text_box, scale);
                let (top, bottom, _) = ink_rows(&g);
                assert!(
                    top.abs_diff(g.box_top) <= 1,
                    "{content} at {scale}: ink from {top}, box from {}",
                    g.box_top
                );
                assert!(
                    (bottom + 1).abs_diff(g.box_top + g.box_height) <= 1,
                    "{content} at {scale}: ink to {bottom}, box to {}",
                    g.box_top + g.box_height
                );
                assert_eq!(g.height, line.height, "the image isn't trimmed");
            }
        }
    }

    // A box around several lines ends at the last line's baseline.
    #[test]
    fn a_trimmed_box_ends_at_the_last_lines_baseline() {
        let mut fa = atlas();
        let one = boxed(&mut fa, "HH", 24.0, TextBox::CapHeight, 1.0);
        let two = boxed(&mut fa, "HH\nHH", 24.0, TextBox::CapHeight, 1.0);
        let lines = boxed(&mut fa, "HH\nHH", 24.0, TextBox::Line, 1.0).height
            - boxed(&mut fa, "HH", 24.0, TextBox::Line, 1.0).height;
        assert_eq!(two.box_top, one.box_top);
        assert!(
            (two.box_height).abs_diff(one.box_height + lines) <= 1,
            "{} for two lines, {} for one, lines {lines} apart",
            two.box_height,
            one.box_height
        );
    }

    // A line in several sizes takes the tallest one's cap height, so its
    // letters are never cut short.
    #[test]
    fn a_mixed_line_uses_its_tallest_height() {
        let mut fa = atlas();
        let big = boxed(&mut fa, "B", 30.0, TextBox::CapHeight, 1.0);
        let spans = [
            Span::new("a").with_size(12.0),
            Span::new("B").with_size(30.0),
        ];
        let mut mixed = TextLayout::new("", 16.0);
        mixed.spans = &spans;
        mixed.text_box = TextBox::CapHeight;
        let g = fa.rasterize(&mixed, 1.0).unwrap();
        assert_eq!(g.box_height, big.box_height);
    }

    // Text drawn in a fallback font is boxed by that font's measurements, not
    // its own family's. Noto Serif Devanagari's x-height is its headline, the
    // bar its letters hang from, which is the densest row of ink.
    #[test]
    fn fallback_text_is_boxed_by_the_font_that_draws_it() {
        let mut fa = atlas();
        fa.load_font(DEVANAGARI).unwrap();
        fa.set_fallbacks(&["Noto Serif Devanagari".to_owned()]);
        let hindi = boxed(&mut fa, "आगे पढ़ें", 12.0, TextBox::XHeight, 2.0);
        let latin = boxed(&mut fa, "xx", 12.0, TextBox::XHeight, 2.0);
        assert!(
            hindi.box_height > latin.box_height,
            "Noto's x-height is taller than Inter's"
        );
        let (_, _, headline) = ink_rows(&hindi);
        assert!(
            headline.abs_diff(hindi.box_top) <= 1,
            "headline at {headline}, box from {}",
            hindi.box_top
        );
    }
}
