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
//! The hit test uses axis-aligned bounding boxes derived from `QuadState`.
//! Rotation and non-uniform scale are not yet accounted for (good enough for
//! M7; full convex-hull testing can land with M5.5 hierarchy).
//!
//! Entities are tested in world insertion order; the **last** entity whose
//! bounds contain the pointer wins (matches GPU draw order — last drawn =
//! visually on top).
//!
//! Virtual entities and hidden entities are never hit-testable.

use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::component::Virtual;
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
/// the axis-aligned bounding box of `qs`.
///
/// Accounts for `QuadState::anchor` — a center-anchored quad (0.5, 0.5) has
/// its origin at the center; a top-left-anchored quad (0.0, 0.0) has its
/// origin at the top-left corner.
pub fn quad_contains(qs: &QuadState, point: Vec2) -> bool {
    let left = qs.position.x - qs.anchor.x * qs.size.x;
    let top = qs.position.y - qs.anchor.y * qs.size.y;
    let right = left + qs.size.x;
    let bottom = top + qs.size.y;
    point.x >= left && point.x < right && point.y >= top && point.y < bottom
}

// ---------------------------------------------------------------------------
// hit_test_system
// ---------------------------------------------------------------------------

/// Query filter for [`hit_test_system`]: all non-virtual interactable entities.
type HitTestQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static QuadState, Option<&'static Visibility>), (With<Interactable>, Without<Virtual>)>;

/// Replaces `stub_input_system`. Runs every frame in [`crate::schedule::ProteusSet::Input`].
///
/// Reads [`PointerInput`], finds the topmost interactable entity under the
/// pointer, and writes [`InteractionEvents`].
pub fn hit_test_system(
    pointer: Res<PointerInput>,
    mut events: ResMut<InteractionEvents>,
    mut hovered: ResMut<HoveredEntity>,
    query: HitTestQuery,
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
    for (e, qs, vis) in query.iter() {
        // Skip hidden entities — they are invisible and not interactive.
        if vis.is_some_and(|v| !v.visible) {
            continue;
        }
        if quad_contains(qs, pos) {
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
