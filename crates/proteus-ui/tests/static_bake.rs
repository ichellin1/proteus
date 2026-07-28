//! M10.5 static component baking regression tests.
//!
//! ## Test matrix
//!
//! | Test | What it guards |
//! |---|---|
//! | `bake_system_is_noop_without_gpu_resources` | Graceful no-op when GpuContext/QuadPipeline/FontAtlas are absent (headless unit-test worlds) |
//! | `bake_system_bakes_quad_and_text_composite` | Full pipeline: child despawned, BakedComposite present, own Border/color/corner_radius neutralized, collect_instances renders one stable textured quad |
//! | (same test) dynamic runtime creation | A composite `Baked` *after* the schedule has already ticked once bakes correctly too, not just one declared at startup |
//!
//! The GPU-backed test follows `proteus-render/tests/headless_render.rs`'s
//! pattern exactly: skip with a warning (not a failure) if no adapter is
//! available, so this passes in restricted CI environments the same way.

use glam::{Vec2, Vec3, Vec4};

use proteus_render::{FontAtlas, GpuContext, QuadPipeline, MAIN_ATLAS_SIZE};
use proteus_ui::{
    collect_instances, Baked, BakedComposite, BakedText, Border, ChildOf, ProteusWorld, QuadState,
    Text,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn quad_at(x: f32, y: f32, w: f32, h: f32) -> QuadState {
    QuadState {
        position: Vec3::new(x, y, 0.0),
        size: Vec2::new(w, h),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(0.2, 0.4, 0.8, 1.0),
        corner_radius: 0.0,
    }
}

/// Same shape as `proteus-render/tests/headless_render.rs::make_device` —
/// try a real adapter, fall back to the software renderer, return `None` if
/// neither is available so the caller can skip gracefully.
async fn make_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

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

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("static-bake-test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
            ..Default::default()
        })
        .await
        .ok()?;

    Some((device, queue))
}

// ---------------------------------------------------------------------------
// No-GPU graceful degradation
// ---------------------------------------------------------------------------

/// Without `GpuContext`/`QuadPipeline`/`FontAtlas` resources present (the
/// convention every other GPU-touching system in this crate already follows,
/// e.g. `one_to_n_setup_system`), `bake_system` must not panic, and must
/// leave a `Baked` entity untouched — no `BakedComposite`, children not
/// despawned — so it's naturally retried once those resources do become
/// available, rather than silently failing forever.
#[test]
fn bake_system_is_noop_without_gpu_resources() {
    let mut world = ProteusWorld::new();
    let parent = world
        .world
        .spawn((Baked, quad_at(0.0, 0.0, 100.0, 50.0)))
        .id();
    let child = world
        .world
        .spawn((quad_at(0.0, 0.0, 10.0, 10.0), ChildOf(parent)))
        .id();

    world.update(0.0);
    world.update(0.0);

    assert!(
        world.world.get::<BakedComposite>(parent).is_none(),
        "should not bake without GPU resources present"
    );
    assert!(
        world.world.get_entity(child).is_ok(),
        "child should not be despawned when baking never happened"
    );
}

// ---------------------------------------------------------------------------
// Full headless-GPU bake
// ---------------------------------------------------------------------------

/// Bakes a `Quad` parent (with a `Border`) + `Text` child into a single
/// textured quad, and separately proves a composite declared *after* the
/// schedule has already run once (dynamic runtime creation, not just
/// startup) bakes correctly too.
#[test]
fn bake_system_bakes_quad_and_text_composite() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };

    let pipeline = QuadPipeline::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, 64);
    let mut font_atlas = FontAtlas::with_embedded_font(MAIN_ATLAS_SIZE, MAIN_ATLAS_SIZE);

    let mut pw = ProteusWorld::new();
    pw.world.insert_resource(GpuContext {
        device: device.clone(),
        queue: queue.clone(),
    });
    pw.world.insert_resource(pipeline);

    // --- Composite #1: declared at "startup" (before the first tick) ---
    let parent = pw
        .world
        .spawn((
            quad_at(100.0, 50.0, 120.0, 60.0),
            Border {
                width: 2.0,
                color: Vec4::ONE,
                offset: -1.0,
            },
            Baked,
        ))
        .id();
    let child = pw
        .world
        .spawn((
            quad_at(0.0, 0.0, 40.0, 16.0),
            Text::new("Hi", 16.0),
            ChildOf(parent),
        ))
        .id();

    // Bake the child's text the same way bake_pending_text does (a shell
    // method in the reference demo, not something proteus-ui itself exposes
    // as a standalone function) — needed so gather_bake_instances captures
    // real glyph pixels, not just an untextured background.
    let region = font_atlas
        .bake_text("Hi", 16.0)
        .expect("text bake should succeed in a fresh atlas");
    pw.world.resource::<QuadPipeline>().write_to_main_atlas(
        &queue,
        region.x,
        region.y,
        region.width,
        region.height,
        &region.rgba_pixels,
    );
    pw.world.entity_mut(child).insert(BakedText {
        uv_offset: region.uv_offset(MAIN_ATLAS_SIZE),
        uv_scale: region.uv_scale(MAIN_ATLAS_SIZE),
        pixel_size: [region.width as f32, region.height as f32],
    });

    pw.world.insert_resource(font_atlas);

    // One tick bakes it — BakeFlush's ApplyDeferred runs in the same
    // schedule.run() call, so the result is visible immediately after.
    pw.update(0.0);

    assert!(
        pw.world.get_entity(child).is_err(),
        "child should be despawned after baking"
    );
    let baked = *pw
        .world
        .get::<BakedComposite>(parent)
        .expect("parent should have BakedComposite after baking");
    assert!(baked.uv_offset[0] >= 0.0 && baked.uv_offset[0] <= 1.0);
    assert!(baked.uv_offset[1] >= 0.0 && baked.uv_offset[1] <= 1.0);
    assert!(baked.uv_scale[0] > 0.0 && baked.uv_scale[1] > 0.0);
    assert!(
        (baked.pixel_size[0] - 120.0).abs() < 1.0,
        "baked region width should match the parent's own size, got {}",
        baked.pixel_size[0]
    );

    assert!(
        pw.world.get::<Border>(parent).is_none(),
        "Border should be removed — its appearance is now baked into the texture"
    );
    let qs = pw
        .world
        .get::<QuadState>(parent)
        .expect("parent keeps its own QuadState");
    assert_eq!(qs.color, Vec4::ONE, "color should be neutralized to white");
    assert_eq!(qs.corner_radius, 0.0, "corner_radius should be neutralized");
    assert!(
        (qs.position.x - 100.0).abs() < 1e-3 && (qs.position.y - 50.0).abs() < 1e-3,
        "position must be untouched by neutralization"
    );

    let instances = collect_instances(&mut pw.world);
    assert_eq!(
        instances.len(),
        1,
        "baked parent (no children left) should render as exactly one instance, got {}",
        instances.len()
    );
    assert_eq!(
        instances[0].atlas_page, 0,
        "BakedComposite lives in main_atlas"
    );
    assert_eq!(instances[0].uv_offset, baked.uv_offset);
    assert_eq!(instances[0].uv_scale, baked.uv_scale);

    // --- Composite #2: declared dynamically at runtime, after the schedule
    // has already ticked once — proves this isn't a startup-only path. ---
    let parent2 = pw
        .world
        .spawn((quad_at(-200.0, 0.0, 60.0, 30.0), Baked))
        .id();
    pw.update(0.0);

    assert!(
        pw.world.get::<BakedComposite>(parent2).is_some(),
        "a composite declared after the schedule already ran should still bake"
    );
}
