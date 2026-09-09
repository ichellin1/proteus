//! Static image decoding (M9.7) — PNG/JPEG bytes → RGBA8 pixels.
//!
//! Decoded pixels are handed to [`crate::TextureRegistry::register_static`] (M11) for `main_atlas`
//! placement, then uploaded via [`crate::QuadPipeline::write_to_main_atlas`] — the same two-step
//! flow rasterized text goes through.
//!
//! Pure Rust (the `image` crate's `png`/`jpeg` decoders have no system
//! dependency), so this works unmodified on both native and wasm32.

/// Decoded RGBA8 image, ready to register via [`crate::TextureRegistry::register_static`].
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    /// Length is `width * height * 4`.
    pub rgba_pixels: Vec<u8>,
}

/// Decode PNG or JPEG bytes into RGBA8 pixels. The format is sniffed from the
/// data itself, not a file extension.
pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("decode_image: {e}"))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(DecodedImage {
        width,
        height,
        rgba_pixels: rgba.into_raw(),
    })
}

/// Premultiplies RGB by alpha in place, in a straight-alpha RGBA8 buffer
/// (`len` a multiple of 4). `main_atlas` is stored premultiplied (see
/// `unpremultiply`'s doc comment in `quad.wgsl`) so hardware bilinear
/// filtering interpolates correctly at partial-alpha texels — filtering
/// straight alpha linearly blends whatever "don't care" RGB a source image
/// happened to store at fully-transparent pixels, which produces dark or
/// light fringing depending on that value. Called by
/// [`crate::QuadPipeline::write_to_main_atlas`] on every upload, so callers
/// (glyph rasterization, decoded images) hand it straight alpha as produced.
/// A no-op wherever alpha is 0 or 255 — i.e. every image before
/// per-pixel-transparent PNGs existed (opaque photos, `main_atlas`'s white
/// sentinel).
pub fn premultiply_alpha(rgba: &mut [u8]) {
    debug_assert_eq!(
        rgba.len() % 4,
        0,
        "premultiply_alpha: len must be a multiple of 4"
    );
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a == 255 {
            continue;
        }
        px[0] = ((px[0] as u16 * a + 127) / 255) as u8;
        px[1] = ((px[1] as u16 * a + 127) / 255) as u8;
        px[2] = ((px[2] as u16 * a + 127) / 255) as u8;
    }
}

/// Downscale `image` (aspect-preserved) so neither dimension exceeds
/// `max_side`. A no-op if it already fits.
///
/// Real photos routinely arrive far larger than anything sensible to pack
/// whole into `main_atlas` (2048×2048, shared with baked text) — a
/// 2000×3000px source, for instance, cannot fit at all in that height, and
/// even a source under 2048px in both dimensions can still starve the
/// registry's remaining space for everything else it needs to hold. Call this
/// on every [`decode_image`] result before
/// [`crate::TextureRegistry::register_static`].
pub fn resize_to_fit(image: DecodedImage, max_side: u32) -> DecodedImage {
    if image.width <= max_side && image.height <= max_side {
        return image;
    }
    // Scale by whichever dimension is more over-budget, then derive the
    // other from it — imageops::resize does not preserve aspect ratio on
    // its own if given two independently-clamped target dimensions.
    let scale = (max_side as f32 / image.width as f32).min(max_side as f32 / image.height as f32);
    let target_width = ((image.width as f32 * scale).round() as u32).max(1);
    let target_height = ((image.height as f32 * scale).round() as u32).max(1);

    let buf = image::RgbaImage::from_raw(image.width, image.height, image.rgba_pixels).expect(
        "resize_to_fit: width/height/rgba_pixels came from decode_image, must be consistent",
    );
    let resized = image::imageops::resize(
        &buf,
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3,
    );
    let (width, height) = resized.dimensions();
    DecodedImage {
        width,
        height,
        rgba_pixels: resized.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2 red PNG, base64-decoded at test time — small enough to inline,
    /// avoids needing a fixture file on disk.
    fn tiny_red_png() -> Vec<u8> {
        // Generated with the `image` crate itself (see the test below that
        // round-trips through `image::save_buffer`), not hand-authored bytes.
        let mut buf = std::io::Cursor::new(Vec::new());
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        img.write_to(&mut buf, image::ImageFormat::Png)
            .expect("encode tiny test PNG");
        buf.into_inner()
    }

    #[test]
    fn decode_image_returns_correct_dimensions() {
        let decoded = decode_image(&tiny_red_png()).expect("decode should succeed");
        assert_eq!(decoded.width, 2);
        assert_eq!(decoded.height, 2);
        assert_eq!(decoded.rgba_pixels.len(), 2 * 2 * 4);
    }

    #[test]
    fn decode_image_pixels_match_source_color() {
        let decoded = decode_image(&tiny_red_png()).expect("decode should succeed");
        for chunk in decoded.rgba_pixels.chunks_exact(4) {
            assert_eq!(chunk, &[255, 0, 0, 255]);
        }
    }

    #[test]
    fn decode_image_rejects_garbage_bytes() {
        assert!(decode_image(&[0u8, 1, 2, 3, 4, 5]).is_err());
    }

    fn solid_rgba(width: u32, height: u32) -> DecodedImage {
        DecodedImage {
            width,
            height,
            rgba_pixels: [255u8, 0, 0, 255].repeat((width * height) as usize),
        }
    }

    #[test]
    fn resize_to_fit_is_noop_when_already_within_bounds() {
        let img = solid_rgba(100, 150);
        let resized = resize_to_fit(img, 200);
        assert_eq!(resized.width, 100);
        assert_eq!(resized.height, 150);
    }

    #[test]
    fn resize_to_fit_downscales_oversized_dimension() {
        let img = resize_to_fit(solid_rgba(2000, 3000), 600);
        assert!(img.width <= 600, "width {} exceeds max_side", img.width);
        assert!(img.height <= 600, "height {} exceeds max_side", img.height);
        assert_eq!(img.rgba_pixels.len(), (img.width * img.height * 4) as usize);
    }

    #[test]
    fn resize_to_fit_preserves_aspect_ratio() {
        // 2000x3000 is exactly 2:3 — the resized image should be too, within
        // a pixel of rounding either way.
        let img = resize_to_fit(solid_rgba(2000, 3000), 600);
        let expected_height = (img.width as f32 * 1.5).round() as u32;
        assert!(
            (img.height as i64 - expected_height as i64).abs() <= 1,
            "expected ~2:3 aspect ratio, got {}x{}",
            img.width,
            img.height
        );
    }

    #[test]
    fn resize_to_fit_clamps_only_the_dimension_that_exceeds_max_side() {
        // Only height exceeds max_side (300) here — width (100) should
        // shrink proportionally, not stay at 100.
        let img = resize_to_fit(solid_rgba(100, 900), 300);
        assert_eq!(img.height, 300);
        assert!(
            img.width < 100,
            "width should shrink to preserve aspect ratio, got {}",
            img.width
        );
    }

    #[test]
    fn premultiply_alpha_is_noop_at_full_opacity() {
        let mut px = [10u8, 20, 200, 255];
        premultiply_alpha(&mut px);
        assert_eq!(px, [10, 20, 200, 255]);
    }

    #[test]
    fn premultiply_alpha_zeroes_rgb_at_zero_alpha_regardless_of_source_color() {
        // This is the whole point: a source PNG can store *any* "don't care"
        // RGB at a fully-transparent pixel (black, white, anything) — after
        // premultiplying, it's always (0,0,0,0), so hardware bilinear
        // filtering against a neighboring opaque texel can't fringe dark or
        // light depending on what that don't-care value happened to be.
        let mut black_transparent = [0u8, 0, 0, 0];
        let mut white_transparent = [255u8, 255, 255, 0];
        premultiply_alpha(&mut black_transparent);
        premultiply_alpha(&mut white_transparent);
        assert_eq!(black_transparent, [0, 0, 0, 0]);
        assert_eq!(white_transparent, [0, 0, 0, 0]);
    }

    #[test]
    fn premultiply_alpha_scales_rgb_by_alpha_fraction() {
        let mut px = [200u8, 100, 50, 128]; // alpha ≈ 0.502
        premultiply_alpha(&mut px);
        assert_eq!(px[3], 128, "alpha channel itself is untouched");
        // 200 * 128 / 255 ≈ 100, within rounding.
        assert!((px[0] as i16 - 100).abs() <= 1, "R got {}", px[0]);
        assert!((px[1] as i16 - 50).abs() <= 1, "G got {}", px[1]);
        assert!((px[2] as i16 - 25).abs() <= 1, "B got {}", px[2]);
    }

    #[test]
    fn premultiply_alpha_handles_multiple_pixels() {
        let mut buf = [255u8, 255, 255, 0, 10, 20, 30, 255];
        premultiply_alpha(&mut buf);
        assert_eq!(&buf[0..4], &[0, 0, 0, 0]);
        assert_eq!(&buf[4..8], &[10, 20, 30, 255]);
    }
}
