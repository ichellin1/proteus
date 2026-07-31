//! Headless render integration test.
//!
//! Spins up a wgpu device without a surface, renders a known scene to an
//! offscreen `Rgba8Unorm` texture, reads the pixels back via a mappable buffer,
//! and asserts expected colors at specific pixel coordinates.
//!
//! If no GPU adapter is available (CI runner with no rendering support) the
//! test is skipped with a warning rather than failing. On Linux CI the test
//! requires `mesa-vulkan-drivers` (lavapipe) — see `.github/workflows/ci.yml`.

use proteus_render::{QuadInstance, QuadPipeline};

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

/// Render a 32×32 red quad into a 64×64 off-screen texture and verify:
///
///  - Center pixel (32,32) is red   — the quad covers the center quarter.
///  - Corner pixels (0,0), (63,63)  are black — the clear color.
///
/// Scene layout (with `ortho(64,64)`, 1 unit = 1 pixel, origin at center):
///
/// ```text
///  (0,0) ─────────────── (63,0)
///    │    black           │
///    │  (16,16)──(48,16)  │
///    │    │   red  │      │
///    │  (16,48)──(48,48)  │
///    │           black    │
///  (0,63)─────────────── (63,63)
/// ```
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn headless_quad_renders_to_expected_color() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("headless_render: no GPU adapter available — skipping");
        return;
    };

    // --- Pipeline ---
    let mut pipeline = QuadPipeline::new(&device, &queue, FORMAT, 16);
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
        corner_radius: 0.0, // sharp corners — no SDF rounding at edges
        uv_offset: QuadPipeline::WHITE_PIXEL_UV_OFFSET,
        uv_scale: QuadPipeline::WHITE_PIXEL_UV_SCALE,
        atlas_page: 0,
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 0.0, 0.0],
        shadow_color: [0.0, 0.0, 0.0, 0.0],
        base_atlas_page: 1,
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
    // We don't assert R == 255 exactly. The atlas sampler (linear) blends the
    // 1×1 white texel with adjacent uninitialized texels at sub-pixel UVs, so
    // the exact byte value is hardware-dependent. The meaningful check is that
    // the red channel dominates and the quad landed in the right place.
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

/// A component with a `Glow` and a texture that has a genuine transparent
/// hole in it (e.g. the reference demo's animated-logo mark, hatch gaps
/// rendered as real alpha) must let whatever is *behind* the component show
/// through that hole — not the glow's own color.
///
/// This was a latent bug in the shadow/glow SDF math: `shadow_alpha` plateaus
/// at a roughly-constant value across the component's entire interior (not
/// just near the edge), invisible for every component before this one because
/// it always sat underneath fully-opaque main content. Regression test for
/// the `shadow_alpha *= smoothstep(-1.0, 1.0, dist)` interior mask in
/// `quad.wgsl`.
#[test]
#[cfg(not(target_arch = "wasm32"))]
fn glow_does_not_leak_through_transparent_texture_holes() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("headless_render: no GPU adapter available — skipping");
        return;
    };

    let mut pipeline = QuadPipeline::new(&device, &queue, FORMAT, 16);
    pipeline.set_view_projection(&queue, QuadPipeline::ortho(WIDTH as f32, HEIGHT as f32));

    // A 4×4 fully-transparent region — every texel is (0,0,0,0), so bilinear
    // sampling anywhere within it (no neighboring non-transparent texel to
    // blend against) reads back exactly transparent, keeping this test
    // focused on the shadow/glow masking bug rather than the separate
    // premultiplied-alpha filtering fix.
    let texture_id = pipeline
        .texture_registry
        .register_static(4, 4, false)
        .expect("main_atlas has room for a 4x4 region");
    pipeline.write_to_main_atlas(&queue, 0, 0, 4, 4, &[0u8; 4 * 4 * 4]);
    let (uv_offset, uv_scale) = pipeline
        .texture_registry
        .main_atlas_uv(texture_id, proteus_render::MAIN_ATLAS_SIZE)
        .expect("just registered");

    // Same 32×32 quad footprint as the test above (rows/cols [16,48]), fully
    // untinted so the transparent texture drives main_color's alpha, with a
    // `Glow`-shaped shadow: navy at 0.8 effective alpha, softness 10 (matches
    // `proteus-shell-native`'s `hover_glow()`), zero offset/spread.
    let instances = [QuadInstance {
        position: [0.0, 0.0, 0.5],
        size: [32.0, 32.0],
        rotation: 0.0,
        scale: 1.0,
        anchor: [0.5, 0.5],
        color: [1.0, 1.0, 1.0, 1.0], // untinted
        opacity: 1.0,
        corner_radius: 0.0,
        uv_offset,
        uv_scale,
        atlas_page: 0,
        base_uv_offset: [0.0, 0.0],
        base_uv_scale: [0.0, 0.0],
        crossfade_t: 0.0,
        border_width: 0.0,
        border_color: [0.0, 0.0, 0.0, 0.0],
        border_offset: 0.0,
        shadow_params: [0.0, 0.0, 10.0, 0.0], // offset 0, softness 10, spread 0
        shadow_color: [0.0, 0.0, 0.502, 0.8], // navy @ 0.8 — proteus-shell-native's hover_glow()
        base_atlas_page: 1,
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Render `instances` into a fresh `WIDTH × HEIGHT` offscreen texture cleared
/// to `clear_color`, and read the result back as a flat RGBA8 byte buffer.
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
            depth_stencil_attachment: None,
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

/// Try to get a wgpu device suitable for headless rendering.
/// Returns `None` if no adapter is available so the test can skip gracefully.
async fn make_device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
            // downlevel_defaults: permissive enough for software renderers,
            // still enforces everything we actually use.
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
            ..Default::default()
        })
        .await
        .ok()?;

    Some((device, queue))
}
