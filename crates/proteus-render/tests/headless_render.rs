// Headless render integration test.
//
// Spins up a wgpu device without a surface, renders a known scene to an
// offscreen `Rgba8Unorm` texture, reads the pixels back via a mappable buffer,
// and asserts expected colors at specific pixel coordinates.
//
// If no GPU adapter is available, each test reports itself skipped, by name,
// rather than failing, unless `REQUIRE_GPU` is set, as on CI. Linux CI gets
// an adapter from `mesa-vulkan-drivers` (lavapipe); see
// `.github/workflows/ci.yml`.

use proteus_render::{
    pack_atlas_page, AtlasConfig, MainAtlasPlacement, QuadInstance, QuadPipeline,
    ATLAS_SELECTOR_MAIN, ATLAS_SELECTOR_TRANSITION, DEFAULT_TRANSITION_ATLAS_SIZE,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

// 64 columns × 4 bytes/px = 256 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT exactly.
// No padding row needed, which simplifies the readback math.
const BYTES_PER_ROW: u32 = WIDTH * 4;

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

// Render a 32×32 red quad into a 64×64 off-screen texture and verify:
//
//  - Center pixel (32,32) is red   — the quad covers the center quarter.
//  - Corner pixels (0,0), (63,63)  are black — the clear color.
//
// Scene layout (with `ortho(64,64)`, 1 unit = 1 pixel, origin at center):
//
// ```text
//  (0,0) ─────────────── (63,0)
//    │    black           │
//    │  (16,16)──(48,16)  │
//    │    │   red  │      │
//    │  (16,48)──(48,48)  │
//    │           black    │
//  (0,63)─────────────── (63,63)
// ```
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn headless_quad_renders_to_expected_color() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };

    // --- Pipeline ---
    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // --- Instance: 32×32 red quad centered at world origin, no corner radius ---
    //
    // With ortho(64, 64) the quad covers NDC [-0.5, 0.5]×[-0.5, 0.5],
    // which maps to texture rows [16, 48] and cols [16, 48].
    // The center pixel (row=32, col=32) is well inside.
    let instances = [QuadInstance {
        position: [0.0, 0.0, 0.5],
        size: [32.0, 32.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [1.0, 0.0, 0.0, 1.0], // red
        opacity: 1.0,
        corner_radii: [0; 4], // sharp corners — no SDF rounding at edges
        uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
        uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
        atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 0.0, 0.0],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
    }];
    let black = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let pixels = render_and_read_back(&device, &queue, &mut pipeline, &instances, black);

    // --- Pixel assertions ---
    let pixel = |row: u32, col: u32| -> [u8; 4] {
        let off = (row * BYTES_PER_ROW + col * 4) as usize;
        pixels[off..off + 4].try_into().unwrap()
    };

    // Center (32, 32) — safely inside the red quad.
    //
    // The `> 200` tolerance is looser than needed, since the white pixel reads
    // pure white (see `WHITE_PIXEL_UV_OFFSET`). This test is about the quad
    // landing in the right place with the right hue;
    // `untextured_quad_renders_at_full_intensity_on_a_non_default_page_size`
    // below checks the intensity.
    let center = pixel(HEIGHT / 2, WIDTH / 2);
    assert!(
        center[0] > 200,
        "center.R expected bright red (>200), got {}",
        center[0]
    );
    assert!(center[1] < 10, "center.G expected ~0, got {}", center[1]);
    assert!(center[2] < 10, "center.B expected ~0, got {}", center[2]);

    // Top-left corner (0, 0) — outside the quad, should be the clear color (black).
    let tl = pixel(0, 0);
    assert!(tl[0] < 10, "top-left.R expected ~0, got {}", tl[0]);
    assert!(tl[1] < 10, "top-left.G expected ~0, got {}", tl[1]);
    assert!(tl[2] < 10, "top-left.B expected ~0, got {}", tl[2]);

    // Bottom-right corner (63, 63) — also outside the quad.
    let br = pixel(HEIGHT - 1, WIDTH - 1);
    assert!(br[0] < 10, "bottom-right.R expected ~0, got {}", br[0]);
    assert!(br[1] < 10, "bottom-right.G expected ~0, got {}", br[1]);
    assert!(br[2] < 10, "bottom-right.B expected ~0, got {}", br[2]);
}

// A component with a `Glow` and a texture that has a genuine transparent
// hole in it (e.g. the reference demo's animated-logo mark, hatch gaps
// rendered as real alpha) must let whatever is *behind* the component show
// through that hole — not the glow's own color.
//
// Checks the `shadow_alpha *= smoothstep(-1.0, 1.0, dist)` interior mask in
// the shader's `fragment.wgsl`. Without it, `shadow_alpha` stays roughly constant across the
// component's whole interior, not just near the edge. That only shows where
// the texture is transparent; under opaque content it's hidden.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn glow_does_not_leak_through_transparent_texture_holes() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };

    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // A 4×4 fully-transparent region — every texel is (0,0,0,0), so bilinear
    // sampling anywhere within it (no neighboring non-transparent texel to
    // blend against) reads back exactly transparent, keeping this test
    // focused on the shadow/glow mask rather than premultiplied-alpha
    // filtering.
    let texture_id = pipeline
        .texture_registry
        .register_static(4, 4, false)
        .expect("main_atlas has room for a 4x4 region");
    let placement = pipeline
        .texture_registry
        .main_atlas_region(texture_id)
        .expect("just registered");
    // Write to the region registered above, not to (0, 0), which is the white pixel
    // every untextured component samples.
    pipeline.write_to_main_atlas(&queue, placement, &[0u8; 4 * 4 * 4]);
    let uv = pipeline
        .texture_registry
        .main_atlas_uv(texture_id)
        .expect("just registered");
    let (uv_offset, uv_scale) = (uv.uv_offset, uv.uv_scale);

    // Same 32×32 quad footprint as the test above (rows/cols [16,48]), fully
    // untinted so the transparent texture drives main_color's alpha, with a
    // `Glow`-shaped shadow: navy at 0.8 effective alpha, softness 10, zero
    // offset and spread.
    let instances = [QuadInstance {
        position: [0.0, 0.0, 0.5],
        size: [32.0, 32.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [1.0, 1.0, 1.0, 1.0], // untinted
        opacity: 1.0,
        corner_radii: [0; 4],
        uv_offset,
        uv_scale,
        // The page the region is actually on; it isn't always 0.
        atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, uv.page),
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 10.0, 0.0], // offset 0, softness 10, spread 0
        shadow_color: [0.0, 0.0, 0.502, 0.8], // navy @ 0.8
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
    }];

    // Clear to a color nothing else in this scene produces, so "the clear
    // color shows through" is unambiguous in the assertions below.
    let green = wgpu::Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    let pixels = render_and_read_back(&device, &queue, &mut pipeline, &instances, green);
    let pixel = |row: u32, col: u32| -> [u8; 4] {
        let off = (row * BYTES_PER_ROW + col * 4) as usize;
        pixels[off..off + 4].try_into().unwrap()
    };

    // Center (32, 32) — deep inside the quad, sampling the transparent hole.
    // Must be the green clear color, not the navy glow.
    let center = pixel(HEIGHT / 2, WIDTH / 2);
    assert!(
        center[1] > 200,
        "center.G expected bright green (>200) — background should show through \
         the transparent hole, got {center:?}"
    );
    assert!(
        center[2] < 50,
        "center.B expected ~0 (no navy glow bleeding through the hole), got {center:?}"
    );

    // A couple of pixels beyond the quad's right edge (col 48), still well
    // within the softness-10 halo range — the glow should still visibly
    // bleed outward here, i.e. NOT pure green.
    let halo = pixel(HEIGHT / 2, 50);
    assert!(
        halo[2] > 20,
        "halo pixel just outside the shape should show glow bleed (elevated B \
         channel from navy), got {halo:?} — the interior mask must not have \
         suppressed the exterior halo too"
    );
}

// Checks that the main-atlas page is used all the way through: by
// `write_to_main_atlas`'s `origin.z`, the array bind group, and the shader's
// `sample_atlas`, in `fill.wgsl`.
//
// Writes solid red into page 1 at (8, 8), leaving page 0 at that same (x, y) untouched (still
// zero-initialized, i.e. transparent). Renders two quads with *identical* UVs side by side —
// one sampling page 1, one sampling page 0 — against a green clear. Both assertions matter:
// the positive one (page 1 shows red) proves the layer index reaches the GPU at all; the
// negative one (page 0 does *not* show red, despite identical UVs) is what actually rules out
// "the shader ignores the layer and always samples layer 0" — without it, a shader that
// dropped the layer argument entirely would still pass the positive half by coincidence.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn content_written_to_a_nonzero_main_atlas_page_renders_from_that_page() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };

    let page_size = 64u32;
    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig {
            page_size,
            page_count: 2,
        },
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // Bypass the allocator deliberately — this test targets the GPU layer/index plumbing
    // itself, not allocation policy (that's texture_registry.rs's job).
    let placement = MainAtlasPlacement {
        page: 1,
        x: 8,
        y: 8,
        width: 8,
        height: 8,
    };
    pipeline.write_to_main_atlas(&queue, placement, &[255u8; 8 * 8 * 4]); // opaque red-capable (white; tinted red below)
    let s = page_size as f32;
    let uv_offset = [placement.x as f32 / s, placement.y as f32 / s];
    let uv_scale = [placement.width as f32 / s, placement.height as f32 / s];

    let quad = |x: f32, page: u32| QuadInstance {
        position: [x, 0.0, 0.5],
        size: [16.0, 16.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [1.0, 0.0, 0.0, 1.0], // red tint — visible only where the sampled alpha is opaque
        opacity: 1.0,
        corner_radii: [0; 4],
        uv_offset,
        uv_scale,
        atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, page),
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 0.0, 0.0],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
    };
    // Side by side: left quad samples page 1 (written), right quad samples page 0
    // (untouched) — identical UVs otherwise.
    let instances = [quad(-16.0, 1), quad(16.0, 0)];

    let green = wgpu::Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    let pixels = render_and_read_back(&device, &queue, &mut pipeline, &instances, green);
    let pixel = |row: u32, col: u32| -> [u8; 4] {
        let off = (row * BYTES_PER_ROW + col * 4) as usize;
        pixels[off..off + 4].try_into().unwrap()
    };

    // Left quad (col 16, page 1) — should show the opaque red content that was written there.
    let page1 = pixel(HEIGHT / 2, 16);
    assert!(
        page1[0] > 200 && page1[1] < 10,
        "page 1 should render the red content written to it, got {page1:?}"
    );

    // Right quad (col 48, page 0, identical UVs) — page 0 was never written at (8, 8), so this
    // must show the green clear color, not red — the negative control proving the layer index
    // is genuinely selecting a different physical layer, not being ignored by the shader.
    let page0 = pixel(HEIGHT / 2, 48);
    assert!(
        page0[1] > 200 && page0[0] < 50,
        "page 0 at the same (x, y) must NOT show the content written to page 1 — got \
         {page0:?}. If this is red, the shader is ignoring the array-layer index."
    );
}

// An untextured quad must render its `color` at full intensity **whatever
// `AtlasConfig::page_size` is set to**.
//
// `WHITE_PIXEL_UV_OFFSET` is a normalized UV, so the texel it lands on
// depends on the page size. A texel-center offset such as `0.5 / 2048` is the
// center of texel 0 only on a 2048px page. At 4096 (the
// `ProteusConfig::desktop()` preset) it lands on the texel 0/1 boundary on
// both axes, so the bilinear sampler averages the white texel with three
// unwritten ones, and every solid-color quad renders at about quarter
// intensity.
//
// Rendered here at 4096 against a green clear: a red quad must come back
// essentially pure red, not a quarter-strength wash of it.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn untextured_quad_renders_at_full_intensity_on_a_non_default_page_size() {
    let Some((device, queue)) = pollster::block_on(make_device_with_large_textures(4096)) else {
        // Not a failure even with `REQUIRE_GPU`, which only promises an
        // adapter: a software renderer's textures may not reach 4096 pixels.
        report_skip("no GPU adapter whose textures reach 4096 pixels");
        return;
    };

    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig {
            page_size: 4096,
            page_count: 1,
        },
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    let instances = [QuadInstance {
        position: [0.0, 0.0, 0.5],
        size: [32.0, 32.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [1.0, 0.0, 0.0, 1.0], // red
        opacity: 1.0,
        corner_radii: [0; 4],
        uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
        uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
        atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 0.0, 0.0],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
    }];

    // Green clear, so a sentinel that samples partly-transparent texels shows up
    // as red blended toward green rather than as a subtler dimming.
    let green = wgpu::Color {
        r: 0.0,
        g: 1.0,
        b: 0.0,
        a: 1.0,
    };
    let pixels = render_and_read_back(&device, &queue, &mut pipeline, &instances, green);
    let off = ((HEIGHT / 2) * BYTES_PER_ROW + (WIDTH / 2) * 4) as usize;
    let center: [u8; 4] = pixels[off..off + 4].try_into().unwrap();

    assert!(
        center[0] > 240,
        "centre should be essentially full red at page_size 4096, got {center:?} — the \
         white-pixel sentinel is sampling outside the white block"
    );
    assert!(
        center[1] < 15,
        "no green should survive under an opaque quad, got {center:?}"
    );
}

// A component's opacity fades its border, drop shadow and glow along with its
// fill. Each effect is drawn white on a transparent fill over black, and one
// pixel of it is read at opacity 1, 0.5 and 0: full, half and nothing.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn opacity_fades_the_border_shadow_and_glow() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };
    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // A 32 × 32 quad over columns and rows 16 to 48, with a transparent fill.
    let base = QuadInstance {
        position: [0.0, 0.0, 0.5],
        size: [32.0, 32.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [0.0, 0.0, 0.0, 0.0],
        opacity: 1.0,
        corner_radii: [0; 4],
        uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
        uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
        atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 0.0, 0.0],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
    };
    let white = [1.0, 1.0, 1.0, 1.0];
    let cases = [
        (
            "border",
            // 6 px wide, inside the edge; column 19 is in the middle of it.
            QuadInstance {
                border_width: 6.0,
                border_color: white,
                border_offset: -1.0,
                ..base
            },
            19,
        ),
        (
            "drop shadow",
            // Spread 6, so a solid band 6 px wide around the quad; column 51
            // is in the middle of it, outside the quad.
            QuadInstance {
                shadow_params: [0.0, 0.0, 0.5, 6.0],
                shadow_color: white,
                ..base
            },
            51,
        ),
        (
            "glow",
            // Softness 10, no spread: a halo that fades out from the edge.
            QuadInstance {
                shadow_params: [0.0, 0.0, 10.0, 0.0],
                shadow_color: white,
                ..base
            },
            50,
        ),
    ];
    let black = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    for (name, instance, col) in cases {
        let mut red_at = |opacity: f32| -> f32 {
            let pixels = render_and_read_back(
                &device,
                &queue,
                &mut pipeline,
                &[QuadInstance {
                    opacity,
                    ..instance
                }],
                black,
            );
            pixels[(HEIGHT / 2 * BYTES_PER_ROW + col * 4) as usize] as f32
        };
        let full = red_at(1.0);
        let half = red_at(0.5);
        let none = red_at(0.0);

        assert!(full > 60.0, "{name}: visible at opacity 1, got {full}");
        assert!(
            (half - full / 2.0).abs() <= full * 0.1 + 3.0,
            "{name}: half strength at opacity 0.5, got {half} against {full}"
        );
        assert!(none <= 2.0, "{name}: nothing at opacity 0, got {none}");
    }
}

// `z` only orders components, through the order of the instances, so a
// component is drawn whatever its `z`. Passed into the clip-space depth, where
// the projection keeps only 0 to 1,000, it would clip away a component below 0
// or above 1,000, which would still take clicks.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn quads_are_drawn_whatever_their_z() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };
    let mut pipeline = QuadPipeline::new(
        &device,
        &queue,
        FORMAT,
        16,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // Four 16×16 quads side by side along the middle row, each red.
    let zs = [-1.0, 0.0, 999.0, 2000.0];
    let instances: Vec<QuadInstance> = zs
        .iter()
        .enumerate()
        .map(|(i, &z)| QuadInstance {
            position: [-24.0 + 16.0 * i as f32, 0.0, z],
            size: [16.0, 16.0],
            rotation: 0.0,
            scale: 1.0,
            anchor: [0.5, 0.5],
            color: [1.0, 0.0, 0.0, 1.0],
            opacity: 1.0,
            corner_radii: [0; 4],
            uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
            atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
            base_uv_offset: [0.0, 0.0],
            base_uv_scale: [0.0, 0.0],
            crossfade_t: 0.0,
            border_width: 0.0,
            border_color: [0.0, 0.0, 0.0, 0.0],
            border_offset: 0.0,
            shadow_params: [0.0, 0.0, 0.0, 0.0],
            shadow_color: [0.0, 0.0, 0.0, 0.0],
            base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
        })
        .collect();
    let black = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let pixels = render_and_read_back(&device, &queue, &mut pipeline, &instances, black);

    for (i, z) in zs.iter().enumerate() {
        // The center of quad `i`: columns 8 + 16i, the middle row.
        let col = 8 + 16 * i as u32;
        let off = ((HEIGHT / 2) * BYTES_PER_ROW + col * 4) as usize;
        let red = pixels[off];
        assert!(red > 200, "the quad at z = {z} wasn't drawn (red = {red})");
    }
}

// The plain pipeline draws plain components exactly as the full one does, so
// turning it on changes no pixel: here, over a mix of plain components, large
// and small, and ones with a border, shadow or glow, which the full pipeline
// still draws between them.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn the_plain_pipeline_draws_the_same_pixels() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };
    let plain = |x: f32, y: f32, w: f32, h: f32, radius: f32, color: [f32; 4]| {
        let mut q = QuadInstance {
            position: [x, y, 0.0],
            size: [w, h],
            rotation: 0.0,
            scale: 1.0,
            anchor: [0.5, 0.5],
            color,
            opacity: 1.0,
            corner_radii: [0; 4],
            uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
            atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
            base_uv_offset: [0.0, 0.0],
            base_uv_scale: [0.0, 0.0],
            crossfade_t: 0.0,
            border_width: 0.0,
            border_color: [0.0; 4],
            border_offset: 0.0,
            shadow_params: [0.0; 4],
            shadow_color: [0.0; 4],
            base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
        };
        q.set_corner_radii([radius, radius * 0.5, 0.0, radius]);
        q
    };
    let mut instances = vec![
        // A large plain one, alone between others: a run of its own.
        plain(-2.0, 2.0, 66.0, 64.0, 12.0, [0.2, 0.5, 0.9, 1.0]),
    ];
    let mut bordered = plain(10.0, -6.0, 30.0, 30.0, 8.0, [0.9, 0.3, 0.2, 1.0]);
    bordered.border_width = 3.0;
    bordered.border_color = [1.0, 1.0, 1.0, 1.0];
    bordered.border_offset = -1.0;
    instances.push(bordered);
    // Small plain ones, too small for a run of their own.
    instances.push(plain(-20.0, -20.0, 6.0, 6.0, 3.0, [0.1, 0.9, 0.4, 1.0]));
    instances.push(plain(-12.0, -20.0, 6.0, 6.0, 3.0, [0.9, 0.9, 0.1, 0.5]));
    let mut glowing = plain(16.0, 18.0, 20.0, 14.0, 6.0, [0.6, 0.2, 0.8, 1.0]);
    glowing.shadow_params = [0.0, 0.0, 6.0, 0.0];
    glowing.shadow_color = [1.0, 0.8, 0.2, 0.9];
    instances.push(glowing);
    // A translucent, rotated, faded plain one over the rest, large enough for a
    // run of its own too.
    let mut over = plain(4.0, 2.0, 70.0, 60.0, 14.0, [1.0, 0.5, 0.0, 0.6]);
    over.rotation = 0.3;
    over.opacity = 0.7;
    instances.push(over);

    let black = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let draw = |plain_pipeline: bool| {
        let mut pipeline = QuadPipeline::new(
            &device,
            &queue,
            FORMAT,
            16,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        );
        pipeline.set_plain_pipeline(&device, plain_pipeline);
        pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));
        render_and_read_back(&device, &queue, &mut pipeline, &instances, black)
    };
    let full = draw(false);
    let with_plain = draw(true);
    assert!(full.iter().any(|&b| b != 0), "the scene draws something");
    assert!(
        full == with_plain,
        "the plain pipeline changed {} bytes",
        full.iter().zip(&with_plain).filter(|(a, b)| a != b).count()
    );
}

// Skipping the pixels a later opaque component covers draws exactly the same
// pixels: here, over a stack of overlapping components, opaque and
// translucent, rounded and rotated, some with a border or shadow, drawn with
// both pipelines.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn skipping_covered_pixels_draws_the_same_pixels() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        no_gpu();
        return;
    };
    let mut instances = Vec::new();
    for i in 0..24 {
        let f = i as f32;
        let mut q = QuadInstance {
            position: [-20.0 + f * 1.7, 18.0 - f * 1.5, 0.0],
            size: [30.0 + (i % 5) as f32 * 4.0, 22.0 + (i % 3) as f32 * 6.0],
            rotation: if i % 7 == 3 { 0.4 } else { 0.0 },
            scale: 1.0,
            anchor: [0.5, 0.5],
            color: [
                0.1 + 0.035 * f,
                0.8 - 0.03 * f,
                0.5,
                if i % 6 == 5 { 0.5 } else { 1.0 },
            ],
            opacity: if i % 9 == 8 { 0.6 } else { 1.0 },
            corner_radii: [0; 4],
            uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
            uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
            atlas_page: pack_atlas_page(ATLAS_SELECTOR_MAIN, 0),
            base_uv_offset: [0.0, 0.0],
            base_uv_scale: [0.0, 0.0],
            crossfade_t: 0.0,
            border_width: 0.0,
            border_color: [0.0; 4],
            border_offset: 0.0,
            shadow_params: [0.0; 4],
            shadow_color: [0.0; 4],
            base_atlas_page: pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0),
        };
        q.set_corner_radius((i % 4) as f32 * 4.0);
        if i % 8 == 2 {
            q.border_width = 2.0;
            q.border_color = [1.0, 1.0, 1.0, 1.0];
            q.border_offset = -1.0;
        }
        if i % 10 == 4 {
            q.shadow_params = [2.0, -2.0, 3.0, 0.0];
            q.shadow_color = [0.0, 0.0, 0.0, 0.6];
        }
        instances.push(q);
    }

    let black = wgpu::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    let draw = |plain_pipeline: bool, skip_covered: bool| {
        let mut pipeline = QuadPipeline::new(
            &device,
            &queue,
            FORMAT,
            32,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        );
        pipeline.set_plain_pipeline(&device, plain_pipeline);
        pipeline.set_skip_covered_pixels(&device, skip_covered);
        pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));
        render_and_read_back(&device, &queue, &mut pipeline, &instances, black)
    };
    let drawn = draw(false, false);
    for plain_pipeline in [false, true] {
        let skipping = draw(plain_pipeline, true);
        assert!(
            drawn == skipping,
            "skipping covered pixels, with the plain pipeline {}, changed {} bytes",
            if plain_pipeline { "on" } else { "off" },
            drawn.iter().zip(&skipping).filter(|(a, b)| a != b).count()
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Render `instances` into a fresh `WIDTH × HEIGHT` offscreen texture cleared
// to `clear_color`, and read the result back as a flat RGBA8 byte buffer.
fn render_and_read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut QuadPipeline,
    instances: &[QuadInstance],
    clear_color: wgpu::Color,
) -> Vec<u8> {
    let render_target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless_rt"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let render_view = render_target.create_view(&Default::default());

    pipeline.upload_instances(queue, instances);
    // Only while skipping covered pixels is on.
    pipeline.prepare_depth(device, WIDTH, HEIGHT);

    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("headless_readback"),
        size: (BYTES_PER_ROW * HEIGHT) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("headless_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &render_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: pipeline.depth_attachment(),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pipeline.draw(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &render_target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(BYTES_PER_ROW),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = readback_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll failed");
    rx.recv().unwrap().expect("readback buffer map failed");

    let view = slice.get_mapped_range();
    view.to_vec()
}

// Like [`make_device`], but asks for `wgpu::Limits::default()` so the test can
// build an atlas larger than `downlevel_defaults`' 2048 cap. Returns `None` if
// no adapter exists *or* the one we get can't actually reach `min_dimension` —
// a software rasteriser on CI may legitimately be smaller, and that's a skip,
// not a failure.
async fn make_device_with_large_textures(
    min_dimension: u32,
) -> Option<(wgpu::Device, wgpu::Queue)> {
    let (device, queue) = make_device_with_limits(wgpu::Limits::default()).await?;
    if device.limits().max_texture_dimension_2d < min_dimension {
        return None;
    }
    Some((device, queue))
}

// Reports that the running test is skipped, and why, by the test's name, which
// the test harness gives its thread. It writes straight to stderr: the harness
// captures `eprintln!`, so a skip would otherwise look the same as a pass.
fn report_skip(why: &str) {
    use std::io::Write;
    let name = std::thread::current()
        .name()
        .unwrap_or("a GPU test")
        .to_string();
    let _ = writeln!(std::io::stderr(), "skipped: {name}: {why}");
}

// For a test with no GPU adapter: fails if `REQUIRE_GPU` is set, as on CI,
// where every platform's GPU tests must run, and otherwise reports the skip.
fn no_gpu() {
    if std::env::var("REQUIRE_GPU").is_ok() {
        panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
    }
    report_skip("no GPU adapter");
}

// Try to get a wgpu device suitable for headless rendering.
// Returns `None` if no adapter is available so the test can skip gracefully.
async fn make_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    // downlevel_defaults: permissive enough for software renderers,
    // still enforces everything we actually use.
    make_device_with_limits(wgpu::Limits::downlevel_defaults()).await
}

async fn make_device_with_limits(limits: wgpu::Limits) -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    // First try a real adapter; fall back to the software renderer (lavapipe /
    // llvmpipe). Both paths return None if nothing is available.
    let adapter = match instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
    {
        Ok(a) => a,
        Err(_) => instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                compatible_surface: None,
                force_fallback_adapter: true,
            })
            .await
            .ok()?,
    };

    eprintln!(
        "headless_render: adapter = {} ({:?})",
        adapter.get_info().name,
        adapter.get_info().backend,
    );

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("headless-test"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            memory_hints: Default::default(),
            ..Default::default()
        })
        .await
        .ok()?;

    Some((device, queue))
}
