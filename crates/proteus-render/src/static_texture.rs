//! Decoding PNG and JPEG images into RGBA pixels.
//!
//! The pixels are then placed in the main atlas with
//! [`crate::TextureRegistry::register_static`] and uploaded with
//! [`crate::QuadPipeline::write_to_main_atlas`], as text is. The decoders are
//! pure Rust, so this works natively and in wasm.

/// A decoded image, ready to place with [`crate::TextureRegistry::register_static`].
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA pixels, `width * height * 4` bytes, not premultiplied.
    pub rgba_pixels: Vec<u8>,
}

/// Decodes a PNG or JPEG image into RGBA pixels. The format is detected from
/// the data.
///
/// # Errors
///
/// If the bytes aren't a PNG or JPEG image the decoder can read.
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

/// Multiplies each pixel's red, green and blue by its alpha, in place. `rgba`
/// is straight-alpha RGBA, a multiple of 4 bytes long.
///
/// The main atlas is stored premultiplied so that bilinear filtering is
/// correct where alpha changes; see `unpremultiply` in `quad.wgsl`.
/// [`crate::QuadPipeline::write_to_main_atlas`] calls it on every upload, so
/// callers pass straight alpha. Pixels with alpha 0 or 255 are unchanged.
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

/// Scales `image` down, keeping its shape, so neither side exceeds `max_side`.
/// Does nothing if it already fits.
///
/// Photos are often much larger than they will be drawn, and a large image can
/// fill the atlas page, or not fit at all. Call it on each [`decode_image`]
/// result before [`crate::TextureRegistry::register_static`].
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

    // A 2×2 red PNG, inline, so the tests need no fixture file.
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
        // A PNG can store any color at a fully transparent pixel. After
        // premultiplying it is always (0, 0, 0, 0), so filtering next to an
        // opaque pixel doesn't pick up that color.
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
