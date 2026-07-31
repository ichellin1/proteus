//! Integration tests for the M4 text rendering pipeline.
//!
//! These tests exercise `FontAtlas` end-to-end — rasterization and pixel buffer integrity —
//! without requiring a GPU device. Atlas packing/placement (once part of `FontAtlas` itself) moved
//! to `TextureRegistry`/`MainAtlasAllocator` in M11 — see `texture_registry.rs`'s and
//! `main_atlas_allocator.rs`'s own unit tests for that coverage. GPU-dependent tests
//! (write_to_main_atlas) live in `headless_render.rs`.

use proteus_render::{FontAtlas, EMBEDDED_FONT_BYTES, MAIN_ATLAS_SIZE};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a `FontAtlas` backed by the embedded Inter Bold font.
fn atlas() -> FontAtlas {
    FontAtlas::with_embedded_font()
}

// ---------------------------------------------------------------------------
// Embedded font
// ---------------------------------------------------------------------------

#[test]
fn embedded_font_bytes_are_non_empty() {
    assert!(
        !EMBEDDED_FONT_BYTES.is_empty(),
        "embedded font bytes should not be empty"
    );
    // TTF/OTF files begin with a specific magic number.
    // TTF: 0x00 01 00 00  or 'true' (0x74 72 75 65)
    // OTF: 'OTTO' (0x4F 54 54 4F)
    let magic = &EMBEDDED_FONT_BYTES[..4];
    let is_ttf = magic == [0x00, 0x01, 0x00, 0x00] || magic == b"true";
    let is_otf = magic == b"OTTO";
    assert!(
        is_ttf || is_otf,
        "embedded font does not have a valid TTF/OTF magic number: {magic:?}"
    );
}

// ---------------------------------------------------------------------------
// FontAtlas construction
// ---------------------------------------------------------------------------

#[test]
fn font_atlas_constructs_with_embedded_font() {
    // Must not panic.
    let _fa = atlas();
}

#[test]
fn font_atlas_constructs_with_custom_bytes() {
    // Verify the `new` constructor with explicit bytes works too.
    let _fa = FontAtlas::new(EMBEDDED_FONT_BYTES);
}

// ---------------------------------------------------------------------------
// rasterize_text — basic sanity
// ---------------------------------------------------------------------------

#[test]
fn rasterize_ascii_string_succeeds() {
    let mut fa = atlas();
    let r = fa.rasterize_text("Hello, World!", 24.0);
    assert!(
        r.is_some(),
        "rasterize_text returned None for a simple ASCII string"
    );
}

#[test]
fn rasterize_empty_string_returns_none() {
    let mut fa = atlas();
    assert!(
        fa.rasterize_text("", 24.0).is_none(),
        "expected None for empty string"
    );
}

#[test]
fn rasterize_single_character_succeeds() {
    let mut fa = atlas();
    let r = fa.rasterize_text("A", 32.0);
    assert!(
        r.is_some(),
        "rasterize_text returned None for single character 'A'"
    );
    let r = r.unwrap();
    assert!(r.width > 0, "width should be positive");
    assert!(r.height > 0, "height should be positive");
}

// ---------------------------------------------------------------------------
// Pixel buffer integrity
// ---------------------------------------------------------------------------

#[test]
fn pixel_buffer_length_matches_dimensions() {
    let mut fa = atlas();
    let r = fa.rasterize_text("Test", 20.0).unwrap();
    assert_eq!(
        r.rgba_pixels.len(),
        (r.width * r.height * 4) as usize,
        "pixel buffer length should be width × height × 4"
    );
}

#[test]
fn pixel_buffer_rgb_channels_are_white_where_alpha_nonzero() {
    let mut fa = atlas();
    let r = fa.rasterize_text("Xx", 32.0).unwrap();
    for (i, chunk) in r.rgba_pixels.chunks_exact(4).enumerate() {
        let (r_ch, g, b, a) = (chunk[0], chunk[1], chunk[2], chunk[3]);
        if a > 0 {
            assert_eq!(
                r_ch, 255,
                "pixel {i}: R should be 255 where alpha > 0 (got {r_ch})"
            );
            assert_eq!(
                g, 255,
                "pixel {i}: G should be 255 where alpha > 0 (got {g})"
            );
            assert_eq!(
                b, 255,
                "pixel {i}: B should be 255 where alpha > 0 (got {b})"
            );
        }
    }
}

#[test]
fn pixel_buffer_has_visible_coverage() {
    // At least some pixels must be non-transparent for any renderable character.
    let mut fa = atlas();
    let r = fa.rasterize_text("Proteus", 24.0).unwrap();
    let has_visible = r.rgba_pixels.chunks_exact(4).any(|c| c[3] > 0);
    assert!(
        has_visible,
        "rasterized text should contain at least one visible pixel"
    );
}

// ---------------------------------------------------------------------------
// Sizes 12 – 48 px (M4 DoD requirement)
// ---------------------------------------------------------------------------

#[test]
fn rasterize_succeeds_at_all_required_sizes() {
    let mut fa = atlas();
    for size_px in [12.0_f32, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0] {
        let r = fa.rasterize_text("Ag", size_px);
        assert!(r.is_some(), "rasterize_text returned None at {size_px}px");
        let r = r.unwrap();
        assert!(
            r.width > 0 && r.height > 0,
            "zero-size glyphs at {size_px}px"
        );
    }
}

#[test]
fn larger_size_produces_larger_glyphs() {
    let mut fa = atlas();
    let small = fa.rasterize_text("A", 12.0).unwrap();
    let large = fa.rasterize_text("A", 48.0).unwrap();
    // Larger font size must produce a taller (or equal) glyph run.
    assert!(
        large.height >= small.height,
        "48px glyph height ({}) should be ≥ 12px glyph height ({})",
        large.height,
        small.height,
    );
    assert!(
        large.width >= small.width,
        "48px glyph width ({}) should be ≥ 12px glyph width ({})",
        large.width,
        small.width,
    );
}

#[test]
fn wider_string_produces_wider_glyph_run() {
    let mut fa = atlas();
    let single = fa.rasterize_text("I", 24.0).unwrap();
    let wide = fa.rasterize_text("WWWWWWWW", 24.0).unwrap();
    assert!(
        wide.width > single.width,
        "wide string width ({}) should be > single char width ({})",
        wide.width,
        single.width
    );
}

// ---------------------------------------------------------------------------
// MAIN_ATLAS_SIZE constant
// ---------------------------------------------------------------------------

#[test]
fn main_atlas_size_is_positive() {
    // Constant assertion — evaluated at compile time.
    const { assert!(MAIN_ATLAS_SIZE > 0, "MAIN_ATLAS_SIZE must be positive") };
}

#[test]
fn main_atlas_size_is_power_of_two() {
    // Constant assertion — evaluated at compile time.
    // (Format args are not allowed in const context; message is a static literal.)
    const {
        assert!(
            MAIN_ATLAS_SIZE.is_power_of_two(),
            "MAIN_ATLAS_SIZE must be a power of two for GPU compatibility",
        )
    };
}
