//! Text: a line of text drawn on an entity, or more where it has a `\n`.
//!
//! ```text
//! Text { content, size_px }      declared on an entity
//!         │  the renderer finds Text without BakedText
//!         ▼
//! FontAtlas::rasterize_text      laid out, shaped and rasterized
//!         │  added to the main atlas
//!         ▼
//! BakedText + TextTextureRef     where the text is in the atlas
//!         │
//!         ▼
//! drawn as a quad over the entity's background
//! ```
//!
//! Glyphs are rasterized white, with their coverage in the alpha channel, and
//! drawn in `Text::color`.
//!
//! During a transition, the text moves and scales with the entity like any
//! other texture. A label is usually a child entity of the component it
//! labels, positioned relative to it.
//!
//! ## Text larger than an atlas page
//!
//! Text is one line, or more where it has a `\n`, with no length limit. Text
//! wider or taller than an atlas page ([`AtlasConfig::page_size`], 2048
//! pixels by default) is clipped to the page, with a warning: the part that
//! fits is drawn at its normal size, and the rest is cut off.
//!
//! [`AtlasConfig::page_size`]: proteus_render::AtlasConfig::page_size

use bevy_ecs::prelude::*;
use glam::Vec4;
pub use proteus_render::{TextAlign, TextOverflow};
use proteus_render::{TextLayout, DEFAULT_ELLIPSIS};

// ---------------------------------------------------------------------------
// Text component
// ---------------------------------------------------------------------------

/// Text to draw on an entity: one line, or more where it has a `\n`.
///
/// The renderer rasterizes it into the main atlas and adds a [`BakedText`].
/// Changing a field afterwards has no effect until the `BakedText` is
/// removed; `proteus-sdk`'s `Handle::set_text` does both. Text larger than an
/// atlas page is clipped; see the [module docs](self).
///
/// Its lines can wrap at a width, align within it, take a line height, and
/// stop at a number of lines, ending in an ellipsis or clipped:
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
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Text {
    /// The text. Any character the font has can be used.
    pub content: String,
    /// Font size in pixels.
    pub size_px: f32,
    /// The text's color, RGBA. Defaults to opaque white. Its alpha is
    /// multiplied by the entity's opacity.
    pub color: Vec4,
    /// Extra space between characters, in pixels. `0.0`, the default, is the
    /// font's normal spacing; negative values tighten it. See
    /// [`FontAtlas::rasterize_text`].
    ///
    /// [`FontAtlas::rasterize_text`]: proteus_render::FontAtlas::rasterize_text
    pub letter_spacing_px: f32,
    /// The width lines wrap at, in logical pixels, and the width of the text's
    /// box, which its lines align within. `None`, the default, breaks lines
    /// only at `\n`, and the box is as wide as the widest line. A word wider
    /// than the wrap width breaks between characters.
    pub wrap_width: Option<f32>,
    /// Where lines sit across the text's box. [`TextAlign::Start`] by default.
    /// The text's anchor places the box on its entity.
    pub align: TextAlign,
    /// The distance from one line's baseline to the next, as a multiple of
    /// the size, as CSS's unitless `line-height` is. `None`, the default, is
    /// the font's own line spacing.
    pub line_height: Option<f32>,
    /// The most lines shown. `None`, the default, shows them all.
    pub max_lines: Option<u32>,
    /// How text with more lines than `max_lines` ends: with an ellipsis, "…"
    /// by default, or clipped. An ellipsis wraps as words do, and the text
    /// keeps as much of itself as fits with it; one too long to fit in
    /// `max_lines` on its own is clipped, with a warning.
    pub overflow: TextOverflow,
}

impl Text {
    /// Creates text in opaque white with normal letter spacing.
    pub fn new(content: impl Into<String>, size_px: f32) -> Self {
        Self {
            content: content.into(),
            size_px,
            color: Vec4::ONE,
            letter_spacing_px: 0.0,
            wrap_width: None,
            align: TextAlign::Start,
            line_height: None,
            max_lines: None,
            overflow: TextOverflow::Ellipsis(DEFAULT_ELLIPSIS.to_owned()),
        }
    }

    /// Wraps lines at `width` logical pixels, which is also the width of the
    /// text's box. See [`Text::wrap_width`].
    pub fn with_wrap_width(mut self, width: f32) -> Self {
        self.wrap_width = Some(width);
        self
    }

    /// Sets where lines sit across the text's box.
    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Sets the distance between baselines, as a multiple of the size: `1.5`
    /// is one and a half times the size.
    pub fn with_line_height(mut self, multiple: f32) -> Self {
        self.line_height = Some(multiple);
        self
    }

    /// Shows at most `lines` lines, ending in the text's overflow: an
    /// ellipsis, "…" by default, or clipped.
    pub fn with_max_lines(mut self, lines: u32) -> Self {
        self.max_lines = Some(lines);
        self
    }

    /// Ends text with more lines than its maximum in `ellipsis`, such as
    /// `" more"`, rather than "…".
    pub fn with_ellipsis(mut self, ellipsis: impl Into<String>) -> Self {
        self.overflow = TextOverflow::Ellipsis(ellipsis.into());
        self
    }

    /// Sets how text with more lines than its maximum ends.
    pub fn with_overflow(mut self, overflow: TextOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// How the renderer lays this text out.
    pub fn layout(&self) -> TextLayout<'_> {
        TextLayout {
            content: &self.content,
            size_px: self.size_px,
            letter_spacing_px: self.letter_spacing_px,
            wrap_width: self.wrap_width,
            align: self.align,
            line_height: self.line_height,
            max_lines: self.max_lines,
            ellipsis: match &self.overflow {
                TextOverflow::Ellipsis(e) => Some(e),
                TextOverflow::Clip => None,
            },
        }
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
