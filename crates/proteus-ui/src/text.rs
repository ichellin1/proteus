//! Text: a single line of text drawn on an entity.
//!
//! ```text
//! Text { content, size_px }      declared on an entity
//!         │  the renderer finds Text without BakedText
//!         ▼
//! FontAtlas::rasterize_text      the glyphs, as pixels
//!         │  added to the main atlas
//!         ▼
//! BakedText + TextureRef         where the text is in the atlas
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

use bevy_ecs::prelude::*;
use glam::Vec4;

// ---------------------------------------------------------------------------
// Text component
// ---------------------------------------------------------------------------

/// A single line of text to draw on an entity.
///
/// The renderer rasterizes it into the main atlas and adds a [`BakedText`].
/// Changing `content` or `size_px` afterwards has no effect until the
/// `BakedText` is removed.
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
}

impl Text {
    /// Creates text in opaque white with normal letter spacing.
    pub fn new(content: impl Into<String>, size_px: f32) -> Self {
        Self {
            content: content.into(),
            size_px,
            color: Vec4::ONE,
            letter_spacing_px: 0.0,
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
