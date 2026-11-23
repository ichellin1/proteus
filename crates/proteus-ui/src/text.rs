//! Text drawn on an entity, in one line or, where it has a `\n`, more.
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
//! Text has no length limit, and is one line or, where it has a `\n`, more.
//! Text wider or taller than an atlas page ([`AtlasConfig::page_size`], 2048
//! pixels by default) is clipped to the page, with a warning. The part that
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

/// Text to draw on an entity, in one line or, where it has a `\n`, more.
///
/// The renderer rasterizes it into the main atlas and adds a [`BakedText`].
/// Changing a field afterwards has no effect until the `BakedText` is
/// removed. `proteus-sdk`'s `Handle::set_text` does both. Text larger than an
/// atlas page is clipped. See the [module docs](self).
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
    /// ellipsis, "…" by default, or is clipped. An ellipsis wraps as words do,
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
            size_px,
            color: Vec4::ONE,
            letter_spacing_px: 0.0,
            wrap_width: None,
            align: TextAlign::Start,
            line_height: None,
            max_lines: None,
            overflow: TextOverflow::Ellipsis(DEFAULT_ELLIPSIS.to_owned()),
            family: None,
            weight: None,
            italic: false,
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
    /// has more lines than its maximum.
    pub fn with_ellipsis(mut self, ellipsis: impl Into<String>) -> Self {
        self.overflow = TextOverflow::Ellipsis(ellipsis.into());
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
            family: self.family.as_deref(),
            weight: self.weight,
            italic: self.italic,
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

/// The fonts an app has loaded, or said it will load, by family.
/// `proteus-sdk`'s `Proteus::load_font`, `expect_font` and `font_failed`
/// change it. Before the renderer next bakes text, it adds each font loaded.
#[derive(Resource, Debug, Default)]
pub struct Fonts {
    status: std::collections::HashMap<String, FontStatus>,
    /// Fonts loaded that the renderer hasn't added yet, each as its bytes and
    /// its family.
    pending: Vec<(std::sync::Arc<[u8]>, String)>,
    /// The families a warning has been logged for.
    warned: std::collections::HashSet<String>,
    /// The fallback families, in the order they're tried.
    fallbacks: Vec<String>,
    /// Whether the fallback families changed since the renderer last took
    /// them.
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

    /// Adds a font of `family`, from its file's `bytes`, for the renderer to
    /// load.
    pub fn loaded(&mut self, family: impl Into<String>, bytes: std::sync::Arc<[u8]>) {
        let family = family.into();
        self.status.insert(family.clone(), FontStatus::Loaded);
        self.pending.push((bytes, family));
        self.version += 1;
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
        self.fallbacks = families;
        self.fallbacks_changed = true;
        self.version += 1;
    }

    /// Returns the fallback families, in the order they're tried.
    pub fn fallbacks(&self) -> &[String] {
        &self.fallbacks
    }

    /// Returns the fallback families if they changed since the last call, or
    /// `None`. This is for the renderer.
    pub fn take_fallbacks(&mut self) -> Option<Vec<String>> {
        std::mem::take(&mut self.fallbacks_changed).then(|| self.fallbacks.clone())
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

    /// Takes the fonts loaded since the last call, for the renderer.
    pub fn take_pending(&mut self) -> Vec<(std::sync::Arc<[u8]>, String)> {
        std::mem::take(&mut self.pending)
    }

    /// Returns whether this is the first time `family` is warned about, so
    /// each family's warning is logged once.
    pub fn first_warning(&mut self, family: &str) -> bool {
        self.warned.insert(family.to_owned())
    }
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
