//! Pointer input and hit testing for M7 interactivity.
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
//! InteractionEvents  (Resource)
//!         │  shell reads this after update() in advance_demo()
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
//! Virtual entities and hidden entities are never hit-testable.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::component::Virtual;
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
    /// True while the primary button is held, including the `just_pressed` frame.
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
}

// ---------------------------------------------------------------------------
// HoveredEntity resource
// ---------------------------------------------------------------------------

/// Tracks which entity (if any) was under the pointer last frame.
///
/// Used by [`hit_test_system`] to compute hover-enter and hover-exit deltas.
#[derive(Resource, Default)]
pub struct HoveredEntity(pub Option<Entity>);

// ---------------------------------------------------------------------------
// Interactable component
// ---------------------------------------------------------------------------

/// Marks an entity as a hit-test target.
///
/// Entities without this component are never returned in [`InteractionEvents`],
/// even if the pointer is inside their bounds.
///
/// In M7 this is a pure marker. Callbacks (`onClick`, `onHoverEnter`, etc.)
/// will be added in M10 when the TypeScript SDK defines the developer-facing
/// API.
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
type HitTestQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static QuadState,
        Option<&'static Visibility>,
        Option<&'static EffectiveVisibility>,
    ),
    (With<Interactable>, Without<Virtual>),
>;

/// Replaces `stub_input_system`. Runs every frame in [`crate::schedule::ProteusSet::Input`].
///
/// Reads [`PointerInput`], finds the topmost interactable entity under the
/// pointer, and writes [`InteractionEvents`].
///
/// M10: an `Interactable` child's *local* `QuadState` is relative to its
/// parent, so it's resolved to world space (via [`resolve_world_position_query`])
/// before hit-testing — otherwise a child's hit region would silently test the
/// wrong screen location. Root entities are unaffected (resolution is a no-op
/// when there's no `ChildOf` ancestor).
pub fn hit_test_system(
    pointer: Res<PointerInput>,
    mut events: ResMut<InteractionEvents>,
    mut hovered: ResMut<HoveredEntity>,
    query: HitTestQuery,
    quad_states: Query<&QuadState>,
    parents: Query<&ChildOf>,
) {
    // Clear last frame's events.
    events.clicked.clear();
    events.hover_entered.clear();
    events.hover_exited.clear();

    let Some(pos) = pointer.position else {
        // Cursor left the window — exit any active hover.
        if let Some(prev) = hovered.0.take() {
            events.hover_exited.push(prev);
        }
        return;
    };

    // Find the topmost entity whose bounds contain the pointer.
    // Entities are tested in world order; last hit wins (matches draw order).
    let mut hit: Option<Entity> = None;
    for (e, qs, vis, eff_vis) in query.iter() {
        // Prefer the cascaded EffectiveVisibility; fall back to the entity's
        // own raw Visibility for callers that run hit_test_system without the
        // full schedule (existing test convention in this crate).
        let visible = eff_vis
            .map(|v| v.0)
            .unwrap_or_else(|| vis.is_none_or(|v| v.visible));
        if !visible {
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
        }
    }
}
