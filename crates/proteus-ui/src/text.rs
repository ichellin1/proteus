//! Text drawn on an entity, in one line or, where it has a `\n`, more.
//!
//! ```text
//! Text { content, size_px }      declared on an entity
//!         │  the renderer finds Text without BakedText
//!         ▼
//! TextRasterizer::rasterize      laid out, shaped and rasterized
//!         │  added to the main atlas
//!         ▼
//! BakedText + TextTextureRef     where the text is in the atlas
//!         │
//!         ▼
//! drawn as a quad over the entity's background
//! ```
//!
//! Glyphs are rasterized white, with their coverage in the alpha channel, and
//! drawn in `Text::color`, so the color can change at no cost. A text with a
//! colored [`Span`] is rasterized in its colors instead, since one image takes
//! one tint, and only the text's alpha applies as it's drawn. Changing such a
//! text's color bakes it again. See [`Text::color`].
//!
//! During a transition, the text moves and scales with the entity like any
//! other texture. A label is usually a child entity of the component it
//! labels, positioned relative to it.
//!
//! ## Measuring
//!
//! [`measure_text`] gives a text's size before it's baked: the size its
//! [`BakedText`] will have. The [`Typesetter`] in [`Fonts`] lays it out, with
//! the fonts it's drawn in.
//!
//! ## Text larger than an atlas page
//!
//! Text has no length limit, and is one line or, where it has a `\n`, more.
//! Text wider or taller than an atlas page ([`AtlasConfig::page_size`], 2048
//! pixels by default) is clipped to the page, with a warning. The part that
//! fits is drawn at its normal size, and the rest is cut off.
//!
//! [`AtlasConfig::page_size`]: proteus_render::AtlasConfig::page_size

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec4};
use proteus_render::{FontError, QuadPipeline, TextLayout, Typesetter};
pub use proteus_render::{Span, TextAlign, TextOverflow, VerticalAlign};

use crate::bake::{bake_scale_to_fit, BakeScale};

// ---------------------------------------------------------------------------
// Text component
// ---------------------------------------------------------------------------

/// Text to draw on an entity, in one line or, where it has a `\n`, more.
///
/// The renderer rasterizes it into the main atlas and adds a [`BakedText`].
/// Changing a field afterwards has no effect until the `BakedText` is
/// removed, apart from [`Text::color`]. `proteus-sdk`'s `Handle::set_text` does
/// both. Text larger than an atlas page is clipped. See the
/// [module docs](self).
///
/// A text's lines can wrap at a width, align within that width, take a line
/// height, and stop at a number of lines, either with an ellipsis or clipped.
/// This example uses all of them:
///
/// ```
/// # use proteus_ui::{Text, TextAlign};
/// let summary = Text::new("A long summary of the article…", 16.0)
///     .with_wrap_width(280.0)
///     .with_align(TextAlign::Center)
///     .with_line_height(1.4)
///     .with_max_lines(3)
///     .with_ellipsis(" more");
/// ```
///
/// A text can mix styles, as [`Span`]s. Each span takes whatever it doesn't
/// set from the text:
///
/// ```
/// # use glam::Vec4;
/// # use proteus_ui::{Span, Text};
/// let muted = Vec4::new(0.4, 0.4, 0.45, 1.0);
/// let byline = Text::rich(
///     [
///         Span::new("Mary Wollstonecraft").with_weight(600),
///         Span::new("\n1792").with_size(12.0).with_color(muted),
///     ],
///     14.0,
/// );
/// ```
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Text {
    /// The text. Any character the font has can be used. A text with
    /// [`Text::spans`] draws them instead, and leaves this empty.
    pub content: String,
    /// The text's spans, in order, each a part of it with a style of its own.
    /// Empty, the default, draws [`Text::content`]. A text with both draws the
    /// spans, with a warning.
    pub spans: Vec<Span>,
    /// Font size in pixels.
    pub size_px: f32,
    /// The text's color, RGBA. Defaults to opaque white. Its alpha is
    /// multiplied by the entity's opacity.
    ///
    /// It's applied as the text is drawn, so it can change at no cost. A text
    /// with a colored span is baked in its colors instead, with the spans that
    /// set none in this color, and only its alpha is applied as it's drawn.
    /// Changing its red, green or blue bakes it again, which costs about as
    /// much as setting the text again. Fading it with alpha or opacity costs
    /// nothing.
    pub color: Vec4,
    /// Extra space between characters, in pixels. `0.0`, the default, is the
    /// font's normal spacing; negative values tighten it.
    pub letter_spacing_px: f32,
    /// The width lines wrap at, in logical pixels. This is also the width of
    /// the text's box, and lines align within the box. `None`, the default,
    /// breaks lines only at `\n`, and the box is as wide as the widest line. A
    /// word wider than the wrap width breaks between characters.
    pub wrap_width: Option<f32>,
    /// Where lines sit across the text's box. The default is
    /// [`TextAlign::Start`]. The text's anchor places the box on the entity.
    pub align: TextAlign,
    /// The distance from one line's baseline to the next, as a multiple of
    /// the size, like CSS's unitless `line-height`. `None`, the default, uses
    /// the font's own line spacing.
    pub line_height: Option<f32>,
    /// The most lines shown. `None`, the default, shows them all.
    pub max_lines: Option<u32>,
    /// How text with more lines than `max_lines` ends. It ends with an
    /// ellipsis, "…" by default, or is clipped. An ellipsis can be spans. One
    /// given as a string is in the text's own style. An ellipsis wraps as words do,
    /// and the text keeps as much of itself as fits with the ellipsis. An
    /// ellipsis too long to fit in `max_lines` on its own is clipped, with a
    /// warning.
    pub overflow: TextOverflow,
    /// The font family, such as `"Lora"`, of a font the app has loaded.
    /// `None`, the default, is the app's default font. A family the app has
    /// said it will load waits, or is drawn in the default font meanwhile, as
    /// [`Fonts::expect`] says. A family that was never loaded is drawn in the
    /// default font, with a warning.
    pub family: Option<String>,
    /// The weight, from 100 to 900, where 400 is regular and 700 is bold. A
    /// family without that weight uses the nearest weight it has, and a family
    /// with no bold face is thickened. `None`, the default, uses the font's own
    /// weight, or 700, bold, in the embedded font.
    pub weight: Option<u16>,
    /// Whether the text is italic. It uses the family's italic face, or is
    /// slanted if the family has none. The default is false.
    pub italic: bool,
}

impl Text {
    /// Creates text in opaque white with normal letter spacing.
    pub fn new(content: impl Into<String>, size_px: f32) -> Self {
        Self {
            content: content.into(),
            spans: Vec::new(),
            size_px,
            color: Vec4::ONE,
            letter_spacing_px: 0.0,
            wrap_width: None,
            align: TextAlign::Start,
            line_height: None,
            max_lines: None,
            overflow: TextOverflow::default(),
            family: None,
            weight: None,
            italic: false,
        }
    }

    /// Creates text in opaque white made of `spans`, at `size_px` where a span
    /// sets no size of its own.
    pub fn rich(spans: impl IntoIterator<Item = Span>, size_px: f32) -> Self {
        Self {
            spans: spans.into_iter().collect(),
            ..Self::new("", size_px)
        }
    }

    /// Wraps lines at `width` logical pixels, and makes `width` the width of
    /// the text's box. See [`Text::wrap_width`].
    pub fn with_wrap_width(mut self, width: f32) -> Self {
        self.wrap_width = Some(width);
        self
    }

    /// Sets where lines sit across the text's box.
    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Sets the distance between baselines, as a multiple of the size, so
    /// `1.5` is one and a half times the size.
    pub fn with_line_height(mut self, multiple: f32) -> Self {
        self.line_height = Some(multiple);
        self
    }

    /// Shows at most `lines` lines. Text with more lines ends as
    /// [`Text::overflow`] says, in an ellipsis, "…" by default, or clipped.
    pub fn with_max_lines(mut self, lines: u32) -> Self {
        self.max_lines = Some(lines);
        self
    }

    /// Ends the text in `ellipsis`, such as `" more"`, rather than "…", if it
    /// has more lines than its maximum. The ellipsis is in the text's own
    /// style.
    pub fn with_ellipsis(mut self, ellipsis: impl Into<String>) -> Self {
        self.overflow = TextOverflow::Ellipsis(vec![Span::new(ellipsis)]);
        self
    }

    /// Ends the text in `spans`, such as a link-colored `" more"`, if it has
    /// more lines than its maximum.
    pub fn with_ellipsis_spans(mut self, spans: impl IntoIterator<Item = Span>) -> Self {
        self.overflow = TextOverflow::Ellipsis(spans.into_iter().collect());
        self
    }

    /// Sets how text with more lines than its maximum ends.
    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Sets the font family, such as `"Lora"`. See [`Text::family`].
    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }

    /// Sets the weight, from 100 to 900, where 400 is regular and 700 is bold.
    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Makes the text italic. See [`Text::italic`].
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Returns how the renderer lays this text out.
    pub fn layout(&self) -> TextLayout<'_> {
        TextLayout {
            content: &self.content,
            spans: &self.spans,
            color: self.color,
            size_px: self.size_px,
            letter_spacing_px: self.letter_spacing_px,
            wrap_width: self.wrap_width,
            align: self.align,
            line_height: self.line_height,
            max_lines: self.max_lines,
            ellipsis: match &self.overflow {
                TextOverflow::Ellipsis(spans) => Some(spans),
                TextOverflow::Clip => None,
            },
            family: self.family.as_deref(),
            weight: self.weight,
            italic: self.italic,
        }
    }

    /// Returns whether a span of the text or of its ellipsis sets a color, so
    /// the text is baked in its colors. See [`Text::color`].
    pub fn is_colored(&self) -> bool {
        self.layout().is_colored()
    }

    /// Returns the color the text's image is tinted with as it's drawn. That's
    /// its color, or, for a text baked in its colors, white at its alpha.
    pub fn tint(&self) -> Vec4 {
        if self.is_colored() {
            Vec4::ONE.with_w(self.color.w)
        } else {
            self.color
        }
    }

    /// Returns whether the text, one of its spans or its ellipsis names
    /// `family`.
    pub fn uses_family(&self, family: &str) -> bool {
        self.layout().families().contains(&family)
    }

    /// Sets the text's color.
    ///
    /// ```
    /// # use glam::Vec4;
    /// # use proteus_ui::Text;
    /// let label = Text::new("Hello", 22.0).with_color(Vec4::new(0.1, 0.1, 0.1, 1.0));
    /// ```
    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }

    /// Sets the extra space between characters, in pixels.
    ///
    /// ```
    /// # use proteus_ui::Text;
    /// // Letter spacing of 0.06em is 0.06 times the size.
    /// let title = Text::new("PROTEUS", 90.0).with_letter_spacing(90.0 * 0.06);
    /// ```
    pub fn with_letter_spacing(mut self, letter_spacing_px: f32) -> Self {
        self.letter_spacing_px = letter_spacing_px;
        self
    }
}

// ---------------------------------------------------------------------------
// BakedText component
// ---------------------------------------------------------------------------

/// Where an entity's [`Text`] is in the main atlas, added once the text is
/// baked.
///
/// The text is drawn as its own [`QuadInstance`] at `pixel_size`, centered on
/// the entity, rather than stretched to the entity's size.
///
/// [`QuadInstance`]: proteus_render::QuadInstance
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BakedText {
    /// Texture coordinates of the text's top-left corner in the atlas.
    pub uv_offset: [f32; 2],
    /// The text's size in texture coordinates; `uv_offset + uv_scale` is its
    /// bottom-right corner.
    pub uv_scale: [f32; 2],
    /// The main-atlas page the text is on.
    pub page: u32,
    /// The text's size in pixels, which is the size it is drawn at.
    pub pixel_size: [f32; 2],
}

/// The color a text with a colored span was baked with, added beside its
/// [`BakedText`]. When the text's red, green or blue no longer match, the
/// renderer bakes it again, since its spans that set no color are in it. See
/// [`Text::color`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BakedColor(pub Vec3);

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

/// How a text in a font that's still loading is drawn meanwhile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WhileLoading {
    /// Not drawn at all until the font loads, or fails and the default font is
    /// used. This is the default, since a text that changes font once it's
    /// drawn looks like a mistake.
    #[default]
    Hide,
    /// Drawn in the default font, and drawn again in the text's own font once
    /// that loads. This suits an app that would rather show something at once.
    DefaultFont,
}

/// What's known of a font family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStatus {
    /// The app said it will load a font of the family. Its texts are drawn as
    /// the [`WhileLoading`] value says.
    Expected(WhileLoading),
    /// A font of the family has loaded. Its texts are drawn in it.
    Loaded,
    /// The load failed. Texts in the family are drawn in the default font.
    Failed,
}

/// The fonts an app has loaded, and what's known of those it said it will
/// load, by family. It holds the [`Typesetter`], so the app measures text with
/// the fonts the renderer draws it in.
///
/// `proteus-sdk`'s `Proteus::load_font`, `expect_font`, `font_failed` and
/// `set_fallback_fonts` change it, and the renderer sets the default font from
/// its config when it starts. A font loaded is in the typesetter at once. The
/// renderer then bakes again the texts that change.
#[derive(Resource, Debug, Default)]
pub struct Fonts {
    typesetter: Typesetter,
    status: std::collections::HashMap<String, FontStatus>,
    /// The families loaded since the renderer last looked, so it can bake
    /// their texts again.
    loaded: Vec<String>,
    /// The families a warning has been logged for.
    warned: std::collections::HashSet<String>,
    /// Whether the fallback families changed since the renderer last looked.
    fallbacks_changed: bool,
    /// Counts every change. The renderer uses it to tell when texts that wait
    /// for a font should try again.
    version: u64,
}

impl Fonts {
    /// Says a font of `family` will be loaded, and how its texts are drawn
    /// until it is. Does nothing to a family already loaded.
    pub fn expect(&mut self, family: impl Into<String>, while_loading: WhileLoading) {
        let family = family.into();
        if self.status.get(&family) != Some(&FontStatus::Loaded) {
            self.status
                .insert(family, FontStatus::Expected(while_loading));
            self.version += 1;
        }
    }

    /// Adds the TTF or OTF font in `bytes` to the typesetter, and returns the
    /// family name inside it. Texts in the family can be measured at once, and
    /// the renderer bakes them again in it.
    ///
    /// # Errors
    ///
    /// [`FontError`] if `bytes` isn't a TTF or OTF font that can be read.
    /// Nothing changes then.
    pub fn load(&mut self, bytes: &[u8]) -> Result<String, FontError> {
        let family = self.typesetter.load_font(bytes)?;
        self.status.insert(family.clone(), FontStatus::Loaded);
        self.loaded.push(family.clone());
        self.version += 1;
        Ok(family)
    }

    /// Says the font of `family` failed to load, so its texts are drawn in
    /// the default font. Does nothing to a family already loaded.
    pub fn failed(&mut self, family: impl Into<String>) {
        let family = family.into();
        if self.status.get(&family) != Some(&FontStatus::Loaded) {
            self.status.insert(family, FontStatus::Failed);
            self.version += 1;
        }
    }

    /// Sets the fallback families. They're tried in order for a character a
    /// text's own family lacks, before the default font.
    pub fn set_fallbacks(&mut self, families: Vec<String>) {
        self.typesetter.set_fallbacks(&families);
        self.fallbacks_changed = true;
        self.version += 1;
    }

    /// Returns the fallback families, in the order they're tried.
    pub fn fallbacks(&self) -> &[String] {
        self.typesetter.fallbacks()
    }

    /// Returns whether the fallbacks changed since the last call. This is for
    /// the renderer, which bakes every text again when they do.
    pub fn take_fallbacks_changed(&mut self) -> bool {
        std::mem::take(&mut self.fallbacks_changed)
    }

    /// Returns a number that changes whenever a font is expected, loaded or
    /// failed, or the fallbacks change.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns what's known of `family`, or `None` if it was never expected,
    /// loaded or failed.
    pub fn status(&self, family: &str) -> Option<FontStatus> {
        self.status.get(family).copied()
    }

    /// Takes the families loaded since the last call. This is for the
    /// renderer, which bakes their texts again.
    pub fn take_loaded(&mut self) -> Vec<String> {
        std::mem::take(&mut self.loaded)
    }

    /// Returns the typesetter, which lays text out in the fonts.
    pub fn typesetter(&self) -> &Typesetter {
        &self.typesetter
    }

    /// Returns the typesetter, to lay text out, or to set the default font.
    pub fn typesetter_mut(&mut self) -> &mut Typesetter {
        &mut self.typesetter
    }

    /// Returns whether `text` waits, not drawn, because a family it, a span or
    /// its ellipsis names is expected with [`WhileLoading::Hide`] and not
    /// loaded yet. A family that failed, or was never expected or loaded, is
    /// drawn in the default font, and warned about once.
    pub fn text_waits(&mut self, text: &TextLayout) -> bool {
        // Every family is looked at, so each is warned about.
        let mut waits = false;
        for family in text.families() {
            waits |= self.waits_for(family);
        }
        waits
    }

    /// Returns whether a text in `family` waits for it to load. See
    /// [`Fonts::text_waits`].
    fn waits_for(&mut self, family: &str) -> bool {
        if self.typesetter.has_family(family) {
            return false;
        }
        match self.status(family) {
            Some(FontStatus::Expected(WhileLoading::Hide)) => true,
            Some(FontStatus::Expected(WhileLoading::DefaultFont) | FontStatus::Loaded) => false,
            Some(FontStatus::Failed) => {
                if self.first_warning(family) {
                    log::warn!(
                        "the font family {family:?} failed to load: its text is drawn in the \
                         default font"
                    );
                }
                false
            }
            None => {
                if self.first_warning(family) {
                    log::warn!(
                        "no font of the family {family:?} has been loaded, or is expected: its \
                         text is drawn in the default font. Load it with `load_font`, or say \
                         it's coming with `expect_font`"
                    );
                }
                false
            }
        }
    }

    /// Returns whether a fallback font is expected with [`WhileLoading::Hide`]
    /// and not loaded yet. Until it loads, a text with characters no loaded
    /// font has waits, since the fallback may have them.
    pub fn waits_for_fallback(&self) -> bool {
        self.fallbacks()
            .iter()
            .any(|family| self.status(family) == Some(FontStatus::Expected(WhileLoading::Hide)))
    }

    /// Returns whether this is the first time `family` is warned about, so
    /// each family's warning is logged once.
    fn first_warning(&mut self, family: &str) -> bool {
        self.warned.insert(family.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Measuring
// ---------------------------------------------------------------------------

/// Returns the size `text` is baked at, in logical pixels, without baking it:
/// the `pixel_size` its [`BakedText`] will have. `proteus-sdk`'s
/// `Proteus::measure_text` calls it.
///
/// It's measured at the world's [`BakeScale`] and rounded up to whole pixels
/// at that scale, as the bake rounds it, so at another scale it can differ by
/// under a pixel. A text too large for an atlas page measures as it bakes: at
/// the smaller scale it's baked at, or clipped to the page.
///
/// Returns `None` while the text waits for a font, as it isn't drawn yet: a
/// family it names, or a fallback that may have characters no loaded font has,
/// is expected with [`WhileLoading::Hide`]. With
/// [`WhileLoading::DefaultFont`], it's the size in the default font, as drawn
/// meanwhile. Also `None` with no font at all. An empty text is zero.
pub fn measure_text(world: &mut World, text: &Text) -> Option<Vec2> {
    let bake_scale = world.get_resource::<BakeScale>().map_or(1.0, |s| s.0);
    let max = page_side(world);
    let mut fonts = world.get_resource_or_insert_with(Fonts::default);
    let layout = text.layout();
    if fonts.text_waits(&layout) {
        return None;
    }
    fonts.typesetter().default_family()?;
    let (size, scale) = fit_to_page(
        bake_scale,
        max,
        |scale| fonts.typesetter_mut().measure(&layout, scale),
        |size| (size.width, size.height),
    );
    let Some(size) = size else {
        return Some(Vec2::ZERO);
    };
    if !size.missing.is_empty() && fonts.waits_for_fallback() {
        return None;
    }
    Some(Vec2::new(size.width.min(max) as f32, size.height.min(max) as f32) / scale)
}

/// Returns the side of a main-atlas page in pixels, or `u32::MAX` with no
/// renderer.
pub fn page_side(world: &World) -> u32 {
    world
        .get_resource::<QuadPipeline>()
        .map_or(u32::MAX, |p| p.texture_registry.max_texture_side())
}

/// Makes a text's image at `scale` with `make`, and returns it with the scale
/// it's made at. One too large for a page `max` pixels on a side is made again
/// at the largest scale that fits, if that's above 1; see
/// [`bake_scale_to_fit`]. Baking and measuring both use it, so they agree.
pub fn fit_to_page<T>(
    scale: f32,
    max: u32,
    mut make: impl FnMut(f32) -> Option<T>,
    size: impl Fn(&T) -> (u32, u32),
) -> (Option<T>, f32) {
    let made = make(scale);
    if let Some(made) = &made {
        let (width, height) = size(made);
        let fitted = bake_scale_to_fit(scale, width as f32 / scale, height as f32 / scale, max);
        if fitted < scale {
            return (make(fitted), fitted);
        }
    }
    (made, scale)
}

/// Marks a text drawn with boxes for characters no font had. The text is
/// baked again when a font loads or the fallbacks change.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MissingGlyphs;

/// Marks a text that waits, not drawn, for a fallback font that's expected
/// with [`WhileLoading::Hide`]. The text has characters that only a font still
/// to load might have. The renderer removes the marker when what's known of
/// the fonts changes.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WaitsForFont;

// ---------------------------------------------------------------------------
// ClearGlyphCache
// ---------------------------------------------------------------------------

/// Asks the renderer to empty its glyph cache before it next bakes text.
/// `proteus-sdk`'s `Proteus::clear_glyph_cache` adds it, and the renderer
/// removes it once the cache is empty.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct ClearGlyphCache;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::world::World;

    #[test]
    fn text_component_stores_content_and_size() {
        let t = Text::new("Hello", 24.0);
        assert_eq!(t.content, "Hello");
        assert!((t.size_px - 24.0).abs() < 1e-6);
    }

    #[test]
    fn text_color_defaults_to_white() {
        let t = Text::new("Hi", 16.0);
        assert_eq!(t.color, Vec4::ONE);
    }

    #[test]
    fn text_with_color_overrides_default() {
        let dark = Vec4::new(0.1, 0.1, 0.1, 1.0);
        let t = Text::new("Hi", 16.0).with_color(dark);
        assert_eq!(t.color, dark);
    }

    #[test]
    fn rich_text_is_its_spans() {
        let text = Text::rich([Span::new("Hello "), Span::new("world").bold()], 16.0);
        assert_eq!(text.content, "");
        assert_eq!(text.spans[1].weight, Some(700));
        assert_eq!(text.layout().spans.len(), 2);
    }

    // An ellipsis given as a string is one span that sets nothing, so it's in
    // the text's own style.
    #[test]
    fn an_ellipsis_is_a_string_or_spans() {
        let text = Text::new("Hi", 16.0).with_ellipsis(" more");
        assert_eq!(
            text.overflow,
            TextOverflow::Ellipsis(vec![Span::new(" more")])
        );
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let text = Text::new("Hi", 16.0).with_ellipsis_spans([Span::new(" more").with_color(red)]);
        assert!(text.is_colored(), "an ellipsis's color counts");
    }

    // A plain text is tinted in its color. A text with a colored span is
    // baked in its colors, so it's tinted white at its alpha.
    #[test]
    fn a_colored_text_is_tinted_white_at_its_alpha() {
        let blue = Vec4::new(0.0, 0.0, 1.0, 0.5);
        assert_eq!(Text::new("Hi", 16.0).with_color(blue).tint(), blue);
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let colored = Text::rich([Span::new("Hi").with_color(red)], 16.0).with_color(blue);
        assert_eq!(colored.tint(), Vec4::new(1.0, 1.0, 1.0, 0.5));
    }

    #[test]
    fn a_text_uses_its_spans_families() {
        let text = Text::rich([Span::new("a").with_family("Lora")], 16.0)
            .with_family("Inter")
            .with_ellipsis_spans([Span::new("b").with_family("Material Icons")]);
        assert!(text.uses_family("Inter"));
        assert!(text.uses_family("Lora"));
        assert!(text.uses_family("Material Icons"));
        assert!(!text.uses_family("Crimson Pro"));
    }

    #[test]
    fn text_component_round_trips_through_world() {
        let mut world = World::new();
        let e = world.spawn(Text::new("Proteus", 32.0)).id();
        let t = world.get::<Text>(e).unwrap();
        assert_eq!(t.content, "Proteus");
    }

    #[test]
    fn baked_text_stores_uv_coords() {
        let baked = BakedText {
            uv_offset: [0.1, 0.2],
            uv_scale: [0.3, 0.05],
            page: 1,
            pixel_size: [60.0, 24.0],
        };
        let mut world = World::new();
        let e = world.spawn(baked.clone()).id();
        let b = world.get::<BakedText>(e).unwrap();
        assert_eq!(b.uv_offset, [0.1, 0.2]);
        assert_eq!(b.uv_scale, [0.3, 0.05]);
        assert_eq!(b.page, 1);
    }

    #[test]
    fn entity_can_have_both_text_and_baked_text() {
        let mut world = World::new();
        let e = world
            .spawn((
                Text::new("Label", 16.0),
                BakedText {
                    uv_offset: [0.0, 0.0],
                    uv_scale: [0.1, 0.02],
                    page: 0,
                    pixel_size: [40.0, 16.0],
                },
            ))
            .id();
        assert!(world.get::<Text>(e).is_some());
        assert!(world.get::<BakedText>(e).is_some());
    }
}
