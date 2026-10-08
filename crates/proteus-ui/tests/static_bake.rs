// Tests of baking: a component with `Baked`, and the pieces of a split. The
// GPU tests skip with a warning when no adapter is available, as in
// `proteus-render`'s tests.

use glam::{Vec2, Vec3, Vec4};

use proteus_render::{
    unpack_atlas_page, AtlasConfig, FontAtlas, GpuContext, QuadPipeline, ATLAS_SELECTOR_MAIN,
    DEFAULT_TRANSITION_ATLAS_SIZE,
};
use proteus_ui::{
    collect_instances, Baked, BakedComposite, BakedText, Border, ChildOf, CompositeTextureRef,
    ProteusWorld, QuadState, Text, TextTextureRef,
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

// Same shape as `proteus-render/tests/headless_render.rs::make_device` —
// try a real adapter, fall back to the software renderer, return `None` if
// neither is available so the caller can skip gracefully.
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

// Without GPU resources, `bake_system` must not panic and must leave a `Baked`
// entity alone, with no `BakedComposite` and its children still there, so the
// bake happens once the resources exist.
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

// Bakes a `Quad` parent (with a `Border`) + `Text` child into a single
// textured quad, and separately proves a composite declared *after* the
// schedule has already run once (dynamic runtime creation, not just
// startup) bakes correctly too.
#[test]
fn bake_system_bakes_quad_and_text_composite() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };

    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    let mut font_atlas = FontAtlas::with_embedded_font();

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

    // Bake the child's text as the renderer does, so the component bake
    // includes real glyphs: rasterize, then register in the atlas.
    let glyphs = font_atlas
        .rasterize_text("Hi", 16.0, 0.0)
        .expect("text rasterize should succeed in a fresh font atlas");
    let text_texture_id = pw
        .world
        .resource_mut::<QuadPipeline>()
        .texture_registry
        .register_static(glyphs.width, glyphs.height, false)
        .expect("main_atlas should have room");
    let (placement, uv) = {
        let pipeline = pw.world.resource::<QuadPipeline>();
        (
            pipeline
                .texture_registry
                .main_atlas_region(text_texture_id)
                .unwrap(),
            pipeline
                .texture_registry
                .main_atlas_uv(text_texture_id)
                .unwrap(),
        )
    };
    pw.world
        .resource::<QuadPipeline>()
        .write_to_main_atlas(&queue, placement, &glyphs.rgba_pixels);
    pw.world.entity_mut(child).insert((
        BakedText {
            uv_offset: uv.uv_offset,
            uv_scale: uv.uv_scale,
            page: uv.page,
            pixel_size: [glyphs.width as f32, glyphs.height as f32],
        },
        TextTextureRef(text_texture_id),
    ));

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
    let (selector, page) = unpack_atlas_page(instances[0].atlas_page);
    assert_eq!(
        selector, ATLAS_SELECTOR_MAIN,
        "BakedComposite lives in main_atlas"
    );
    assert_eq!(
        page, baked.page,
        "instance must sample the page BakedComposite baked to"
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

// ---------------------------------------------------------------------------
// CompositeTextureRef reference counting, and freeing on destroy
// ---------------------------------------------------------------------------

// `bake_system` inserts `CompositeTextureRef` alongside `BakedComposite`; its
// `ComponentHooks` ref-count the `main_atlas` region against the baked
// entity's lifetime. A freshly baked entity holds the only reference (an
// explicit `free()` must be refused); despawning it decrements the ref count
// to zero (`on_replace` fires uniformly on despawn, before `on_remove`), and
// the region becomes genuinely reusable — not just marked absent.
// Text is baked by the host when it renders, after the tick. A component must
// not be baked before its subtree's text is, or the bake would leave the text
// out and destroy the children that had it. Once baked, the component's own
// text is part of the bake, so it isn't drawn again on top.
#[test]
fn a_bake_waits_for_its_text_and_draws_its_own_text_once() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };
    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    let mut pw = ProteusWorld::new();
    pw.world.insert_resource(GpuContext {
        device: device.clone(),
        queue: queue.clone(),
    });
    pw.world.insert_resource(pipeline);

    let parent = pw
        .world
        .spawn((
            quad_at(0.0, 0.0, 120.0, 60.0),
            Text::new("Card", 16.0),
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

    pw.update(0.0);
    assert!(
        pw.world.get::<BakedComposite>(parent).is_none(),
        "not baked while its text isn't"
    );
    assert!(pw.world.get_entity(child).is_ok(), "the child is kept");

    // Stand-ins for the host's text bake.
    let baked_text = BakedText {
        uv_offset: [0.0, 0.0],
        uv_scale: [0.01, 0.01],
        page: 0,
        pixel_size: [30.0, 16.0],
    };
    pw.world.entity_mut(parent).insert(baked_text.clone());
    pw.update(0.0);
    assert!(
        pw.world.get::<BakedComposite>(parent).is_none(),
        "the child's text isn't baked yet either"
    );
    pw.world.entity_mut(child).insert(baked_text);
    pw.update(0.0);

    assert!(
        pw.world.get::<BakedComposite>(parent).is_some(),
        "now baked"
    );
    assert!(pw.world.get_entity(child).is_err());
    assert!(pw.world.get::<Text>(parent).is_none());
    assert!(pw.world.get::<BakedText>(parent).is_none());
    assert_eq!(
        collect_instances(&mut pw.world).len(),
        1,
        "the bake alone, without the text drawn again over it"
    );
}

// A component larger than an atlas page can never be baked. It must be drawn
// normally instead, with its children, and not tried again every tick.
#[test]
fn a_component_larger_than_a_page_is_drawn_unbaked() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };
    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig {
            page_size: 256,
            page_count: 1,
        },
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    let mut pw = ProteusWorld::new();
    pw.world.insert_resource(GpuContext {
        device: device.clone(),
        queue: queue.clone(),
    });
    pw.world.insert_resource(pipeline);

    let parent = pw.world.spawn((quad_at(0.0, 0.0, 300.0, 40.0), Baked)).id();
    let child = pw
        .world
        .spawn((quad_at(0.0, 0.0, 20.0, 20.0), ChildOf(parent)))
        .id();
    pw.update(0.0);

    assert!(pw.world.get::<BakedComposite>(parent).is_none());
    assert!(
        pw.world.get::<Baked>(parent).is_none(),
        "unmarked, so it isn't tried again"
    );
    assert!(pw.world.get_entity(child).is_ok(), "the child is kept");
    assert_eq!(
        pw.world
            .resource::<QuadPipeline>()
            .texture_registry
            .resident_static_count(),
        0
    );
}

#[test]
fn bake_ref_counts_texture_and_frees_region_on_despawn() {
    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };

    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    let mut pw = ProteusWorld::new();
    pw.world.insert_resource(GpuContext {
        device: device.clone(),
        queue: queue.clone(),
    });
    pw.world.insert_resource(pipeline);

    let parent = pw.world.spawn((quad_at(0.0, 0.0, 50.0, 30.0), Baked)).id();
    pw.update(0.0);

    let texture_id = pw
        .world
        .get::<CompositeTextureRef>(parent)
        .expect("bake_system should insert CompositeTextureRef alongside BakedComposite")
        .0;
    assert!(
        pw.world
            .resource::<QuadPipeline>()
            .texture_registry
            .main_atlas_region(texture_id)
            .is_some(),
        "region should exist right after baking"
    );

    // The entity holds the only reference — an explicit free() must be
    // refused, not silently succeed.
    pw.world
        .resource_mut::<QuadPipeline>()
        .texture_registry
        .free(texture_id);
    assert!(
        pw.world
            .resource::<QuadPipeline>()
            .texture_registry
            .main_atlas_region(texture_id)
            .is_some(),
        "free() must refuse a still-referenced region"
    );

    // Despawn — CompositeTextureRef's on_replace hook (fires uniformly on despawn,
    // before on_remove) decrements the ref count to zero.
    pw.world.despawn(parent);

    pw.world
        .resource_mut::<QuadPipeline>()
        .texture_registry
        .free(texture_id);
    assert!(
        pw.world
            .resource::<QuadPipeline>()
            .texture_registry
            .main_atlas_region(texture_id)
            .is_none(),
        "region should be freed once despawn decremented the ref count to zero"
    );

    // And the reclaimed space is genuinely reusable, not just marked absent.
    let reused = pw
        .world
        .resource_mut::<QuadPipeline>()
        .texture_registry
        .register_static(50, 30, false);
    assert!(
        reused.is_some(),
        "freed region's atlas space should be reusable by a new registration"
    );
}

// ---------------------------------------------------------------------------
// Splits: their pieces are baked into the transition atlas
// ---------------------------------------------------------------------------

// With a GPU, a `Row` or `Grid` split bakes the source and each target into
// the transition atlas, and every piece crossfades between the two. Once the
// split completes, every region it used is free again.
#[test]
fn a_split_bakes_its_pieces_and_frees_the_transition_atlas_when_done() {
    use proteus_ui::topology::{GroupTarget, OneToNRequest, SplitStrategy};
    use proteus_ui::{BakedTexture, Easing, Lifecycle, TransitionConfig, Virtual, Visibility};

    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("static_bake: no GPU adapter available — skipping");
        return;
    };

    for strategy in [SplitStrategy::Row, SplitStrategy::Grid { cols: 2, rows: 2 }] {
        let mut pw = ProteusWorld::new();
        pw.world.insert_resource(GpuContext {
            device: device.clone(),
            queue: queue.clone(),
        });
        pw.world.insert_resource(QuadPipeline::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            64,
            AtlasConfig::default(),
            DEFAULT_TRANSITION_ATLAS_SIZE,
        ));

        let targets: Vec<GroupTarget> = (0..3)
            .map(|i| {
                let state = quad_at(-100.0 + i as f32 * 100.0, 100.0, 60.0, 60.0);
                let entity = pw.world.spawn((state.clone(), Lifecycle::Idle)).id();
                GroupTarget { entity, state }
            })
            .collect();
        let target_entities: Vec<_> = targets.iter().map(|t| t.entity).collect();
        pw.world.spawn((
            quad_at(0.0, -100.0, 300.0, 100.0),
            Lifecycle::Idle,
            OneToNRequest {
                targets,
                default_config: TransitionConfig {
                    duration: 0.5,
                    delay: 0.0,
                    easing: Easing::Linear,
                },
                child_configs: None,
                strategy: strategy.clone(),
            },
        ));

        pw.update(0.0);
        let mut pieces = pw
            .world
            .query_filtered::<Option<&BakedTexture>, bevy_ecs::query::With<Virtual>>();
        let baked: Vec<bool> = pieces.iter(&pw.world).map(|b| b.is_some()).collect();
        assert_eq!(baked, [true; 3], "{strategy:?}: every piece is baked");

        pw.update(1.0);
        pw.update(0.0);
        assert_eq!(
            pieces.iter(&pw.world).count(),
            0,
            "{strategy:?}: the pieces are gone once the split completes"
        );
        for &target in &target_entities {
            assert!(pw.world.get::<Visibility>(target).unwrap().visible);
        }
        // The largest region there is: each region has a 2-pixel border.
        let whole = DEFAULT_TRANSITION_ATLAS_SIZE - 4;
        assert!(
            pw.world
                .resource_mut::<QuadPipeline>()
                .allocate_transition_region(whole, whole)
                .is_some(),
            "{strategy:?}: the whole transition atlas is free again"
        );
    }
}
