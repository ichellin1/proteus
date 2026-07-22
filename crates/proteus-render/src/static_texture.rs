//! Static image decoding (M9.7) — PNG/JPEG bytes → RGBA8 pixels.
//!
//! Decoded pixels are handed to [`crate::FontAtlas::bake_image`], which packs
//! them into `main_atlas` via the same shelf packer text baking already uses
//! (see that module's docs for why sharing one packer instance matters — two
//! independent packers writing into the same atlas texture would collide).
//!
//! Pure Rust (the `image` crate's `png`/`jpeg` decoders have no system
//! dependency), so this works unmodified on both native and wasm32.

/// Decoded RGBA8 image, ready to hand to [`crate::FontAtlas::bake_image`].
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

/// Downscale `image` (aspect-preserved) so neither dimension exceeds
/// `max_side`. A no-op if it already fits.
///
/// Real photos routinely arrive far larger than anything sensible to pack
/// whole into `main_atlas` (2048×2048, shared with baked text) — a
/// 2000×3000px source, for instance, cannot fit at all in that height, and
/// even a source under 2048px in both dimensions can still starve the shelf
/// packer's remaining space for everything else it needs to hold. Call this
/// on every [`decode_image`] result before [`crate::FontAtlas::bake_image`].
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
}
