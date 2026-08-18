//! Pointer input and hit testing for M7 interactivity, extended in M12.2 with
//! press/release/drag/focus events and `Disabled`/`TransitioningConfig` gating.
//!
//! ## Data flow
//!
//! ```text
//! Shell (winit / JS events)
//!         │  writes each frame before update()
//!         ▼
//! PointerInput  (Resource)
//!         │
//!         │  hit_test_system reads this + queries Interactable entities
//!         ▼
//! InteractionEvents  (Resource)  +  HoveredEntity / PressedEntity / FocusState
//!         │  shell reads InteractionEvents after update() in advance_demo();
//!         │  interaction::interaction_style_system (M12.2) reads the resources
//!         ▼
//! Demo state machine  →  inserts TransitionRequest on the right entity
//! ```
//!
//! ## Lifecycle of `just_pressed` / `just_released`
//!
//! These are true for exactly **one frame**. The shell sets them when the OS
//! event fires, and clears them at the start of the next tick (before writing
//! the new pointer state).
//!
//! ## Hit testing
//!
//! The hit test uses `qs`'s true (oriented, scaled) footprint, resolved to
//! world space (M10 — see `hierarchy::resolve_world_position_query`) so an
//! `Interactable` child hit-tests against where it's actually drawn, not its
//! raw parent-relative coordinates, and rotated to match `QuadState::rotation`
//! (M10.6 — see `quad_contains`) so a rotated entity's hit region matches its
//! rendered footprint rather than the unrotated shape's axis-aligned box.
//!
//! Entities are tested in world insertion order; the **last** entity whose
//! bounds contain the pointer wins (matches GPU draw order — last drawn =
//! visually on top).
//!
//! Virtual entities, hidden entities, `Disabled` entities, and (M12.2)
//! `Transitioning` entities without `TransitioningConfig::allow_input` are
//! never hit-testable — excluded from the candidate loop entirely, so they
//! neither receive events nor occlude what's drawn behind them while gated.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::component::{Disabled, Lifecycle, TransitioningConfig, Virtual};
use crate::hierarchy::{resolve_world_position_query, EffectiveVisibility};
use crate::{QuadState, Visibility};

// ---------------------------------------------------------------------------
// PointerInput resource
// ---------------------------------------------------------------------------

/// Pointer state written by the shell each frame, before `ProteusWorld::update()`.
///
/// The shell is responsible for clearing `just_pressed` and `just_released`
/// at the start of each tick so they are true for exactly one frame.
///
/// ## Coordinate system
///
/// `position` is in **world-space**: origin at the centre of the viewport,
/// X right, Y up — the same coordinate system as `QuadState::position`.
///
/// The shell must convert from window/CSS coordinates (origin top-left, Y down):
/// ```text
/// world_x = cursor_x - viewport_width  / 2
/// world_y = viewport_height / 2 - cursor_y
/// ```
#[derive(Resource, Default)]
pub struct PointerInput {
    /// Current pointer position in **world-space** (origin centre, Y up).
    /// `None` when the cursor is outside the window.
    pub position: Option<Vec2>,
    /// True only on the frame the primary button transitioned from up to down.
    pub just_pressed: bool,
    /// True only on the frame the primary button transitioned from down to up.
    pub just_released: bool,
    /// True while the primary button is held, including the `just_pressed`
    /// frame — but **not** the `just_released` frame (M12.2's drag reporting
    /// in [`hit_test_system`] relies on `is_pressed` already being `false` by
    /// the time `just_released` fires, so release and drag never both act on
    /// the same frame).
    pub is_pressed: bool,
}

// ---------------------------------------------------------------------------
// InteractionEvents resource
// ---------------------------------------------------------------------------

/// Per-frame interaction events produced by [`hit_test_system`].
///
/// Read these after `ProteusWorld::update()` in the shell's `advance_demo()`.
/// The vecs are cleared and repopulated on every frame.
#[derive(Resource, Default)]
pub struct InteractionEvents {
    /// Entities whose bounds contained the pointer on the frame `just_pressed`
    /// was true — i.e. the user clicked them.
    pub clicked: Vec<Entity>,
    /// Entities the pointer entered this frame (was not hovered last frame,
    /// is hovered this frame).
    pub hover_entered: Vec<Entity>,
    /// Entities the pointer exited this frame (was hovered last frame, is no
    /// longer hovered this frame).
    pub hover_exited: Vec<Entity>,
    /// Entities that became pressed this frame (`just_pressed` while hit) — M12.2.
    pub pressed: Vec<Entity>,
    /// Entities that were released this frame (`just_released` while they were
    /// the pressed entity — fires on whatever was pressed, regardless of
    /// whether the pointer is still over it) — M12.2.
    pub released: Vec<Entity>,
    /// Entities that gained keyboard/click focus this frame — M12.2.
    pub focused: Vec<Entity>,
    /// Entities that lost focus this frame (a different entity was focused;
    /// clicking empty space does not blur — see [`hit_test_system`]) — M12.2.
    pub blurred: Vec<Entity>,
    /// `(entity, delta)` for the currently-pressed entity while the pointer is
    /// held and its position is known — delta is `(0, 0)` on the press frame
    /// itself — M12.2.
    pub dragged: Vec<(Entity, Vec2)>,
}

// ---------------------------------------------------------------------------
// HoveredEntity / PressedEntity / FocusState resources
// ---------------------------------------------------------------------------

/// Tracks which entity (if any) was under the pointer last frame.
///
/// Used by [`hit_test_system`] to compute hover-enter and hover-exit deltas.
#[derive(Resource, Default)]
pub struct HoveredEntity(pub Option<Entity>);

/// Tracks which entity (if any) is currently pressed, and the pointer position
/// as of the last frame it was pressed — M12.2.
///
/// Used by [`hit_test_system`] to compute `pressed`/`released`/`dragged`
/// deltas. `last_position` is private bookkeeping, not part of the public
/// drag-delta contract (that's `InteractionEvents::dragged`).
#[derive(Resource, Default)]
pub struct PressedEntity {
    pub entity: Option<Entity>,
    last_position: Option<Vec2>,
}

/// Tracks which entity (if any) currently has focus — M12.2.
///
/// Updated by [`hit_test_system`] via click-to-focus (see its doc for the
/// exact rule: clicking a different `Interactable` moves focus, clicking
/// empty space leaves it unchanged). `navigation_system`, once it exists
/// (still `stub_navigation_system` today), will also read and write this.
#[derive(Resource, Default)]
pub struct FocusState {
    pub focused: Option<Entity>,
}

// ---------------------------------------------------------------------------
// Interactable component
// ---------------------------------------------------------------------------

/// Marks an entity as a hit-test target.
///
/// Entities without this component are never returned in [`InteractionEvents`],
/// even if the pointer is inside their bounds.
///
/// A pure marker — no callback storage. `onClick`/`onHoverEnter`/etc. handler
/// *registration* is an SDK-layer concern (M12.3+); this crate's job is to
/// produce the underlying events in [`InteractionEvents`] for something else
/// to dispatch.
#[derive(Component, Default)]
pub struct Interactable;

// ---------------------------------------------------------------------------
// Hit test helper
// ---------------------------------------------------------------------------

/// Returns `true` if `point` (window-space pixels, origin top-left) is inside
/// `qs`'s true footprint — accounting for rotation and (uniform) scale, not
/// just an axis-aligned box (M10.6).
///
/// `QuadState::position` is the world location of the rotation *pivot* — the
/// anchor point, per the vertex shader's own transform order (scale, then
/// anchor-shift, then rotate, then translate to `position`; see
/// `hierarchy::compose_with_parent`'s doc for the same convention used to
/// compose a child's world transform). So testing containment is the inverse
/// of that: shift `point` into a frame centered on the pivot, rotate it
/// *back* by `-rotation` to undo the quad's rotation, then test against the
/// same anchor-relative axis-aligned extents the pre-M10.6 version already
/// used — now additionally scaled by `QuadState::scale`, since the rendered
/// quad is too (a second latent gap this function had: `scale` was
/// previously ignored entirely, so a scaled entity's hit box didn't match
/// its rendered size even before rotation was in the picture).
///
/// Accounts for `QuadState::anchor` — a center-anchored quad (0.5, 0.5) has
/// its pivot at the center; a top-left-anchored quad (0.0, 0.0) has its pivot
/// at the top-left corner.
pub fn quad_contains(qs: &QuadState, point: Vec2) -> bool {
    let delta = point - qs.position.truncate();
    let local = Vec2::from_angle(-qs.rotation).rotate(delta);

    let scaled_size = qs.size * qs.scale;
    let min = -qs.anchor * scaled_size;
    let max = (Vec2::ONE - qs.anchor) * scaled_size;

    local.x >= min.x && local.x < max.x && local.y >= min.y && local.y < max.y
}

// ---------------------------------------------------------------------------
// hit_test_system
// ---------------------------------------------------------------------------

/// Query filter for [`hit_test_system`]: all non-virtual interactable entities.
///
/// `Lifecycle`/`TransitioningConfig`/`Disabled` are `Option`/`Has` — entities
/// without them are simply never gated (M12.2's additions are opt-in-to-skip,
/// not opt-in-to-participate; every M7-era caller and test keeps working
/// unchanged).
type HitTestQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static QuadState,
        Option<&'static Visibility>,
        Option<&'static EffectiveVisibility>,
        Option<&'static Lifecycle>,
        Option<&'static TransitioningConfig>,
        Has<Disabled>,
    ),
    (With<Interactable>, Without<Virtual>),
>;

/// Replaces `stub_input_system`. Runs every frame in [`crate::schedule::ProteusSet::Input`].
///
/// Reads [`PointerInput`], finds the topmost interactable entity under the
/// pointer, and writes [`InteractionEvents`] plus [`HoveredEntity`]/
/// [`PressedEntity`]/[`FocusState`].
///
/// M10: an `Interactable` child's *local* `QuadState` is relative to its
/// parent, so it's resolved to world space (via [`resolve_world_position_query`])
/// before hit-testing — otherwise a child's hit region would silently test the
/// wrong screen location. Root entities are unaffected (resolution is a no-op
/// when there's no `ChildOf` ancestor).
///
/// M12.2: `Disabled` entities, and `Transitioning` entities without
/// `TransitioningConfig::allow_input`, are excluded from the candidate loop
/// entirely — click-through, not just event-suppressed (see this module's
/// top doc for the reasoning).
#[allow(clippy::too_many_arguments)]
pub fn hit_test_system(
    pointer: Res<PointerInput>,
    mut events: ResMut<InteractionEvents>,
    mut hovered: ResMut<HoveredEntity>,
    mut pressed_entity: ResMut<PressedEntity>,
    mut focus: ResMut<FocusState>,
    query: HitTestQuery,
    quad_states: Query<&QuadState>,
    parents: Query<&ChildOf>,
) {
    // Clear last frame's events.
    events.clicked.clear();
    events.hover_entered.clear();
    events.hover_exited.clear();
    events.pressed.clear();
    events.released.clear();
    events.focused.clear();
    events.blurred.clear();
    events.dragged.clear();

    let Some(pos) = pointer.position else {
        // Cursor left the window — exit any active hover. Pressed/focus state
        // intentionally persists (mirrors HoveredEntity's own pre-M12.2
        // behavior) — losing the cursor doesn't imply losing a press or focus.
        if let Some(prev) = hovered.0.take() {
            events.hover_exited.push(prev);
        }
        return;
    };

    // Find the topmost entity whose bounds contain the pointer.
    // Entities are tested in world order; last hit wins (matches draw order).
    let mut hit: Option<Entity> = None;
    for (e, qs, vis, eff_vis, lifecycle, transitioning_config, disabled) in query.iter() {
        // Prefer the cascaded EffectiveVisibility; fall back to the entity's
        // own raw Visibility for callers that run hit_test_system without the
        // full schedule (existing test convention in this crate).
        let visible = eff_vis
            .map(|v| v.0)
            .unwrap_or_else(|| vis.is_none_or(|v| v.visible));
        if !visible {
            continue;
        }
        if disabled {
            continue;
        }
        let transitioning = matches!(lifecycle, Some(Lifecycle::Transitioning));
        let allow_input = transitioning_config.is_some_and(|c| c.allow_input);
        if transitioning && !allow_input {
            continue;
        }
        let world_qs = resolve_world_position_query(e, qs, &quad_states, &parents);
        if quad_contains(&world_qs, pos) {
            hit = Some(e);
        }
    }

    // Compute hover enter / exit.
    if hit != hovered.0 {
        if let Some(prev) = hovered.0 {
            events.hover_exited.push(prev);
        }
        if let Some(new) = hit {
            events.hover_entered.push(new);
        }
        hovered.0 = hit;
    }

    // Click: just_pressed while over a hit entity.
    if pointer.just_pressed {
        if let Some(e) = hit {
            events.clicked.push(e);

            // Click-to-focus: moving focus to a different entity blurs the
            // old one. Clicking empty space (handled below, hit=None takes
            // this branch too since it's outside the `if let Some(e) = hit`)
            // does not change focus at all — see this module's top doc.
            if focus.focused != Some(e) {
                if let Some(prev) = focus.focused {
                    events.blurred.push(prev);
                }
                events.focused.push(e);
                focus.focused = Some(e);
            }
        }
    }

    // Press: just_pressed while over a hit entity starts a press; drag delta
    // starts at (0, 0) so the first dragged frame doesn't report a jump.
    if pointer.just_pressed {
        if let Some(e) = hit {
            events.pressed.push(e);
            pressed_entity.entity = Some(e);
            pressed_entity.last_position = Some(pos);
        }
    }

    // Drag: while held and a press is active, report this frame's delta from
    // wherever the pointer was last frame, then update that baseline.
    if pointer.is_pressed {
        if let Some(e) = pressed_entity.entity {
            let last = pressed_entity.last_position.unwrap_or(pos);
            events.dragged.push((e, pos - last));
            pressed_entity.last_position = Some(pos);
        }
    }

    // Release fires on whatever was pressed, regardless of where the pointer
    // is now — standard UI convention (dragging off the button and releasing
    // still counts as releasing that button).
    if pointer.just_released {
        if let Some(e) = pressed_entity.entity.take() {
            events.released.push(e);
        }
        pressed_entity.last_position = None;
    }
}
