//! Integration tests for `proteus-sdk`'s public API — a small button → list
//! app, built only against this crate's own surface (no direct `proteus-ui`
//! calls, except where noted for GPU-backed texture setup, which nothing in
//! this crate's public API can do yet — see `Proteus::world_mut`'s doc).

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Proteus, QuadState, StyleOverride, TransitionConfig};

fn quad_at(x: f32, y: f32) -> QuadState {
    QuadState {
        position: Vec3::new(x, y, 0.0),
        size: Vec2::new(100.0, 100.0),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: Vec4::new(1.0, 0.0, 0.0, 1.0),
        corner_radius: 0.0,
    }
}

fn cfg(duration: f32) -> TransitionConfig {
    TransitionConfig {
        duration,
        delay: 0.0,
        easing: proteus_sdk::linear,
    }
}

// ---------------------------------------------------------------------------
// component() / children / get()
// ---------------------------------------------------------------------------

#[test]
fn component_with_children_produces_real_hierarchy() {
    let mut app = Proteus::new();
    let item1 = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let item2 = app.component(ComponentSpec::new(quad_at(0.0, 52.0)));
    let list = app.component(
        ComponentSpec::new(quad_at(200.0, 0.0))
            .child(item1)
            .child(item2),
    );

    let data = app.get(list).expect("list should exist");
    assert_eq!(data.children.len(), 2);
    assert!(data.children.contains(&item1));
    assert!(data.children.contains(&item2));
}

#[test]
fn get_returns_none_after_destroy() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    assert!(app.get(handle).is_some());

    handle.destroy(&mut app);
    assert!(app.get(handle).is_none());
}

#[test]
fn remove_child_without_destroy_leaves_it_alive_as_a_root() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));

    list.remove_child(&mut app, item, false);

    assert_eq!(app.get(list).unwrap().children.len(), 0);
    assert!(app.get(item).is_some(), "detached child must still exist");
}

#[test]
fn remove_child_with_destroy_despawns_it() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));

    list.remove_child(&mut app, item, true);

    assert!(app.get(item).is_none());
}

// ---------------------------------------------------------------------------
// text() / image() / border() / glow() / drop_shadow() — M12.5 Step 0
// ---------------------------------------------------------------------------

#[test]
fn component_spec_attaches_text_image_border_glow_and_drop_shadow() {
    use proteus_sdk::{Border, DropShadow, Glow, Image, Text};

    let mut app = Proteus::new();
    let handle = app.component(
        ComponentSpec::new(quad_at(0.0, 0.0))
            .text(Text::new("Hello", 16.0))
            .image(Image::new(vec![0u8; 4]))
            .border(Border::new(2.0, Vec4::ONE))
            .drop_shadow(DropShadow::new(Vec2::new(2.0, 2.0), 4.0)),
    );

    assert_eq!(
        app.world()
            .get::<Text>(handle.id())
            .map(|t| t.content.clone()),
        Some("Hello".to_string()),
        "text() should attach a real Text component"
    );
    assert!(
        app.world().get::<Image>(handle.id()).is_some(),
        "image() should attach a real Image component"
    );
    assert!(
        app.world().get::<Border>(handle.id()).is_some(),
        "border() should attach a real Border component"
    );
    assert!(
        app.world().get::<DropShadow>(handle.id()).is_some(),
        "drop_shadow() should attach a real DropShadow component"
    );

    // Glow and DropShadow are mutually exclusive at the shader level — a
    // second component built with only .glow() (no .drop_shadow()) should
    // carry Glow, proving the builder itself doesn't silently drop it.
    let glowing =
        app.component(ComponentSpec::new(quad_at(300.0, 0.0)).glow(Glow::new(8.0, Vec4::ONE)));
    assert!(
        app.world().get::<Glow>(glowing.id()).is_some(),
        "glow() should attach a real Glow component when DropShadow isn't also set"
    );
}

#[test]
fn component_spec_without_optional_components_attaches_none_of_them() {
    use proteus_sdk::{Border, DropShadow, Glow, Image, Text};

    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    assert!(app.world().get::<Text>(handle.id()).is_none());
    assert!(app.world().get::<Image>(handle.id()).is_none());
    assert!(app.world().get::<Border>(handle.id()).is_none());
    assert!(app.world().get::<Glow>(handle.id()).is_none());
    assert!(app.world().get::<DropShadow>(handle.id()).is_none());
}

// ---------------------------------------------------------------------------
// on_click and friends — persistent, not one-shot
// ---------------------------------------------------------------------------

#[test]
fn on_click_fires_and_persists_across_multiple_clicks() {
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));

    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let count_clone = count.clone();
    button.on_click(&mut app, move |_app| {
        count_clone.set(count_clone.get() + 1);
    });

    // Click #1.
    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert_eq!(count.get(), 1);

    // just_pressed must have been auto-cleared by tick() — ticking again
    // with no new press must not refire.
    app.tick(1.0);
    assert_eq!(count.get(), 1);

    // Click #2 — the handler must still be registered (persistent).
    app.pointer_pressed();
    app.tick(1.0);
    assert_eq!(count.get(), 2);
}

#[test]
fn on_click_does_not_fire_for_a_miss() {
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));

    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let fired_clone = fired.clone();
    button.on_click(&mut app, move |_app| fired_clone.set(true));

    app.pointer_moved(Some(Vec2::new(900.0, 900.0)));
    app.pointer_pressed();
    app.tick(1.0);

    assert!(!fired.get());
}

#[test]
fn on_drag_reports_deltas_while_pressed() {
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));

    let last_delta = std::rc::Rc::new(std::cell::Cell::new(Vec2::ZERO));
    let last_delta_clone = last_delta.clone();
    button.on_drag(&mut app, move |_app, delta| last_delta_clone.set(delta));

    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert_eq!(last_delta.get(), Vec2::ZERO);

    app.pointer_moved(Some(Vec2::new(130.0, 90.0)));
    app.tick(1.0);
    assert_eq!(last_delta.get(), Vec2::new(30.0, -10.0));
}

// ---------------------------------------------------------------------------
// signal().set() — resolves target from the component()-declared geometry
// ---------------------------------------------------------------------------

#[test]
fn signal_set_drives_to_toward_its_declared_geometry_and_hides_from() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to_geometry = QuadState {
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        ..quad_at(300.0, 0.0)
    };
    let to = app.component(ComponentSpec::new(to_geometry.clone()));

    let signal = app.signal(None);
    signal.set(&mut app, to, from, cfg(0.1), false);

    // Big enough dt to run the transition to completion in one tick.
    app.tick(1.0);

    let to_data = app.get(to).unwrap();
    assert_eq!(to_data.geometry.color, to_geometry.color);
    assert_eq!(to_data.geometry.position, to_geometry.position);
    assert!(
        to_data.transition.is_none(),
        "transition should have settled within this one tick"
    );

    let from_data = app.get(from).unwrap();
    assert!(
        !from_data.visible,
        "from must be hidden once the morph starts"
    );
}

#[test]
fn get_reflects_transition_progress_mid_flight() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    let signal = app.signal(None);
    signal.set(&mut app, to, from, cfg(10.0), false);

    // Small dt relative to the 10s duration — should still be mid-flight.
    app.tick(1.0);

    let data = app.get(to).unwrap();
    let transition = data.transition.expect("should be mid-transition");
    assert!(transition.progress > 0.0 && transition.progress < 1.0);
    assert_eq!(transition.current, data.geometry);
}

#[test]
fn signal_on_dropped_fires_with_already_transitioning_reason() {
    let mut app = Proteus::new();
    let from1 = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    // A second, still-visible origin — the first `set()` call already hides
    // `from1` (dispatching a valid request always does), so retrying with
    // `from1` again would drop for EntityNotVisible instead of the
    // AlreadyTransitioning case this test means to exercise.
    let from2 = app.component(ComponentSpec::new(quad_at(-300.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    let signal = app.signal(None);

    let dropped_reason = std::rc::Rc::new(std::cell::RefCell::new(None));
    let dropped_reason_clone = dropped_reason.clone();
    signal.on_dropped(&mut app, move |_app, dropped| {
        *dropped_reason_clone.borrow_mut() = Some(dropped.reason);
    });

    // Long-duration transition so `to` is still Transitioning next frame.
    signal.set(&mut app, to, from1, cfg(10.0), false);
    app.tick(0.1);

    // Fire again without interruptible — must be dropped.
    signal.set(&mut app, to, from2, cfg(10.0), false);
    app.tick(0.1);

    assert_eq!(
        *dropped_reason.borrow(),
        Some(proteus_sdk::DropReason::AlreadyTransitioning)
    );
}

#[test]
fn signal_on_dropped_ignores_other_signals_drops() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    let noisy_signal = app.signal(None);
    let quiet_signal = app.signal(None);

    let quiet_fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let quiet_fired_clone = quiet_fired.clone();
    quiet_signal.on_dropped(&mut app, move |_app, _dropped| quiet_fired_clone.set(true));

    noisy_signal.set(&mut app, to, from, cfg(10.0), false);
    app.tick(0.1);
    noisy_signal.set(&mut app, to, from, cfg(10.0), false); // drops on noisy_signal
    app.tick(0.1);

    assert!(
        !quiet_fired.get(),
        "quiet_signal's handler must not fire for noisy_signal's drop"
    );
}

// ---------------------------------------------------------------------------
// bake() — no-op without GPU resources, matching bake_system's own contract
// ---------------------------------------------------------------------------

#[test]
fn bake_is_a_graceful_noop_without_gpu_resources() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).bake());

    app.tick(0.0);
    app.tick(0.0);

    // Should not panic, and the entity should still exist and be queryable —
    // matches bake_system's own documented no-GPU contract.
    assert!(app.get(handle).is_some());
}

// ---------------------------------------------------------------------------
// Interaction styles: hover triggers a mini-transition
// ---------------------------------------------------------------------------

#[test]
fn hover_style_resolves_and_applies() {
    let mut app = Proteus::new();
    let base = quad_at(100.0, 100.0);
    let button = app.component(ComponentSpec::new(base.clone()).hover(StyleOverride {
        color: Some(Vec4::new(0.0, 1.0, 0.0, 1.0)),
        ..Default::default()
    }));

    // Baseline frame (not yet hovered).
    app.pointer_moved(Some(Vec2::new(900.0, 900.0)));
    app.tick(1.0);

    // Hover in.
    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.tick(1.0);

    let data = app.get(button).unwrap();
    assert_eq!(data.state, proteus_sdk::InteractionStateKind::Hover);
    assert_eq!(data.geometry.color, Vec4::new(0.0, 1.0, 0.0, 1.0));
}

// ---------------------------------------------------------------------------
// free_resources() — GPU-backed, skipped gracefully with no adapter
// ---------------------------------------------------------------------------

/// Mirrors `crates/proteus-ui/tests/static_bake.rs`'s `make_device` helper —
/// same skip-with-a-warning-if-no-adapter convention.
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
            label: Some("proteus-sdk-test"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: Default::default(),
            ..Default::default()
        })
        .await
        .ok()?;
    Some((device, queue))
}

#[test]
fn free_resources_decrefs_and_frees_the_texture_region() {
    use proteus_render::{AtlasConfig, GpuContext, QuadPipeline};
    use proteus_ui::{BakedComposite, TextureRef};

    let Some((device, queue)) = pollster::block_on(make_device()) else {
        if std::env::var("REQUIRE_GPU").is_ok() {
            panic!("REQUIRE_GPU is set but no GPU adapter was found — check driver install");
        }
        eprintln!("proteus-sdk app test: no GPU adapter available — skipping");
        return;
    };

    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig::default(),
    );

    let mut app = Proteus::new();
    app.world_mut().insert_resource(GpuContext {
        device: device.clone(),
        queue: queue.clone(),
    });
    app.world_mut().insert_resource(pipeline);

    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).bake());
    app.tick(0.0);

    let texture_id = app
        .world()
        .get::<TextureRef>(handle.id())
        .expect("bake() should have produced a TextureRef")
        .0;
    assert!(app
        .world()
        .resource::<QuadPipeline>()
        .texture_registry
        .main_atlas_region(texture_id)
        .is_some());

    handle.free_resources(&mut app);

    assert!(
        app.world().get::<BakedComposite>(handle.id()).is_none(),
        "free_resources must remove BakedComposite"
    );
    assert!(
        app.world().get::<TextureRef>(handle.id()).is_none(),
        "free_resources must remove TextureRef"
    );

    // Region should now be genuinely freeable (ref count reached zero).
    app.world_mut()
        .resource_mut::<QuadPipeline>()
        .texture_registry
        .free(texture_id);
    assert!(
        app.world()
            .resource::<QuadPipeline>()
            .texture_registry
            .main_atlas_region(texture_id)
            .is_none(),
        "region should be freeable once free_resources decremented the ref count to zero"
    );
}

// ---------------------------------------------------------------------------
// split_to() / merge_from() — group transitions (M12.5 Step 2)
// ---------------------------------------------------------------------------

#[test]
fn split_to_bake_hides_source_and_settles_targets_to_their_declared_geometry() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let target_geometry = QuadState {
        color: Vec4::new(0.0, 1.0, 0.0, 1.0),
        ..quad_at(300.0, 0.0)
    };
    let target1 = app.component(ComponentSpec::new(target_geometry.clone()));
    let target2 = app.component(ComponentSpec::new(QuadState {
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        ..quad_at(400.0, 0.0)
    }));

    source.split_to(&mut app, &[target1, target2], cfg(0.1), SplitStrategy::Bake);
    app.tick(1.0);

    let source_data = app.get(source).unwrap();
    assert!(
        !source_data.visible,
        "source must be hidden once the 1\u{2192}N transition starts"
    );

    let target1_data = app.get(target1).unwrap();
    assert_eq!(target1_data.geometry.color, target_geometry.color);
    assert_eq!(target1_data.geometry.position, target_geometry.position);
    assert!(
        target1_data.transition.is_none(),
        "target's transition should have settled within this one large-dt tick"
    );
}

#[test]
fn merge_from_hides_sources_and_settles_destination_to_its_declared_geometry() {
    use proteus_sdk::MergeLayout;

    let mut app = Proteus::new();
    let source1 = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let source2 = app.component(ComponentSpec::new(quad_at(100.0, 0.0)));
    let dest_geometry = QuadState {
        color: Vec4::new(1.0, 0.0, 1.0, 1.0),
        ..quad_at(500.0, 0.0)
    };
    let dest = app.component(ComponentSpec::new(dest_geometry.clone()));

    dest.merge_from(
        &mut app,
        &[source1, source2],
        cfg(0.1),
        MergeLayout::Horizontal,
    );
    app.tick(1.0);

    let source1_data = app.get(source1).unwrap();
    assert!(
        !source1_data.visible,
        "sources must be hidden once the N\u{2192}1 transition starts"
    );
    let source2_data = app.get(source2).unwrap();
    assert!(!source2_data.visible);

    let dest_data = app.get(dest).unwrap();
    assert_eq!(dest_data.geometry.color, dest_geometry.color);
    assert!(
        dest_data.visible,
        "destination must be revealed once the merge completes"
    );
}
