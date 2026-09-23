//! Integration tests for `proteus-sdk`'s public API — a small button → list
//! app, built only against this crate's own surface (no direct `proteus-ui`
//! calls, except where noted for GPU-backed texture setup, which nothing in
//! this crate's public API can do yet — see `Proteus::world_mut`'s doc).

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{
    ComponentSpec, HandleError, Proteus, QuadState, StyleOverride, TransitionConfig,
};

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

    let _ = handle.destroy(&mut app);
    assert!(app.get(handle).is_none());
}

#[test]
fn remove_child_without_destroy_leaves_it_alive_as_a_root() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));

    let _ = list.remove_child(&mut app, item, false);

    assert_eq!(app.get(list).unwrap().children.len(), 0);
    assert!(app.get(item).is_some(), "detached child must still exist");
}

#[test]
fn remove_child_with_destroy_despawns_it() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));

    let _ = list.remove_child(&mut app, item, true);

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
fn non_interactive_component_does_not_shadow_a_click_on_what_it_overlaps() {
    let mut app = Proteus::new();
    // A full-viewport-like backdrop, spawned first — the worst case for hit
    // testing's "last hit wins, matches draw order" tie-break, since a
    // plain (interactive) version of this would otherwise win over
    // anything spawned earlier that it happens to overlap.
    let _backdrop = app.component(
        ComponentSpec::new(QuadState {
            size: Vec2::new(2000.0, 2000.0),
            ..quad_at(0.0, 0.0)
        })
        .non_interactive(),
    );
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));

    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let fired_clone = fired.clone();
    button.on_click(&mut app, move |_app| fired_clone.set(true));

    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);

    assert!(
        fired.get(),
        "non_interactive backdrop must not shadow the button underneath it"
    );
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
fn signal_set_from_inside_an_on_click_handler_starts_the_transition_next_tick() {
    // M7 deferred this exact combination to M12 and nothing pinned it since.
    // It is the case `callback.rs`'s take-call-put-back dispatch exists for:
    // the handler runs while the callback registry is lifted out of
    // `Proteus`, and `signal.set` re-enters that same `Proteus`.
    //
    // It works — but the transition starts on the *next* tick, because
    // `tick` runs the whole schedule and only then dispatches callbacks, so
    // the `TransitionRequest` the handler queues arrives after
    // `transition_setup_system` has already run for this frame. That one
    // frame of latency is invisible at 60fps and is what this pins; the
    // reference demo never exposed it, since every `on_click` there only
    // sets a flag the next `advance` reads.
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(QuadState {
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        ..quad_at(300.0, 0.0)
    }));

    let signal = app.signal(None);
    button.on_click(&mut app, move |app| {
        signal.set(app, to, from, cfg(10.0), false);
    });

    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);

    assert!(
        app.get(to)
            .expect("to should still exist")
            .transition
            .is_none(),
        "the handler runs after this tick's transition_setup_system, so nothing \
         should have started yet — if this starts passing, tick()'s ordering \
         changed and the latency note on Proteus::tick is stale"
    );

    app.tick(1.0);

    let transition = app
        .get(to)
        .expect("to should still exist")
        .transition
        .expect("the click handler's signal.set should have started a transition by now");
    assert!(
        transition.progress > 0.0 && transition.progress < 1.0,
        "10s transition should be mid-flight after a 1s tick, got {}",
        transition.progress
    );
    assert!(
        !app.get(from).expect("from should still exist").visible,
        "from must be hidden once the morph starts, same as a set() from outside a handler"
    );
}

#[test]
fn signal_set_reveals_to_so_a_round_trip_works() {
    // Phase A's button -> list -> button round trip. Dispatch used to hide
    // `from` without ever showing `to`, so the return leg animated an
    // invisible entity and the whole component was simply gone.
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(300.0, 0.0)).visible(false));

    let signal = app.signal(None);

    // Out: button -> list.
    signal.set(&mut app, list, button, cfg(0.1), false);
    app.tick(1.0);
    assert!(
        app.get(list).unwrap().visible,
        "list must be revealed by the morph that targets it"
    );
    assert!(!app.get(button).unwrap().visible, "button is the exit");

    // Back: list -> button. This is the leg that was impossible.
    signal.set(&mut app, button, list, cfg(0.1), false);
    app.tick(1.0);
    assert!(
        app.get(button).unwrap().visible,
        "button must come back visible, not morph invisibly"
    );
    assert!(!app.get(list).unwrap().visible, "list is now the exit");
}

#[test]
fn component_spawns_hidden_when_the_spec_says_so() {
    let mut app = Proteus::new();
    let shown = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let hidden = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).visible(false));

    assert!(app.get(shown).unwrap().visible, "visible is the default");
    assert!(!app.get(hidden).unwrap().visible);
}

#[test]
fn set_visible_toggles_an_already_spawned_component() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    handle.set_visible(&mut app, false).unwrap();
    assert!(!app.get(handle).unwrap().visible);

    handle.set_visible(&mut app, true).unwrap();
    assert!(app.get(handle).unwrap().visible);
}

#[test]
fn set_visible_on_a_destroyed_handle_is_an_error_not_a_panic() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    handle.destroy(&mut app).unwrap();

    assert!(matches!(
        handle.set_visible(&mut app, true),
        Err(HandleError::EntityNotFound)
    ));
}

#[test]
fn opacity_cascades_to_descendants_through_the_sdk() {
    // The cascade itself is M10's and tested in proteus-ui; this pins that
    // it is reachable and observable from the SDK, which it wasn't before —
    // `Opacity` could only be inserted through `world_mut()`.
    let mut app = Proteus::new();
    let child = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).opacity(0.6));
    let parent = app.component(
        ComponentSpec::new(quad_at(0.0, 0.0))
            .opacity(0.6)
            .child(child),
    );

    app.tick(0.0);

    assert_eq!(app.get(parent).unwrap().opacity, 0.6);
    assert!(
        (app.get(child).unwrap().opacity - 0.36).abs() < 1e-6,
        "child effective opacity should be 0.6 x 0.6, got {}",
        app.get(child).unwrap().opacity
    );
}

#[test]
fn a_childs_opacity_does_not_affect_its_parent() {
    let mut app = Proteus::new();
    let child = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).opacity(0.2));
    let parent = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).child(child));

    app.tick(0.0);

    assert_eq!(
        app.get(parent).unwrap().opacity,
        1.0,
        "cascade is top-down only"
    );
}

#[test]
fn set_opacity_clamps_and_defaults_to_one() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    assert_eq!(app.get(handle).unwrap().opacity, 1.0, "absent means opaque");

    handle.set_opacity(&mut app, 0.5).unwrap();
    assert_eq!(app.get(handle).unwrap().opacity, 0.5);

    handle.set_opacity(&mut app, 4.0).unwrap();
    assert_eq!(app.get(handle).unwrap().opacity, 1.0);
    handle.set_opacity(&mut app, -1.0).unwrap();
    assert_eq!(app.get(handle).unwrap().opacity, 0.0);
}

#[test]
fn fully_transparent_still_hit_tests_but_hidden_does_not() {
    // Opacity is a paint multiplier; visibility is an ECS flag. They do not
    // interact, and this is the observable consequence someone will trip
    // over: an entity faded to nothing still swallows clicks.
    let mut app = Proteus::new();
    let transparent = app.component(ComponentSpec::new(quad_at(100.0, 100.0)).opacity(0.0));

    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clicked_clone = clicked.clone();
    transparent.on_click(&mut app, move |_app| clicked_clone.set(true));

    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert!(
        clicked.get(),
        "opacity 0.0 must not remove the entity from hit-testing"
    );

    // Hiding it does — from the *next* tick. `hit_test_system` runs at the
    // start of the schedule and reads the `EffectiveVisibility` the cascade
    // wrote at the end of the previous one, so input is resolved against
    // what was last painted. Pinned from both sides so the ordering can't
    // change silently.
    clicked.set(false);
    transparent.set_visible(&mut app, false).unwrap();
    app.pointer_pressed();
    app.tick(1.0);
    assert!(
        clicked.get(),
        "the tick that hides it still hit-tests against the previous frame"
    );

    clicked.set(false);
    app.pointer_pressed();
    app.tick(1.0);
    assert!(!clicked.get(), "hidden from the next tick on");
}

#[test]
fn set_opacity_on_a_destroyed_handle_is_an_error_not_a_panic() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    handle.destroy(&mut app).unwrap();

    assert!(matches!(
        handle.set_opacity(&mut app, 0.5),
        Err(HandleError::EntityNotFound)
    ));
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
    use proteus_render::{AtlasConfig, GpuContext, QuadPipeline, DEFAULT_TRANSITION_ATLAS_SIZE};
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
        DEFAULT_TRANSITION_ATLAS_SIZE,
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

    let _ = handle.free_resources(&mut app);

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
// copy_baked_image_from() (M12.5 Step 8)
// ---------------------------------------------------------------------------

#[test]
fn copy_baked_image_from_copies_the_baked_image_and_texture_ref_onto_the_destination() {
    use proteus_render::TextureId;
    use proteus_ui::{BakedImage, TextureRef};

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let dest = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    assert!(
        !dest.copy_baked_image_from(&mut app, source).unwrap(),
        "no-op (false) while source has no BakedImage yet"
    );
    assert!(app.world().get::<BakedImage>(dest.id()).is_none());

    let baked = BakedImage {
        uv_offset: [0.1, 0.2],
        uv_scale: [0.3, 0.4],
        page: 1,
        pixel_size: [64.0, 32.0],
    };
    app.world_mut()
        .entity_mut(source.id())
        .insert((baked.clone(), TextureRef(TextureId::default())));

    assert!(dest.copy_baked_image_from(&mut app, source).unwrap());
    assert_eq!(app.world().get::<BakedImage>(dest.id()), Some(&baked));
    assert_eq!(
        app.world().get::<TextureRef>(dest.id()),
        Some(&TextureRef(TextureId::default()))
    );
    // The source's own copy is untouched — this only ever writes `dest`.
    assert_eq!(app.world().get::<BakedImage>(source.id()), Some(&baked));
}

// ---------------------------------------------------------------------------
// center_crop_to_square() (M12.5 Step 8 follow-up)
// ---------------------------------------------------------------------------

#[test]
fn center_crop_to_square_narrows_the_longer_axis_symmetrically_and_leaves_pixel_size_alone() {
    use proteus_ui::BakedImage;

    let mut app = Proteus::new();
    let entity = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    assert!(
        !entity.center_crop_to_square(&mut app).unwrap(),
        "no-op (false) with no BakedImage yet"
    );

    // Landscape (200x100, 2:1) — width should narrow to a centered half.
    app.world_mut().entity_mut(entity.id()).insert(BakedImage {
        uv_offset: [0.0, 0.0],
        uv_scale: [1.0, 1.0],
        page: 2,
        pixel_size: [200.0, 100.0],
    });
    assert!(entity.center_crop_to_square(&mut app).unwrap());
    let cropped = app.world().get::<BakedImage>(entity.id()).unwrap();
    assert_eq!(cropped.uv_scale, [0.5, 1.0]);
    assert_eq!(cropped.uv_offset, [0.25, 0.0]);
    assert_eq!(cropped.page, 2, "must preserve the atlas page");
    assert_eq!(
        cropped.pixel_size,
        [200.0, 100.0],
        "pixel_size stays the original, uncropped size"
    );

    // Portrait (100x200, 1:2) — height should narrow the same way.
    app.world_mut().entity_mut(entity.id()).insert(BakedImage {
        uv_offset: [0.0, 0.0],
        uv_scale: [1.0, 1.0],
        page: 0,
        pixel_size: [100.0, 200.0],
    });
    let _ = entity.center_crop_to_square(&mut app);
    let cropped = app.world().get::<BakedImage>(entity.id()).unwrap();
    assert_eq!(cropped.uv_scale, [1.0, 0.5]);
    assert_eq!(cropped.uv_offset, [0.0, 0.25]);

    // Square (100x100) — no-op on the UVs.
    app.world_mut().entity_mut(entity.id()).insert(BakedImage {
        uv_offset: [0.1, 0.2],
        uv_scale: [0.5, 0.5],
        page: 0,
        pixel_size: [100.0, 100.0],
    });
    let _ = entity.center_crop_to_square(&mut app);
    let cropped = app.world().get::<BakedImage>(entity.id()).unwrap();
    assert_eq!(cropped.uv_scale, [0.5, 0.5]);
    assert_eq!(cropped.uv_offset, [0.1, 0.2]);
}

// ---------------------------------------------------------------------------
// set_interactive() (M12.5.5 — theme toggle)
// ---------------------------------------------------------------------------

#[test]
fn set_interactive_toggles_whether_clicks_land() {
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let fired_clone = fired.clone();
    button.on_click(&mut app, move |_app| fired_clone.set(true));

    app.pointer_moved(Some(Vec2::new(0.0, 0.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert!(
        fired.get(),
        "interactive by default — the click should land"
    );

    fired.set(false);
    let _ = button.set_interactive(&mut app, false);
    app.pointer_moved(None);
    app.tick(1.0);
    app.pointer_moved(Some(Vec2::new(0.0, 0.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert!(
        !fired.get(),
        "set_interactive(false) must make the entity un-clickable"
    );

    let _ = button.set_interactive(&mut app, true);
    app.pointer_moved(None);
    app.tick(1.0);
    app.pointer_moved(Some(Vec2::new(0.0, 0.0)));
    app.pointer_pressed();
    app.tick(1.0);
    assert!(fired.get(), "set_interactive(true) must re-enable clicking");
}

// ---------------------------------------------------------------------------
// set_disabled / transitioning config — A-03
// ---------------------------------------------------------------------------

/// Registers a click counter and drives one click at `pos`.
fn click_at(app: &mut Proteus, pos: Vec2) {
    app.pointer_moved(Some(pos));
    app.pointer_pressed();
    app.tick(1.0);
}

#[test]
fn a_disabled_component_does_not_hit_test_but_still_reports_its_state() {
    let mut app = Proteus::new();
    let button = app.component(
        ComponentSpec::new(quad_at(100.0, 100.0)).disabled(StyleOverride {
            color: Some(Vec4::new(0.5, 0.5, 0.5, 1.0)),
            ..Default::default()
        }),
    );

    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clone = clicked.clone();
    button.on_click(&mut app, move |_app| clone.set(true));

    click_at(&mut app, Vec2::new(100.0, 100.0));
    assert!(clicked.get(), "enabled to begin with");

    clicked.set(false);
    button.set_disabled(&mut app, true).unwrap();
    click_at(&mut app, Vec2::new(100.0, 100.0));
    assert!(!clicked.get(), "disabled components are not hit-tested");
    assert_eq!(
        app.get(button).unwrap().state,
        proteus_sdk::InteractionStateKind::Disabled,
        "and the disabled style resolves, so it can look dimmed"
    );

    button.set_disabled(&mut app, false).unwrap();
    click_at(&mut app, Vec2::new(100.0, 100.0));
    assert!(clicked.get(), "re-enabling restores hit-testing");
}

#[test]
fn start_disabled_spawns_in_the_disabled_state() {
    let mut app = Proteus::new();
    let plain = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let bare = app.component(ComponentSpec::new(quad_at(0.0, 0.0)).start_disabled());
    let styled = app.component(
        ComponentSpec::new(quad_at(0.0, 0.0))
            .start_disabled()
            .disabled(StyleOverride {
                color: Some(Vec4::new(0.5, 0.5, 0.5, 1.0)),
                ..Default::default()
            }),
    );
    app.tick(0.0);

    assert!(!app.get(plain).unwrap().disabled);
    assert!(app.get(bare).unwrap().disabled);
    assert!(app.get(styled).unwrap().disabled);

    // `state` is style resolution, not the marker. Two ways it differs from
    // `disabled`, both pinned here because both will surprise someone:
    // it stays `Default` for a component that declared no styles, and it
    // lands a tick late even for one that did, since
    // `interaction_style_system` writes `InteractionState` through deferred
    // commands. `disabled` reads the marker and is true immediately.
    assert_eq!(
        app.get(styled).unwrap().state,
        proteus_sdk::InteractionStateKind::Default,
        "style resolution hasn't been applied yet on the spawn tick"
    );
    app.tick(0.0);
    assert_eq!(
        app.get(styled).unwrap().state,
        proteus_sdk::InteractionStateKind::Disabled
    );
    assert_eq!(
        app.get(bare).unwrap().state,
        proteus_sdk::InteractionStateKind::Default,
        "no declared styles, so there is nothing to resolve, ever"
    );
}

#[test]
fn allow_input_lets_a_transitioning_component_still_be_clicked() {
    use proteus_sdk::TransitioningConfig;

    let mut app = Proteus::new();
    let blocked = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    let allowed = app.component(ComponentSpec::new(quad_at(400.0, 100.0)).transitioning(
        TransitioningConfig {
            allow_input: true,
            allow_navigation: false,
        },
    ));

    let hits = std::rc::Rc::new(std::cell::Cell::new((false, false)));
    let h1 = hits.clone();
    blocked.on_click(&mut app, move |_| h1.set((true, h1.get().1)));
    let h2 = hits.clone();
    allowed.on_click(&mut app, move |_| h2.set((h2.get().0, true)));

    // Put both mid-morph with a long duration.
    let _ = blocked.animate_to(&mut app, quad_at(100.0, 100.0), cfg(10.0));
    let _ = allowed.animate_to(&mut app, quad_at(400.0, 100.0), cfg(10.0));
    app.tick(0.1);

    click_at(&mut app, Vec2::new(100.0, 100.0));
    click_at(&mut app, Vec2::new(400.0, 100.0));

    assert_eq!(
        hits.get(),
        (false, true),
        "no interaction mid-morph by default; allow_input opts back in"
    );
}

#[test]
fn set_transitioning_config_none_restores_the_default() {
    use proteus_sdk::TransitioningConfig;

    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(100.0, 100.0)).transitioning(
        TransitioningConfig {
            allow_input: true,
            allow_navigation: false,
        },
    ));
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clone = clicked.clone();
    handle.on_click(&mut app, move |_| clone.set(true));

    handle.set_transitioning_config(&mut app, None).unwrap();
    let _ = handle.animate_to(&mut app, quad_at(100.0, 100.0), cfg(10.0));
    app.tick(0.1);
    click_at(&mut app, Vec2::new(100.0, 100.0));

    assert!(
        !clicked.get(),
        "opt-in removed, so back to blocked mid-morph"
    );
}

#[test]
fn set_disabled_on_a_destroyed_handle_is_an_error_not_a_panic() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    handle.destroy(&mut app).unwrap();

    assert!(matches!(
        handle.set_disabled(&mut app, true),
        Err(HandleError::EntityNotFound)
    ));
}

// ---------------------------------------------------------------------------
// on_transition_complete — A-02
// ---------------------------------------------------------------------------

fn completion_counter(
    app: &mut Proteus,
    handle: proteus_sdk::Handle,
) -> std::rc::Rc<std::cell::Cell<u32>> {
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let clone = count.clone();
    handle.on_transition_complete(app, move |_app| clone.set(clone.get() + 1));
    count
}

#[test]
fn on_transition_complete_fires_for_animate_to() {
    let mut app = Proteus::new();
    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let count = completion_counter(&mut app, handle);

    let _ = handle.animate_to(&mut app, quad_at(300.0, 0.0), cfg(0.1));
    app.tick(1.0);
    assert_eq!(count.get(), 1);

    // Persistent, like on_click — a second transition fires it again.
    let _ = handle.animate_to(&mut app, quad_at(0.0, 0.0), cfg(0.1));
    app.tick(1.0);
    assert_eq!(count.get(), 2);
}

#[test]
fn on_transition_complete_fires_on_the_to_side_of_a_signal_set() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)).visible(false));
    let to_count = completion_counter(&mut app, to);
    let from_count = completion_counter(&mut app, from);

    let signal = app.signal(None);
    signal.set(&mut app, to, from, cfg(0.1), false);
    app.tick(1.0);

    assert_eq!(to_count.get(), 1, "the morphing side completes");
    assert_eq!(from_count.get(), 0, "the exit has no transition of its own");
}

#[test]
fn on_transition_complete_fires_once_on_the_source_of_a_slice_split() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let targets: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(300.0 + i as f32 * 100.0, 0.0))))
        .collect();
    let count = completion_counter(&mut app, source);

    let _ = source.split_to(&mut app, &targets, cfg(0.1), SplitStrategy::Slice);
    app.tick(1.0);

    assert_eq!(
        count.get(),
        1,
        "one group is one completion, not one per target"
    );
}

#[test]
fn on_transition_complete_fires_once_on_the_destination_of_a_merge() {
    use proteus_sdk::MergeLayout;

    let mut app = Proteus::new();
    let sources: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(i as f32 * 100.0, 0.0))))
        .collect();
    let dest = app.component(ComponentSpec::new(quad_at(500.0, 0.0)));
    let count = completion_counter(&mut app, dest);

    let _ = dest.merge_from(&mut app, &sources, cfg(0.1), MergeLayout::Horizontal);
    app.tick(1.0);

    assert_eq!(count.get(), 1);
}

#[test]
fn a_per_target_split_completes_on_its_targets_not_its_source() {
    use proteus_sdk::SplitStrategy;

    // PerTarget is N independent 1->1s with no virtuals, so the source has
    // nothing of its own to finish — it hides and goes Idle in the same
    // tick. Asymmetric with Slice by definition; pinned so the doc on
    // `on_transition_complete` can't quietly become wrong.
    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let target1 = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));
    let target2 = app.component(ComponentSpec::new(quad_at(400.0, 0.0)));

    let source_count = completion_counter(&mut app, source);
    let target1_count = completion_counter(&mut app, target1);
    let target2_count = completion_counter(&mut app, target2);

    let _ = source.split_to(
        &mut app,
        &[target1, target2],
        cfg(0.1),
        SplitStrategy::PerTarget,
    );

    // PerTarget needs two ticks where Slice needs one: `one_to_n_setup_system`
    // inserts each target's `TransitionRequest` through deferred commands,
    // and `transition_setup_system` shares its schedule set, so the request
    // isn't picked up until the following tick.
    app.tick(1.0);
    assert_eq!(target1_count.get(), 0, "not converted to a transition yet");
    app.tick(1.0);

    assert_eq!(
        source_count.get(),
        0,
        "the PerTarget source never transitions"
    );
    assert_eq!(target1_count.get(), 1);
    assert_eq!(target2_count.get(), 1);
}

// ---------------------------------------------------------------------------
// split_to_with_behavior / merge_from_with_behavior — A-09
// ---------------------------------------------------------------------------

#[test]
fn split_to_with_behavior_staggers_each_target() {
    use proteus_sdk::SplitStrategy;

    // Phase A's childBehavior iterator, finally reachable from the SDK.
    // Delay by index, so after 0.15s target 0 has finished its 0.1s morph
    // and target 2 (delayed 0.2s) has not started.
    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let targets: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(300.0 + i as f32 * 100.0, 0.0))))
        .collect();

    source
        .split_to_with_behavior(
            &mut app,
            &targets,
            cfg(0.1),
            SplitStrategy::PerTarget,
            |i, _total| TransitionConfig {
                duration: 0.1,
                delay: i as f32 * 0.1,
                easing: proteus_sdk::linear,
            },
        )
        .unwrap();

    // Tick 1 converts the deferred TransitionRequests (see the PerTarget
    // timing note); tick 2 advances 0.15s into them.
    app.tick(0.0);
    app.tick(0.15);

    assert!(
        app.get(targets[0]).unwrap().transition.is_none(),
        "target 0 has no delay and a 0.1s duration — done by 0.15s"
    );
    assert!(
        app.get(targets[2]).unwrap().transition.is_some(),
        "target 2 is delayed 0.2s — still waiting at 0.15s"
    );
}

#[test]
fn split_to_with_behavior_falls_back_to_the_shared_config() {
    use proteus_sdk::SplitStrategy;

    // A behavior that returns the same config for every index must behave
    // exactly like plain split_to — the eager resolution introduces nothing.
    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let targets: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(300.0 + i as f32 * 100.0, 0.0))))
        .collect();

    source
        .split_to_with_behavior(
            &mut app,
            &targets,
            cfg(0.1),
            SplitStrategy::PerTarget,
            |_i, _total| cfg(0.1),
        )
        .unwrap();
    app.tick(0.0);
    app.tick(1.0);

    for t in &targets {
        assert!(app.get(*t).unwrap().transition.is_none());
    }
}

#[test]
fn merge_from_with_behavior_staggers_each_source() {
    use proteus_sdk::MergeLayout;

    let mut app = Proteus::new();
    let sources: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(i as f32 * 100.0, 0.0))))
        .collect();
    let dest = app.component(ComponentSpec::new(quad_at(500.0, 0.0)));
    let count = completion_counter(&mut app, dest);

    dest.merge_from_with_behavior(
        &mut app,
        &sources,
        cfg(0.1),
        MergeLayout::Horizontal,
        |i, _total| TransitionConfig {
            duration: 0.1,
            delay: i as f32 * 0.1,
            easing: proteus_sdk::linear,
        },
    )
    .unwrap();

    // The group can't complete until its slowest member does — source 2 is
    // delayed 0.2s on top of a 0.1s morph.
    app.tick(0.15);
    assert_eq!(count.get(), 0, "still waiting on the staggered tail");
    app.tick(1.0);
    assert_eq!(count.get(), 1);
}

#[test]
fn with_behavior_on_a_destroyed_handle_is_an_error_not_a_panic() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let target = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));
    source.destroy(&mut app).unwrap();

    assert!(matches!(
        source.split_to_with_behavior(
            &mut app,
            &[target],
            cfg(0.1),
            SplitStrategy::PerTarget,
            |_, _| cfg(0.1),
        ),
        Err(HandleError::EntityNotFound)
    ));
}

// ---------------------------------------------------------------------------
// split_to() / merge_from() — group transitions (M12.5 Step 2)
// ---------------------------------------------------------------------------

#[test]
fn split_to_per_target_hides_source_and_settles_targets_to_their_declared_geometry() {
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

    let _ = source.split_to(
        &mut app,
        &[target1, target2],
        cfg(0.1),
        SplitStrategy::PerTarget,
    );
    app.tick(0.0);

    let source_data = app.get(source).unwrap();
    assert!(
        !source_data.visible,
        "source must be hidden once the 1\u{2192}N transition starts"
    );

    // Prove the target actually *moves*, not just that it ends up where it
    // was declared. A target sits at its declared geometry from spawn, so
    // asserting only the end state can't tell a completed transition from
    // one that never ran — which is what this test did before.
    app.tick(0.05);
    let mid = app.get(target1).unwrap();
    assert!(
        mid.transition.is_some(),
        "target should be mid-flight 0.05s into a 0.1s transition"
    );
    assert_ne!(
        mid.geometry.position, target_geometry.position,
        "mid-flight geometry must be between the source and the target"
    );

    app.tick(1.0);
    let target1_data = app.get(target1).unwrap();
    assert_eq!(target1_data.geometry.color, target_geometry.color);
    assert_eq!(target1_data.geometry.position, target_geometry.position);
    assert!(
        target1_data.transition.is_none(),
        "target's transition should have settled"
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

    let _ = dest.merge_from(
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

#[test]
fn set_declared_geometry_updates_what_a_later_split_to_settles_targets_to() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    // Spawned at one geometry (e.g. a placeholder size before its label has
    // baked), then redeclared to a different one before ever being used as
    // a transition target — mirrors a grid cell whose real size is only
    // known after baking.
    let target = app.component(ComponentSpec::new(quad_at(50.0, 50.0)));
    let real_geometry = QuadState {
        color: Vec4::new(0.0, 1.0, 1.0, 1.0),
        ..quad_at(300.0, 0.0)
    };
    let _ = target.set_declared_geometry(&mut app, real_geometry.clone());

    let _ = source.split_to(&mut app, &[target], cfg(0.1), SplitStrategy::PerTarget);
    app.tick(1.0);

    let target_data = app.get(target).unwrap();
    assert_eq!(target_data.geometry.color, real_geometry.color);
    assert_eq!(target_data.geometry.position, real_geometry.position);
}

#[test]
fn split_to_with_states_uses_the_given_state_not_declared_geometry() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    // A target whose own declared geometry (if `split_to` resolved it the
    // normal way) would be totally different from the explicit state passed
    // here — proves the explicit state actually wins.
    let target = app.component(ComponentSpec::new(quad_at(999.0, 999.0)));
    let explicit_state = QuadState {
        color: Vec4::new(0.0, 1.0, 0.0, 1.0),
        ..quad_at(300.0, 0.0)
    };

    let _ = source.split_to_with_states(
        &mut app,
        &[(target, explicit_state.clone())],
        cfg(0.1),
        SplitStrategy::PerTarget,
    );
    // Two ticks: the first turns the just-inserted `OneToNRequest` into a
    // `TransitionRequest` (`one_to_n_setup_system` and `transition_setup_
    // system` share `ProteusSet::TransitionSetup`, so a request created
    // this frame isn't picked up as an `ActiveTransition` until the next);
    // the second settles it (`cfg(0.1)`'s duration is well under this
    // tick's `dt`).
    app.tick(1.0);
    app.tick(1.0);

    let target_data = app.get(target).unwrap();
    assert_eq!(target_data.geometry.color, explicit_state.color);
    assert_eq!(target_data.geometry.position, explicit_state.position);
}

#[test]
fn split_to_with_states_is_safe_when_the_source_is_also_one_of_the_targets() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    // The self-referential case `split_to_with_states`'s own doc describes:
    // a shape splitting back into a group that includes its own slot.
    // `source`'s *current* geometry (the screen-sized shape here) must
    // survive untouched until the group-transition setup system captures it
    // as the "from" snapshot on the next tick — this call must not stomp it
    // the way `set_declared_geometry` would.
    let source = app.component(ComponentSpec::new(quad_at(500.0, 500.0)));
    let sibling = app.component(ComponentSpec::new(quad_at(999.0, 999.0)));
    let own_slot_state = QuadState {
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        ..quad_at(0.0, 0.0)
    };
    let sibling_state = QuadState {
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        ..quad_at(100.0, 0.0)
    };

    let before = app.get(source).unwrap().geometry;
    let _ = source.split_to_with_states(
        &mut app,
        &[
            (source, own_slot_state.clone()),
            (sibling, sibling_state.clone()),
        ],
        cfg(0.1),
        SplitStrategy::PerTarget,
    );
    // Source geometry must be untouched immediately after the call — the
    // request has only been inserted, not processed yet.
    assert_eq!(app.get(source).unwrap().geometry.position, before.position);

    // Two ticks — see the sibling test's comment for why.
    app.tick(1.0);
    app.tick(1.0);

    // Once settled, the target landed on the explicit state given for it.
    let source_data = app.get(source).unwrap();
    assert_eq!(source_data.geometry.color, own_slot_state.color);
    assert_eq!(source_data.geometry.position, own_slot_state.position);
    let sibling_data = app.get(sibling).unwrap();
    assert_eq!(sibling_data.geometry.position, sibling_state.position);
}

#[test]
fn animate_to_morphs_a_single_entity_with_no_second_entity_involved() {
    let mut app = Proteus::new();
    let particle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let target = QuadState {
        color: Vec4::new(0.0, 1.0, 0.0, 1.0),
        ..quad_at(400.0, 0.0)
    };

    let _ = particle.animate_to(&mut app, target.clone(), cfg(0.1));
    app.tick(1.0);

    let data = app.get(particle).unwrap();
    assert_eq!(data.geometry.color, target.color);
    assert_eq!(data.geometry.position, target.position);
    assert!(
        data.transition.is_none(),
        "should have settled within this one large-dt tick"
    );
}

#[test]
fn animate_to_can_retarget_the_same_entity_once_its_prior_transition_settles() {
    let mut app = Proteus::new();
    let particle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    let _ = particle.animate_to(&mut app, quad_at(100.0, 0.0), cfg(0.1));
    app.tick(1.0);
    assert!(app.get(particle).unwrap().transition.is_none());

    let second_target = QuadState {
        color: Vec4::new(1.0, 1.0, 0.0, 1.0),
        ..quad_at(200.0, 0.0)
    };
    let _ = particle.animate_to(&mut app, second_target.clone(), cfg(0.1));
    app.tick(1.0);

    let data = app.get(particle).unwrap();
    assert_eq!(data.geometry.position, second_target.position);
    assert_eq!(data.geometry.color, second_target.color);
}

// ---------------------------------------------------------------------------
// start_video() / stop_video() / set_video_crossfade()
// ---------------------------------------------------------------------------

#[test]
fn start_video_defaults_to_full_video_no_crossfade() {
    use proteus_ui::VideoCrossfade;

    let mut app = Proteus::new();
    let tile = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    let _ = tile.start_video(&mut app);

    let video_t = app
        .world()
        .get::<VideoCrossfade>(tile.id())
        .expect("start_video should attach VideoCrossfade")
        .video_t;
    assert_eq!(
        video_t, 1.0,
        "no crossfade by default — full video immediately"
    );
}

#[test]
fn set_video_crossfade_updates_video_t_while_playing() {
    use proteus_ui::VideoCrossfade;

    let mut app = Proteus::new();
    let tile = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let _ = tile.start_video(&mut app);

    let _ = tile.set_video_crossfade(&mut app, 0.0);
    assert_eq!(
        app.world()
            .get::<VideoCrossfade>(tile.id())
            .unwrap()
            .video_t,
        0.0
    );

    let _ = tile.set_video_crossfade(&mut app, 0.5);
    assert_eq!(
        app.world()
            .get::<VideoCrossfade>(tile.id())
            .unwrap()
            .video_t,
        0.5
    );
}

#[test]
fn set_video_crossfade_is_a_noop_before_start_video_or_after_stop_video() {
    use proteus_ui::VideoCrossfade;

    let mut app = Proteus::new();
    let tile = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    // Never started — nothing to update, and nothing should be created.
    let _ = tile.set_video_crossfade(&mut app, 0.5);
    assert!(app.world().get::<VideoCrossfade>(tile.id()).is_none());

    // Started, then stopped — same graceful no-op.
    let _ = tile.start_video(&mut app);
    let _ = tile.stop_video(&mut app);
    let _ = tile.set_video_crossfade(&mut app, 0.5);
    assert!(app.world().get::<VideoCrossfade>(tile.id()).is_none());
}

// ---------------------------------------------------------------------------
// Stale handles report, they don't panic
// ---------------------------------------------------------------------------

/// Every mutating `Handle` method used to reach `World::entity_mut`, which
/// **panics** on a despawned entity — while the docs on `Handle::from_entity`,
/// on `proteus-sdk-web`'s `Handle::from_id`, and on the TS `handleFromId`
/// all promised a stale handle would quietly do nothing. On wasm that panic
/// aborts the module: the canvas freezes and only a page reload recovers it.
///
/// Each call below is made on a handle whose entity was just destroyed. The
/// test asserts the whole sequence completes — reaching the end at all is the
/// point, since the old behavior was to abort on the first one — and that each
/// reports `EntityNotFound` rather than a silent success.
#[test]
fn every_mutating_method_on_a_destroyed_handle_reports_instead_of_panicking() {
    let mut app = Proteus::new();

    let handle = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let other = app.component(ComponentSpec::new(quad_at(50.0, 0.0)));
    let texture = app.texture(Default::default());
    handle
        .destroy(&mut app)
        .expect("first destroy should succeed");

    let geometry = quad_at(10.0, 10.0);
    let config = cfg(0.2);

    assert_eq!(
        handle.set_declared_geometry(&mut app, geometry.clone()),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.animate_to(&mut app, geometry.clone(), config),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.start_video(&mut app),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.stop_video(&mut app),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.set_video_crossfade(&mut app, 0.5),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.set_interactive(&mut app, false),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.copy_baked_image_from(&mut app, other),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.center_crop_to_square(&mut app),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.set_texture(&mut app, texture),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.split_to(
            &mut app,
            &[other],
            config,
            proteus_sdk::SplitStrategy::PerTarget
        ),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.split_to_with_states(
            &mut app,
            &[(other, geometry.clone())],
            config,
            proteus_sdk::SplitStrategy::PerTarget
        ),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.merge_from(
            &mut app,
            &[other],
            config,
            proteus_sdk::MergeLayout::Horizontal
        ),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.add_child(&mut app, other),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.remove_child(&mut app, other, false),
        Err(HandleError::EntityNotFound),
        "reported against the dead receiver, even though `other` is alive and \
         nothing below the check would have touched `self`"
    );
    assert_eq!(
        handle.free_resources(&mut app),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.destroy(&mut app),
        Err(HandleError::EntityNotFound),
        "a second destroy is reported rather than passing for a successful one"
    );

    // The world is still usable afterwards — a reported error left nothing
    // half-applied.
    assert!(app.get(other).is_some());
    assert!(app.get(handle).is_none());
}

/// A dead handle passed *into* a call on a live one is reported distinctly, so
/// a caller can tell "my handle died" from "the handle I was handed died".
#[test]
fn a_dead_handle_passed_into_a_live_one_reports_other_entity_not_found() {
    let mut app = Proteus::new();
    let live = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let dead = app.component(ComponentSpec::new(quad_at(50.0, 0.0)));
    dead.destroy(&mut app).unwrap();

    assert_eq!(
        live.add_child(&mut app, dead),
        Err(HandleError::OtherEntityNotFound)
    );
    assert_eq!(
        live.remove_child(&mut app, dead, false),
        Err(HandleError::OtherEntityNotFound)
    );
    assert_eq!(
        live.copy_baked_image_from(&mut app, dead),
        Err(HandleError::OtherEntityNotFound)
    );
    assert_eq!(
        live.split_to(
            &mut app,
            &[dead],
            cfg(0.2),
            proteus_sdk::SplitStrategy::PerTarget
        ),
        Err(HandleError::OtherEntityNotFound),
        "a group transition is all-or-nothing: one dead target fails the call \
         rather than half-running a split that can never complete"
    );
    assert_eq!(
        live.merge_from(
            &mut app,
            &[dead],
            cfg(0.2),
            proteus_sdk::MergeLayout::Horizontal
        ),
        Err(HandleError::OtherEntityNotFound)
    );

    // The live handle is untouched by any of it.
    assert!(app.get(live).is_some());
}

/// "Nothing to do" is not an error. A live component with no baked image yet
/// reports `Ok(false)` — callers poll on exactly this while an image loads, and
/// turning it into an `Err` would make a routine state look like a failure.
#[test]
fn nothing_to_do_is_ok_false_not_an_error() {
    let mut app = Proteus::new();
    let a = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let b = app.component(ComponentSpec::new(quad_at(50.0, 0.0)));

    assert_eq!(a.center_crop_to_square(&mut app), Ok(false));
    assert_eq!(a.copy_baked_image_from(&mut app, b), Ok(false));
    // No GPU pipeline in a headless world, so there is no texture to show.
    let texture = app.texture(Default::default());
    assert_eq!(a.set_texture(&mut app, texture), Ok(false));
    // Alive, but never `start_video`-ed.
    assert_eq!(a.set_video_crossfade(&mut app, 0.5), Ok(false));
}

/// `component()`'s declarative `children` is the same contract as
/// `Handle::add_child`: a dead child is skipped, not a panic. The surviving
/// children still attach.
#[test]
fn component_skips_a_dead_child_instead_of_panicking() {
    let mut app = Proteus::new();
    let live_child = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let dead_child = app.component(ComponentSpec::new(quad_at(10.0, 0.0)));
    dead_child.destroy(&mut app).unwrap();

    let parent = app.component(
        ComponentSpec::new(quad_at(100.0, 100.0))
            .child(live_child)
            .child(dead_child),
    );

    let data = app.get(parent).expect("parent should exist");
    assert_eq!(
        data.children.len(),
        1,
        "only the live child attaches; the dead one is skipped"
    );
    assert_eq!(data.children[0], live_child);
}

// ---------------------------------------------------------------------------
// set_declared_geometry keeps interaction styling in sync (audit C-11)
// ---------------------------------------------------------------------------

/// `interaction_style_system` resolves hover/pressed/focused overrides against
/// its *own* snapshot of the rest state, captured the first frame it saw the
/// entity — it can't read `DeclaredGeometry` (private to `proteus-sdk`). So
/// `set_declared_geometry` has to update that snapshot too, or returning to
/// `Default` snaps the component back to its spawn geometry.
///
/// Exactly the case the method exists for: a component whose real resting
/// layout is only known after spawn — e.g. a cell sized from its own baked
/// label — is precisely the one that would snap.
#[test]
fn set_declared_geometry_updates_what_hover_returns_to() {
    let spawn = quad_at(0.0, 0.0);
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(spawn.clone()).hover(StyleOverride {
        scale: Some(2.0),
        ..Default::default()
    }));

    // Frame 1 captures the declared baseline.
    app.tick(0.016);

    // Rest layout is only now known — e.g. measured from baked content.
    let relaid_out = quad_at(500.0, 250.0);
    button
        .set_declared_geometry(&mut app, relaid_out.clone())
        .unwrap();

    // Hover, settle, then leave and settle again.
    app.pointer_moved(Some(Vec2::new(500.0, 250.0)));
    app.tick(1.0);
    app.tick(1.0);
    app.pointer_moved(Some(Vec2::new(5000.0, 5000.0)));
    app.tick(1.0);
    app.tick(1.0);

    let settled = app.get(button).unwrap().geometry;
    assert_eq!(
        settled.position, relaid_out.position,
        "leaving hover must return to the geometry declared after spawn, not the \
         one captured at spawn"
    );
    assert!(
        (settled.scale - 1.0).abs() < 1e-5,
        "and the hover scale must be undone, got {}",
        settled.scale
    );
}
