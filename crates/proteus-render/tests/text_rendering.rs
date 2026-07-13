//! Integration tests for the M4 text rendering pipeline.
//!
//! These tests exercise `FontAtlas` end-to-end — rasterization, packing, and
//! UV coordinate computation — without requiring a GPU device. GPU-dependent
//! tests (write_to_main_atlas) live in `headless_render.rs`.

use proteus_render::{BakedRegion, FontAtlas, EMBEDDED_FONT_BYTES, MAIN_ATLAS_SIZE};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a 2048×2048 FontAtlas backed by the embedded DejaVu Sans font.
fn atlas() -> FontAtlas {
    FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE)
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
    let _fa = FontAtlas::new(EMBEDDED_FONT_BYTES, 1024, 1024, 0);
}

// ---------------------------------------------------------------------------
// bake_text — basic sanity
// ---------------------------------------------------------------------------

#[test]
fn bake_ascii_string_succeeds() {
    let mut fa = atlas();
    let r = fa.bake_text("Hello, World!", 24.0);
    assert!(
        r.is_some(),
        "bake_text returned None for a simple ASCII string"
    );
}

#[test]
fn bake_empty_string_returns_none() {
    let mut fa = atlas();
    assert!(
        fa.bake_text("", 24.0).is_none(),
        "expected None for empty string"
    );
}

#[test]
fn bake_single_character_succeeds() {
    let mut fa = atlas();
    let r = fa.bake_text("A", 32.0);
    assert!(
        r.is_some(),
        "bake_text returned None for single character 'A'"
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
    let r = fa.bake_text("Test", 20.0).unwrap();
    assert_eq!(
        r.rgba_pixels.len(),
        (r.width * r.height * 4) as usize,
        "pixel buffer length should be width × height × 4"
    );
}

#[test]
fn pixel_buffer_rgb_channels_are_white_where_alpha_nonzero() {
    let mut fa = atlas();
    let r = fa.bake_text("Xx", 32.0).unwrap();
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
    let r = fa.bake_text("Proteus", 24.0).unwrap();
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
fn bake_succeeds_at_all_required_sizes() {
    let mut fa = atlas();
    for size_px in [12.0_f32, 16.0, 20.0, 24.0, 32.0, 40.0, 48.0] {
        let r = fa.bake_text("Ag", size_px);
        assert!(r.is_some(), "bake_text returned None at {size_px}px");
        let r = r.unwrap();
        assert!(
            r.width > 0 && r.height > 0,
            "zero-size region at {size_px}px"
        );
    }
}

#[test]
fn larger_size_produces_larger_region() {
    let mut fa = atlas();
    let small = fa.bake_text("A", 12.0).unwrap();
    let large = fa.bake_text("A", 48.0).unwrap();
    // Larger font size must produce a taller (or equal) region.
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

// ---------------------------------------------------------------------------
// Atlas packing — no overlap, correct UV range
// ---------------------------------------------------------------------------

#[test]
fn successive_allocations_do_not_overlap() {
    let mut fa = atlas();
    let a = fa.bake_text("Hello", 24.0).unwrap();
    let b = fa.bake_text("World", 24.0).unwrap();
    let c = fa.bake_text("Foo", 32.0).unwrap();

    fn overlaps(a: &BakedRegion, b: &BakedRegion) -> bool {
        let ax2 = a.x + a.width;
        let ay2 = a.y + a.height;
        let bx2 = b.x + b.width;
        let by2 = b.y + b.height;
        a.x < bx2 && ax2 > b.x && a.y < by2 && ay2 > b.y
    }

    assert!(!overlaps(&a, &b), "regions a and b overlap: {a:?} vs {b:?}");
    assert!(!overlaps(&a, &c), "regions a and c overlap: {a:?} vs {c:?}");
    assert!(!overlaps(&b, &c), "regions b and c overlap: {b:?} vs {c:?}");
}

#[test]
fn uv_offset_is_in_unit_range() {
    let mut fa = atlas();
    let r = fa.bake_text("UV test", 20.0).unwrap();
    let [ox, oy] = r.uv_offset(MAIN_ATLAS_SIZE);
    assert!((0.0..=1.0).contains(&ox), "uv_offset.x out of [0,1]: {ox}");
    assert!((0.0..=1.0).contains(&oy), "uv_offset.y out of [0,1]: {oy}");
}

#[test]
fn uv_scale_is_positive_and_fits_atlas() {
    let mut fa = atlas();
    let r = fa.bake_text("UV test", 20.0).unwrap();
    let [ox, oy] = r.uv_offset(MAIN_ATLAS_SIZE);
    let [sx, sy] = r.uv_scale(MAIN_ATLAS_SIZE);
    assert!(sx > 0.0, "uv_scale.x should be positive: {sx}");
    assert!(sy > 0.0, "uv_scale.y should be positive: {sy}");
    assert!(
        ox + sx <= 1.0 + 1e-6,
        "UV region exceeds atlas width: {ox} + {sx}"
    );
    assert!(
        oy + sy <= 1.0 + 1e-6,
        "UV region exceeds atlas height: {oy} + {sy}"
    );
}

#[test]
fn uv_scale_reflects_relative_region_size() {
    // A wider string should produce a wider UV scale than a single character.
    let mut fa = atlas();
    let single = fa.bake_text("I", 24.0).unwrap();
    let wide = fa.bake_text("WWWWWWWW", 24.0).unwrap();
    let [sx_single, _] = single.uv_scale(MAIN_ATLAS_SIZE);
    let [sx_wide, _] = wide.uv_scale(MAIN_ATLAS_SIZE);
    assert!(
        sx_wide > sx_single,
        "wide string uv_scale.x ({sx_wide}) should be > single char uv_scale.x ({sx_single})"
    );
}

// ---------------------------------------------------------------------------
// Atlas allocation with y_offset
// ---------------------------------------------------------------------------

#[test]
fn y_offset_skips_reserved_rows() {
    // With y_offset=10, no allocation should start above row 10.
    let mut fa = FontAtlas::new(EMBEDDED_FONT_BYTES, MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE, 10);
    let r = fa.bake_text("A", 24.0).unwrap();
    assert!(
        r.y >= 10,
        "allocation should respect y_offset=10, got y={}",
        r.y
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
