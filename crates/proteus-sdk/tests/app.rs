// Integration tests for `proteus-sdk`'s public API, written against this
// crate's own surface. The exceptions use `proteus-ui` or `proteus-render`
// directly to set up GPU resources, which the public API can't do.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{
    ComponentSpec, HandleError, ImageCrop, Proteus, QuadState, StyleOverride, Text,
    TransitionConfig,
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
        easing: proteus_sdk::Easing::Linear,
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
fn remove_child_leaves_it_alive_as_a_root() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));

    list.remove_child(&mut app, item).unwrap();

    assert_eq!(app.get(list).unwrap().children.len(), 0);
    assert!(app.get(item).is_some(), "detached child must still exist");
}

// Removing another component's child must fail and leave it where it is.
// Without the check, `list_a.remove_child(item_of_b)` would detach the item
// from `list_b` and report success.
#[test]
fn remove_child_refuses_a_component_that_isnt_its_child() {
    let mut app = Proteus::new();
    let item = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list_a = app.component(ComponentSpec::new(quad_at(-200.0, 0.0)));
    let list_b = app.component(ComponentSpec::new(quad_at(200.0, 0.0)).child(item));
    let loose = app.component(ComponentSpec::new(quad_at(0.0, 200.0)));

    assert_eq!(
        list_a.remove_child(&mut app, item),
        Err(HandleError::NotAChild)
    );
    assert_eq!(
        app.get(list_b).unwrap().children.len(),
        1,
        "item stays in list_b"
    );
    assert_eq!(
        list_a.remove_child(&mut app, loose),
        Err(HandleError::NotAChild)
    );
}

// ---------------------------------------------------------------------------
// text() / image() / border() / glow() / drop_shadow()
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
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    // A full-window backdrop drawn over the button. Without
    // `non_interactive()` it would take the click.
    let _backdrop = app.component(
        ComponentSpec::new(QuadState {
            size: Vec2::new(2000.0, 2000.0),
            ..quad_at(0.0, 0.0)
        })
        .non_interactive(),
    );

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
fn a_disabled_component_absorbs_a_click_on_what_it_overlaps() {
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    // Drawn over the button. Disabled, it is inert but still there, so it
    // takes the click and fires nothing.
    let cover = app.component(
        ComponentSpec::new(QuadState {
            size: Vec2::new(400.0, 400.0),
            ..quad_at(100.0, 100.0)
        })
        .start_disabled(),
    );

    let button_clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clone = button_clicked.clone();
    button.on_click(&mut app, move |_app| clone.set(true));
    let cover_clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clone = cover_clicked.clone();
    cover.on_click(&mut app, move |_app| clone.set(true));

    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.pointer_pressed();
    app.tick(1.0);

    assert!(
        !button_clicked.get(),
        "the disabled cover blocks the button"
    );
    assert!(!cover_clicked.get(), "and fires no click of its own");
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
// transition_channel().set() — resolves target from the component()-declared geometry
// ---------------------------------------------------------------------------

#[test]
fn channel_set_drives_to_toward_its_declared_geometry_and_hides_from() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to_geometry = QuadState {
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        ..quad_at(300.0, 0.0)
    };
    let to = app.component(ComponentSpec::new(to_geometry.clone()));

    let channel = app.transition_channel(None);
    channel.set(&mut app, to, from, cfg(0.1), false);

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
fn channel_set_from_inside_an_on_click_handler_starts_the_transition_next_tick() {
    // A handler that calls `channel.set` re-enters the `Proteus` that is
    // dispatching it, the case `callback.rs`'s take-call-put-back dispatch
    // exists for.
    //
    // The transition starts on the next tick, not this one: `tick` runs the
    // whole update before dispatching callbacks, so the request the handler
    // queues arrives after this tick's transition setup has run. The one-tick
    // delay is deliberate, and this test pins it.
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(QuadState {
        color: Vec4::new(0.0, 0.0, 1.0, 1.0),
        ..quad_at(300.0, 0.0)
    }));

    let channel = app.transition_channel(None);
    button.on_click(&mut app, move |app| {
        channel.set(app, to, from, cfg(10.0), false);
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
        .expect("the click handler's channel.set should have started a transition by now");
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
fn channel_set_reveals_to_so_a_round_trip_works() {
    // A button -> list -> button round trip. The return leg only works because
    // dispatch shows `to` as well as hiding `from`; without that, the button
    // would transition while invisible and never reappear.
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let list = app.component(ComponentSpec::new(quad_at(300.0, 0.0)).visible(false));

    let channel = app.transition_channel(None);

    // Out: button -> list.
    channel.set(&mut app, list, button, cfg(0.1), false);
    app.tick(1.0);
    assert!(
        app.get(list).unwrap().visible,
        "list must be revealed by the morph that targets it"
    );
    assert!(!app.get(button).unwrap().visible, "button is the exit");

    // Back: list -> button.
    channel.set(&mut app, button, list, cfg(0.1), false);
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
    // The opacity cascade itself is tested in proteus-ui; this checks that it
    // can be set and read through the SDK.
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
    // Opacity only affects drawing, so a component faded to nothing still
    // receives clicks. Deliberate, and easy to trip over.
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

    // Hiding it does stop clicks, from the next tick: input is matched against
    // what was last drawn. Checked on both ticks so the ordering can't change
    // unnoticed.
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

    let channel = app.transition_channel(None);
    channel.set(&mut app, to, from, cfg(10.0), false);

    // Small dt relative to the 10s duration — should still be mid-flight.
    app.tick(1.0);

    let data = app.get(to).unwrap();
    let transition = data.transition.expect("should be mid-transition");
    assert!(transition.progress > 0.0 && transition.progress < 1.0);
    assert_eq!(transition.current, data.geometry);
}

#[test]
fn channel_on_dropped_fires_with_already_transitioning_reason() {
    let mut app = Proteus::new();
    let from1 = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    // A second, still-visible origin — the first `set()` call already hides
    // `from1` (dispatching a valid request always does), so retrying with
    // `from1` again would drop for EntityNotVisible instead of the
    // AlreadyTransitioning case this test means to exercise.
    let from2 = app.component(ComponentSpec::new(quad_at(-300.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    let channel = app.transition_channel(None);

    let dropped_reason = std::rc::Rc::new(std::cell::RefCell::new(None));
    let dropped_reason_clone = dropped_reason.clone();
    channel.on_dropped(&mut app, move |_app, dropped| {
        *dropped_reason_clone.borrow_mut() = Some(dropped.reason);
    });

    // Long-duration transition so `to` is still Transitioning next tick.
    channel.set(&mut app, to, from1, cfg(10.0), false);
    app.tick(0.1);

    // Fire again without interruptible — must be dropped.
    channel.set(&mut app, to, from2, cfg(10.0), false);
    app.tick(0.1);

    assert_eq!(
        *dropped_reason.borrow(),
        Some(proteus_sdk::DropReason::AlreadyTransitioning)
    );
}

#[test]
fn channel_on_dropped_ignores_other_channels_drops() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    let noisy_channel = app.transition_channel(None);
    let quiet_channel = app.transition_channel(None);

    let quiet_fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let quiet_fired_clone = quiet_fired.clone();
    quiet_channel.on_dropped(&mut app, move |_app, _dropped| quiet_fired_clone.set(true));

    noisy_channel.set(&mut app, to, from, cfg(10.0), false);
    app.tick(0.1);
    noisy_channel.set(&mut app, to, from, cfg(10.0), false); // drops on noisy_channel
    app.tick(0.1);

    assert!(
        !quiet_fired.get(),
        "quiet_channel's handler must not fire for noisy_channel's drop"
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

    // Without a GPU nothing is baked, but the component must still exist and
    // be readable.
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

    // Baseline tick (not yet hovered).
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

// Creates a headless GPU device, or returns `None` if this machine has no
// adapter. Tests that need a GPU skip with a warning in that case.
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

// A `Proteus` with the GPU resources a host would install, or `None` when
// this machine has no adapter.
fn gpu_app() -> Option<Proteus> {
    use proteus_render::{AtlasConfig, GpuContext, QuadPipeline, DEFAULT_TRANSITION_ATLAS_SIZE};

    let (device, queue) = pollster::block_on(make_device())?;
    let pipeline = QuadPipeline::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        64,
        AtlasConfig::default(),
        DEFAULT_TRANSITION_ATLAS_SIZE,
    );
    let mut app = Proteus::new();
    app.world_mut()
        .insert_resource(GpuContext { device, queue });
    app.world_mut().insert_resource(pipeline);
    Some(app)
}

// Skips with a message, unless `REQUIRE_GPU` is set, in which case it panics.
// CI always has a software GPU (lavapipe), so a missing adapter there means a
// broken driver install.
macro_rules! gpu_app_or_skip {
    () => {
        match gpu_app() {
            Some(app) => app,
            None => {
                if std::env::var("REQUIRE_GPU").is_ok() {
                    panic!("REQUIRE_GPU is set but no GPU adapter was found");
                }
                eprintln!("proteus-sdk app test: no GPU adapter available — skipping");
                return;
            }
        }
    };
}

#[test]
fn bake_texture_registers_pixels_and_returns_a_usable_handle() {
    use proteus_sdk::TextureRequest;

    let mut app = gpu_app_or_skip!();
    let rgba = vec![255u8; 8 * 8 * 4];

    let texture = app.bake_texture(8, 8, rgba, TextureRequest::default());
    let (kind, w, h) = texture.state(&app).expect("texture should be registered");

    assert_eq!((w, h), (8, 8));
    assert_eq!(kind, proteus_render::TextureKind::Static);
}

#[test]
fn bake_texture_honours_the_max_side_cap() {
    use proteus_sdk::TextureRequest;

    let mut app = gpu_app_or_skip!();
    let rgba = vec![128u8; 32 * 16 * 4];

    let texture = app.bake_texture(
        32,
        16,
        rgba,
        TextureRequest {
            max_side: Some(8),
            ..Default::default()
        },
    );
    let (_, w, h) = texture.state(&app).expect("texture should be registered");

    assert_eq!(
        (w, h),
        (8, 4),
        "downscaled to the cap, aspect preserved, before packing"
    );
}

#[test]
fn load_texture_decodes_encoded_bytes() {
    use proteus_sdk::TextureRequest;

    let mut app = gpu_app_or_skip!();

    // A 2x2 opaque-red RGBA PNG, inline so the test needs neither an asset
    // file nor an encoder dev-dependency. Header says 2x2, colour type 6
    // (RGBA), bit depth 8; the IDAT is zlib-compressed scanlines.
    const RED_2X2_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72,
        0xb6, 0x0d, 0x24, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x84, 0x19, 0x60, 0x0c, 0x00, 0x47, 0xca, 0x07, 0xf9, 0x67, 0x59,
        0x6e, 0xb7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    let texture = app
        .load_texture(RED_2X2_PNG, TextureRequest::default())
        .expect("valid PNG should decode");
    let (_, w, h) = texture.state(&app).expect("texture should be registered");
    assert_eq!((w, h), (2, 2));
}

#[test]
fn load_texture_returns_none_for_undecodable_bytes() {
    use proteus_sdk::TextureRequest;

    let mut app = gpu_app_or_skip!();
    assert!(app
        .load_texture(b"not an image", TextureRequest::default())
        .is_none());
}

#[test]
fn bake_texture_without_gpu_resources_yields_a_null_handle() {
    use proteus_sdk::TextureRequest;

    // No GPU needed: this is the headless degradation path.
    let mut app = Proteus::new();
    let texture = app.bake_texture(4, 4, vec![0u8; 4 * 4 * 4], TextureRequest::default());

    assert!(
        texture.state(&app).is_none(),
        "a null handle resolves to no texture rather than panicking"
    );
}

#[test]
fn free_resources_decrefs_and_frees_the_texture_region() {
    use proteus_render::{AtlasConfig, GpuContext, QuadPipeline, DEFAULT_TRANSITION_ATLAS_SIZE};
    use proteus_ui::{BakedComposite, CompositeTextureRef};

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
        .get::<CompositeTextureRef>(handle.id())
        .expect("bake() should have produced a CompositeTextureRef")
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
        app.world()
            .get::<CompositeTextureRef>(handle.id())
            .is_none(),
        "free_resources must remove CompositeTextureRef"
    );

    // The `Baked` marker went too, so the component isn't baked again.
    app.tick(0.0);
    assert!(
        app.world().get::<BakedComposite>(handle.id()).is_none(),
        "free_resources must stay freed, not be baked again"
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

// Text and an image are removed too, so the host has nothing to bake again.
#[test]
fn free_resources_removes_text_and_images_so_nothing_is_baked_again() {
    use proteus_ui::Image;

    let mut app = Proteus::new();
    let handle = app.component(
        ComponentSpec::new(quad_at(0.0, 0.0))
            .text(Text::new("Hello", 16.0))
            .image(Image::new(vec![0u8; 4])),
    );

    handle.free_resources(&mut app).unwrap();

    assert!(app.world().get::<Text>(handle.id()).is_none());
    assert!(app.world().get::<Image>(handle.id()).is_none());
}

#[test]
fn set_text_and_set_image_replace_the_content_and_its_bake() {
    use proteus_render::TextureId;
    use proteus_ui::{BakedImage, BakedText, Image, ImageTextureRef, TextTextureRef};

    let mut app = Proteus::new();
    let handle = app.component(
        ComponentSpec::new(quad_at(0.0, 0.0))
            .text(Text::new("Old", 16.0))
            .image(Image::new(vec![0u8; 4])),
    );
    // Stand-ins for what the host's bake adds.
    app.world_mut().entity_mut(handle.id()).insert((
        BakedText {
            uv_offset: [0.0, 0.0],
            uv_scale: [0.1, 0.1],
            page: 0,
            pixel_size: [30.0, 16.0],
        },
        TextTextureRef(TextureId::default()),
        BakedImage::new([0.0, 0.0], [0.1, 0.1], 0, [4.0, 4.0]),
        ImageTextureRef(TextureId::default()),
    ));

    handle.set_text(&mut app, Text::new("New", 16.0)).unwrap();
    assert_eq!(app.world().get::<Text>(handle.id()).unwrap().content, "New");
    assert!(
        app.world().get::<BakedText>(handle.id()).is_none(),
        "the old bake goes, so the host bakes the new text"
    );
    assert!(app.world().get::<TextTextureRef>(handle.id()).is_none());
    assert!(
        app.world().get::<BakedImage>(handle.id()).is_some(),
        "the image is untouched"
    );

    handle
        .set_image(&mut app, Image::new(vec![1u8; 4]))
        .unwrap();
    assert_eq!(
        &*app.world().get::<Image>(handle.id()).unwrap().bytes,
        &[1u8; 4]
    );
    assert!(app.world().get::<BakedImage>(handle.id()).is_none());
    assert!(app.world().get::<ImageTextureRef>(handle.id()).is_none());
}

// ---------------------------------------------------------------------------
// copy_baked_image_from()
// ---------------------------------------------------------------------------

#[test]
fn copy_baked_image_from_copies_the_baked_image_and_texture_ref_onto_the_destination() {
    use proteus_render::TextureId;
    use proteus_ui::{BakedImage, ImageTextureRef};

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let dest = app.component(ComponentSpec::new(quad_at(300.0, 0.0)));

    assert!(
        !dest.copy_baked_image_from(&mut app, source).unwrap(),
        "no-op (false) while source has no BakedImage yet"
    );
    assert!(app.world().get::<BakedImage>(dest.id()).is_none());

    let baked = BakedImage::new([0.1, 0.2], [0.3, 0.4], 1, [64.0, 32.0]);
    app.world_mut()
        .entity_mut(source.id())
        .insert((baked.clone(), ImageTextureRef(TextureId::default())));

    assert!(dest.copy_baked_image_from(&mut app, source).unwrap());
    assert_eq!(app.world().get::<BakedImage>(dest.id()), Some(&baked));
    assert_eq!(
        app.world().get::<ImageTextureRef>(dest.id()),
        Some(&ImageTextureRef(TextureId::default()))
    );
    // The source's own copy is untouched — this only ever writes `dest`.
    assert_eq!(app.world().get::<BakedImage>(source.id()), Some(&baked));
}

// ---------------------------------------------------------------------------
// crop_image()
// ---------------------------------------------------------------------------

#[test]
fn a_centered_square_crop_narrows_the_longer_axis_and_leaves_pixel_size_alone() {
    use proteus_ui::BakedImage;

    let mut app = Proteus::new();
    let entity = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));

    assert!(
        !entity
            .crop_image(&mut app, ImageCrop::CenteredSquare)
            .unwrap(),
        "no-op (false) with no BakedImage yet"
    );

    // Landscape (200x100, 2:1) — width should narrow to a centered half.
    app.world_mut()
        .entity_mut(entity.id())
        .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 2, [200.0, 100.0]));
    assert!(entity
        .crop_image(&mut app, ImageCrop::CenteredSquare)
        .unwrap());
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
    app.world_mut()
        .entity_mut(entity.id())
        .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [100.0, 200.0]));
    let _ = entity.crop_image(&mut app, ImageCrop::CenteredSquare);
    let cropped = app.world().get::<BakedImage>(entity.id()).unwrap();
    assert_eq!(cropped.uv_scale, [1.0, 0.5]);
    assert_eq!(cropped.uv_offset, [0.0, 0.25]);

    // Square (100x100) — no-op on the UVs.
    app.world_mut()
        .entity_mut(entity.id())
        .insert(BakedImage::new([0.1, 0.2], [0.5, 0.5], 0, [100.0, 100.0]));
    let _ = entity.crop_image(&mut app, ImageCrop::CenteredSquare);
    let cropped = app.world().get::<BakedImage>(entity.id()).unwrap();
    assert_eq!(cropped.uv_scale, [0.5, 0.5]);
    assert_eq!(cropped.uv_offset, [0.1, 0.2]);
}

// Cropping must start from the whole image each time. Otherwise a second crop
// crops the first: a 2:1 image would go to a centered half, then a quarter.
#[test]
fn cropping_again_replaces_the_crop_and_none_restores_the_whole_image() {
    use proteus_ui::BakedImage;

    let mut app = Proteus::new();
    let entity = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    app.world_mut()
        .entity_mut(entity.id())
        .insert(BakedImage::new([0.0, 0.0], [1.0, 1.0], 0, [200.0, 100.0]));
    let region = |app: &Proteus| {
        let b = app.world().get::<BakedImage>(entity.id()).unwrap();
        (b.uv_offset, b.uv_scale)
    };

    entity
        .crop_image(&mut app, ImageCrop::CenteredSquare)
        .unwrap();
    entity
        .crop_image(&mut app, ImageCrop::CenteredSquare)
        .unwrap();
    assert_eq!(region(&app), ([0.25, 0.0], [0.5, 1.0]), "not compounded");

    entity.crop_image(&mut app, ImageCrop::None).unwrap();
    assert_eq!(region(&app), ([0.0, 0.0], [1.0, 1.0]));
}

#[test]
fn aspect_and_rect_crops_select_the_expected_region() {
    use proteus_ui::BakedImage;

    let mut app = Proteus::new();
    let entity = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    // A 100×200 portrait image, at an offset in the atlas.
    app.world_mut()
        .entity_mut(entity.id())
        .insert(BakedImage::new([0.2, 0.2], [0.4, 0.4], 0, [100.0, 200.0]));
    let region = |app: &Proteus| {
        let b = app.world().get::<BakedImage>(entity.id()).unwrap();
        (b.uv_offset, b.uv_scale)
    };

    // 1:1 kept to the top: the top half of the image.
    let top = ImageCrop::Aspect {
        ratio: 1.0,
        anchor: glam::Vec2::new(0.5, 0.0),
    };
    entity.crop_image(&mut app, top).unwrap();
    assert_eq!(region(&app), ([0.2, 0.2], [0.4, 0.2]));

    // The right half, given explicitly.
    let right = ImageCrop::Rect {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    entity.crop_image(&mut app, right).unwrap();
    assert_eq!(region(&app), ([0.4, 0.2], [0.2, 0.4]));
}

// ---------------------------------------------------------------------------
// set_interactive()
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
// set_disabled() / transitioning config
// ---------------------------------------------------------------------------

// Registers a click counter and drives one click at `pos`.
fn click_at(app: &mut Proteus, pos: Vec2) {
    app.pointer_moved(Some(pos));
    app.pointer_pressed();
    app.tick(1.0);
}

#[test]
fn a_disabled_component_ignores_clicks_but_still_reports_its_state() {
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
    assert!(!clicked.get(), "disabled components fire no clicks");
    assert_eq!(
        app.get(button).unwrap().state,
        proteus_sdk::InteractionStateKind::Disabled,
        "and the disabled style resolves, so it can look dimmed"
    );

    button.set_disabled(&mut app, false).unwrap();
    click_at(&mut app, Vec2::new(100.0, 100.0));
    assert!(clicked.get(), "re-enabling restores clicks");
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

    // `state` is the interaction style applied, not whether the component is
    // disabled. It differs from `disabled` in two ways, both pinned here: it
    // stays `Default` for a component with no styles, and it updates a tick
    // late even for one with styles. `disabled` is true immediately.
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
fn allow_pointer_lets_a_transitioning_component_still_be_clicked() {
    use proteus_sdk::TransitionInteractionConfig;

    let mut app = Proteus::new();
    let blocked = app.component(ComponentSpec::new(quad_at(100.0, 100.0)));
    let allowed = app.component(
        ComponentSpec::new(quad_at(400.0, 100.0)).transition_interaction(
            TransitionInteractionConfig {
                allow_pointer: true,
                allow_navigation: false,
            },
        ),
    );

    let hits = std::rc::Rc::new(std::cell::Cell::new((false, false)));
    let h1 = hits.clone();
    blocked.on_click(&mut app, move |_| h1.set((true, h1.get().1)));
    let h2 = hits.clone();
    allowed.on_click(&mut app, move |_| h2.set((h2.get().0, true)));

    // Put both mid-transition with a long duration.
    let _ = blocked.animate_to(&mut app, quad_at(100.0, 100.0), cfg(10.0));
    let _ = allowed.animate_to(&mut app, quad_at(400.0, 100.0), cfg(10.0));
    app.tick(0.1);

    click_at(&mut app, Vec2::new(100.0, 100.0));
    click_at(&mut app, Vec2::new(400.0, 100.0));

    assert_eq!(
        hits.get(),
        (false, true),
        "no interaction mid-morph by default; allow_pointer opts back in"
    );
}

#[test]
fn set_transition_interaction_none_restores_the_default() {
    use proteus_sdk::TransitionInteractionConfig;

    let mut app = Proteus::new();
    let handle = app.component(
        ComponentSpec::new(quad_at(100.0, 100.0)).transition_interaction(
            TransitionInteractionConfig {
                allow_pointer: true,
                allow_navigation: false,
            },
        ),
    );
    let clicked = std::rc::Rc::new(std::cell::Cell::new(false));
    let clone = clicked.clone();
    handle.on_click(&mut app, move |_| clone.set(true));

    handle.set_transition_interaction(&mut app, None).unwrap();
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
// on_transition_complete()
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
fn on_transition_complete_fires_on_the_to_side_of_a_channel_set() {
    let mut app = Proteus::new();
    let from = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let to = app.component(ComponentSpec::new(quad_at(300.0, 0.0)).visible(false));
    let to_count = completion_counter(&mut app, to);
    let from_count = completion_counter(&mut app, from);

    let channel = app.transition_channel(None);
    channel.set(&mut app, to, from, cfg(0.1), false);
    app.tick(1.0);

    assert_eq!(to_count.get(), 1, "the morphing side completes");
    assert_eq!(from_count.get(), 0, "the exit has no transition of its own");
}

#[test]
fn on_transition_complete_fires_once_on_the_source_of_a_row_split() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let targets: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(300.0 + i as f32 * 100.0, 0.0))))
        .collect();
    let count = completion_counter(&mut app, source);

    let _ = source.split_to(&mut app, &targets, cfg(0.1), SplitStrategy::Row);
    app.tick(1.0);

    assert_eq!(
        count.get(),
        1,
        "one group is one completion, not one per target"
    );
}

#[test]
fn a_hover_style_doesnt_block_a_click_or_count_as_a_transition() {
    let mut app = Proteus::new();
    let button = app.component(
        ComponentSpec::new(quad_at(100.0, 100.0)).hover(StyleOverride {
            color: Some(Vec4::new(0.5, 0.5, 0.5, 1.0)),
            ..Default::default()
        }),
    );
    let completions = completion_counter(&mut app, button);
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let clone = clicks.clone();
    button.on_click(&mut app, move |_app| clone.set(clone.get() + 1));
    let hovers = std::rc::Rc::new(std::cell::Cell::new(0));
    let clone = hovers.clone();
    button.on_hover_enter(&mut app, move |_app| clone.set(clone.get() + 1));

    app.tick(1.0 / 60.0);
    app.pointer_moved(Some(Vec2::new(100.0, 100.0)));
    app.tick(1.0 / 60.0);
    assert!(
        app.get(button).unwrap().transition.is_none(),
        "the hover animation isn't reported as a transition"
    );

    // Partway through the 0.15 s hover animation.
    app.pointer_pressed();
    app.tick(1.0 / 60.0);
    app.pointer_released();
    for _ in 0..30 {
        app.tick(1.0 / 60.0);
    }

    assert_eq!(clicks.get(), 1, "a click during the hover animation lands");
    assert_eq!(hovers.get(), 1, "and the hover isn't interrupted");
    assert_eq!(completions.get(), 0, "style changes don't count");
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

    let _ = dest.merge_from(&mut app, &sources, cfg(0.1), MergeLayout::Row);
    app.tick(1.0);

    assert_eq!(count.get(), 1);
}

#[test]
fn a_per_target_split_completes_on_its_targets_not_its_source() {
    use proteus_sdk::SplitStrategy;

    // PerTarget runs one independent 1->1 transition per target, so the
    // source has no transition of its own: it is hidden in the same tick and
    // never reports. Pinned so `on_transition_complete`'s doc stays right.
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

    // PerTarget needs two ticks where Row needs one: each target's
    // transition request is created a tick after the split is set up.
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
// split_to_with_behavior() / merge_from_with_behavior()
// ---------------------------------------------------------------------------

#[test]
fn split_to_with_behavior_staggers_each_target() {
    use proteus_sdk::SplitStrategy;

    // Delay by index, so after 0.15s target 0 has finished its 0.1s
    // transition and target 2 (delayed 0.2s) has not started.
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
                easing: proteus_sdk::Easing::Linear,
            },
        )
        .unwrap();

    // Tick 1 creates the targets' transitions (see the PerTarget timing
    // note); tick 2 advances 0.15s into them.
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
    // exactly like plain split_to.
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
        MergeLayout::Row,
        |i, _total| TransitionConfig {
            duration: 0.1,
            delay: i as f32 * 0.1,
            easing: proteus_sdk::Easing::Linear,
        },
    )
    .unwrap();

    // The group can't complete until its slowest member does: source 2 is
    // delayed 0.2s on top of a 0.1s transition.
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
// split_to() / merge_from(): group transitions
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

    // Check that the target moves, not just that it ends where it was
    // declared: a target sits at its declared geometry from the start, so
    // the end state alone can't tell a completed transition from one that
    // never ran.
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

    let _ = dest.merge_from(&mut app, &[source1, source2], cfg(0.1), MergeLayout::Row);
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
    // Created at one geometry, then given a new declared geometry before it
    // is used as a transition target, like a grid cell whose real size is
    // only known once its label has baked.
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
    // The target's own declared geometry is very different from the state
    // passed here, which shows the explicit state wins.
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
    // Two ticks: the first sets up the split, which creates the target's
    // transition for the next tick; the second finishes it, since
    // `cfg(0.1)`'s duration is well under this tick's `dt`.
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
    // A component that is also one of its own split targets. Its current
    // geometry (here, screen-sized) must be left alone until the split
    // captures it as the starting geometry on the next tick; this call must
    // not move it the way `set_declared_geometry` would.
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
    // Source geometry must be untouched immediately after the call; the
    // split hasn't been set up yet.
    assert_eq!(app.get(source).unwrap().geometry.position, before.position);

    // Two ticks; see the test above for why.
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

    // Never started: nothing to update, and nothing should be created.
    let _ = tile.set_video_crossfade(&mut app, 0.5);
    assert!(app.world().get::<VideoCrossfade>(tile.id()).is_none());

    // Started, then stopped: likewise nothing to do.
    let _ = tile.start_video(&mut app);
    let _ = tile.stop_video(&mut app);
    let _ = tile.set_video_crossfade(&mut app, 0.5);
    assert!(app.world().get::<VideoCrossfade>(tile.id()).is_none());
}

// ---------------------------------------------------------------------------
// Stale handles report, they don't panic
// ---------------------------------------------------------------------------

// Every `Handle` method called on a destroyed component must report
// `EntityNotFound`, not panic. A panic in the web build stops the whole page.
// Reaching the end of this test is itself the check that nothing panicked.
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
        handle.crop_image(&mut app, ImageCrop::CenteredSquare),
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
        handle.merge_from(&mut app, &[other], config, proteus_sdk::MergeLayout::Row),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.add_child(&mut app, other),
        Err(HandleError::EntityNotFound)
    );
    assert_eq!(
        handle.remove_child(&mut app, other),
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

    // The app is still usable afterwards: a reported error leaves nothing
    // half-applied.
    assert!(app.get(other).is_some());
    assert!(app.get(handle).is_none());
}

// A destroyed component passed into a call on a live one is reported as
// `OtherEntityNotFound`, so a caller can tell which of the two is gone.
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
        live.remove_child(&mut app, dead),
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
        live.merge_from(&mut app, &[dead], cfg(0.2), proteus_sdk::MergeLayout::Row),
        Err(HandleError::OtherEntityNotFound)
    );

    // The live handle is untouched by any of it.
    assert!(app.get(live).is_some());
}

// "Nothing to do" is not an error. A live component with no baked image yet
// reports `Ok(false)`: callers check for this while an image loads, and an
// `Err` would make a routine state look like a failure.
#[test]
fn nothing_to_do_is_ok_false_not_an_error() {
    let mut app = Proteus::new();
    let a = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let b = app.component(ComponentSpec::new(quad_at(50.0, 0.0)));

    assert_eq!(a.crop_image(&mut app, ImageCrop::CenteredSquare), Ok(false));
    assert_eq!(a.copy_baked_image_from(&mut app, b), Ok(false));
    // No GPU pipeline in a headless world, so there is no texture to show.
    let texture = app.texture(Default::default());
    assert_eq!(a.set_texture(&mut app, texture), Ok(false));
    // Alive, but never `start_video`-ed.
    assert_eq!(a.set_video_crossfade(&mut app, 0.5), Ok(false));
}

// A child in `component()`'s spec that no longer exists is skipped rather
// than causing a panic, and the other children still attach.
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
// set_declared_geometry keeps interaction styles in sync
// ---------------------------------------------------------------------------

// Interaction styles resolve against their own copy of the declared geometry.
// `set_declared_geometry` must update it too, or a component snaps back to
// its original geometry when the pointer leaves it: exactly the component
// whose layout is only known after creation, which the method exists for.
#[test]
fn set_declared_geometry_updates_what_hover_returns_to() {
    let spawn = quad_at(0.0, 0.0);
    let mut app = Proteus::new();
    let button = app.component(ComponentSpec::new(spawn.clone()).hover(StyleOverride {
        scale: Some(2.0),
        ..Default::default()
    }));

    // Tick 1 captures the declared geometry.
    app.tick(0.016);

    // The real layout is only now known, for example measured from baked
    // content.
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

// ---------------------------------------------------------------------------
// A grid too small for its pieces
// ---------------------------------------------------------------------------

// A grid with fewer cells than pieces must be refused up front. Otherwise the
// extra pieces are silently dropped: in a split an extra target just appears
// at the end, and in a merge an extra source vanishes instead of moving.
#[test]
fn split_to_refuses_a_grid_with_too_few_cells() {
    use proteus_sdk::SplitStrategy;

    let mut app = Proteus::new();
    let source = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let targets: Vec<_> = (0..3)
        .map(|i| app.component(ComponentSpec::new(quad_at(i as f32 * 100.0, 200.0))))
        .collect();

    assert_eq!(
        source.split_to(
            &mut app,
            &targets,
            cfg(0.1),
            SplitStrategy::Grid { cols: 2, rows: 1 }
        ),
        Err(HandleError::GridTooSmall {
            pieces: 3,
            cells: 2
        })
    );
    app.tick(0.0);
    assert!(app.get(source).unwrap().visible, "nothing started");
}

#[test]
fn merge_from_refuses_a_grid_with_too_few_cells() {
    use proteus_sdk::MergeLayout;

    let mut app = Proteus::new();
    let dest = app.component(ComponentSpec::new(quad_at(0.0, 0.0)));
    let sources: Vec<_> = (0..5)
        .map(|i| app.component(ComponentSpec::new(quad_at(i as f32 * 100.0, 200.0))))
        .collect();

    assert_eq!(
        dest.merge_from(
            &mut app,
            &sources,
            cfg(0.1),
            MergeLayout::Grid { cols: 2, rows: 2 }
        ),
        Err(HandleError::GridTooSmall {
            pieces: 5,
            cells: 4
        })
    );
    app.tick(0.0);
    assert!(
        sources.iter().all(|s| app.get(*s).unwrap().visible),
        "nothing started"
    );
}
