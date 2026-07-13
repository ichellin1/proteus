//! Text ECS components for M4 — single-line SDF text rendering.
//!
//! ## Data flow
//!
//! ```text
//! Text { content, size_px }          ← developer declares on an entity
//!         │
//!         │ (shell detects Text without BakedText, calls FontAtlas::bake_text)
//!         ▼
//! FontAtlas::bake_text → BakedRegion (CPU pixels + atlas coords)
//!         │
//!         │ (shell calls QuadPipeline::write_to_main_atlas)
//!         ▼
//! BakedText { uv_offset, uv_scale }  ← written back onto the entity
//!         │
//!         │ (render loop uses these UVs in QuadInstance)
//!         ▼
//! GPU shader samples main_atlas sub-region → tinted text pixel
//! ```
//!
//! ## Color
//!
//! Text pixels are rasterized as white (R=G=B=255) with glyph coverage in the
//! alpha channel. The entity's `QuadState::color` field tints the text in the
//! shader, so the same baked texture can appear in any color without re-baking.
//!
//! ## Transitions
//!
//! A text-bearing entity is treated identically to any other textured quad. The
//! transition system lerps `QuadState` (position, size, color, corner_radius)
//! while the baked text texture stays in the atlas. This means text can shrink,
//! grow, move, and fade exactly like any other component — no special-casing.

use bevy_ecs::prelude::*;
use glam::Vec4;

// ---------------------------------------------------------------------------
// Text component
// ---------------------------------------------------------------------------

/// Declares that an entity should display a single line of text.
///
/// When this component is present on an entity that does not yet have a
/// [`BakedText`] component, the shell's text-bake step rasterizes the
/// string, uploads it to `main_atlas`, and inserts [`BakedText`] with the
/// resulting UV coordinates.
///
/// Re-baking on content or size change is not yet implemented (M4 Phase 1).
/// The baked texture is permanent for the entity's lifetime.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Text {
    /// The string to render. Supports all glyphs present in the embedded font.
    pub content: String,
    /// Font size in pixels. Valid range: 1.0 – 512.0.
    /// Sizes outside 12–48 px may render with reduced quality on some hardware.
    pub size_px: f32,
    /// RGBA color of the rendered glyphs.
    ///
    /// Defaults to opaque white (`Vec4::ONE`), which reads well on any colored
    /// background. Set to a dark value for light backgrounds, or to any accent
    /// color to match your design.
    ///
    /// The alpha channel is multiplied by the entity's whole-component opacity
    /// in the shader, so partially-transparent text is supported.
    pub color: Vec4,
}

impl Text {
    /// Convenience constructor. Glyph color defaults to opaque white.
    pub fn new(content: impl Into<String>, size_px: f32) -> Self {
        Self {
            content: content.into(),
            size_px,
            color: Vec4::ONE,
        }
    }

    /// Override the glyph color. Builder-style so it chains with `Text::new`.
    ///
    /// ```rust,ignore
    /// Text::new("Hello", 22.0).with_color(Vec4::new(0.1, 0.1, 0.1, 1.0))
    /// ```
    pub fn with_color(mut self, color: Vec4) -> Self {
        self.color = color;
        self
    }
}

// ---------------------------------------------------------------------------
// BakedText component
// ---------------------------------------------------------------------------

/// Written by the shell after a [`Text`] entity's string has been rasterized
/// and uploaded to the GPU `main_atlas`.
///
/// The render loop reads `uv_offset` and `uv_scale` to point the entity's
/// [`QuadInstance`] at the correct atlas sub-region.
///
/// [`QuadInstance`]: proteus_render::QuadInstance
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BakedText {
    /// Normalised UV origin within `main_atlas` (top-left corner of the text region).
    /// Range: [0, 1] × [0, 1].
    pub uv_offset: [f32; 2],
    /// Normalised UV extent of the text region.
    /// `uv_offset + uv_scale` gives the bottom-right UV corner.
    pub uv_scale: [f32; 2],
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
        };
        let mut world = World::new();
        let e = world.spawn(baked.clone()).id();
        let b = world.get::<BakedText>(e).unwrap();
        assert_eq!(b.uv_offset, [0.1, 0.2]);
        assert_eq!(b.uv_scale, [0.3, 0.05]);
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
                },
            ))
            .id();
        assert!(world.get::<Text>(e).is_some());
        assert!(world.get::<BakedText>(e).is_some());
    }
}
