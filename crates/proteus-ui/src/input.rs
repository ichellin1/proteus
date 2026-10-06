//! Pointer input and hit testing.
//!
//! The host writes the pointer's state to [`PointerInput`] before each tick.
//! [`hit_test_system`] finds the component under the pointer and records this
//! tick's clicks, hovers, presses, releases, drags and focus changes in
//! [`InteractionEvents`]. The interaction style system and the SDK's callbacks
//! read them from there.
//!
//! ## Hit testing
//!
//! A component is hit-tested against the area it is actually drawn in: its
//! position relative to its parents, its rotation and its scale.
//!
//! When several components contain the pointer, the one with the greatest
//! `(z, SpawnOrder)` wins: the same order [`crate::collect_instances`] draws
//! top-level components in, so the click goes to the one drawn on top.
//!
//! **A known difference from drawing:** a child is always drawn over its
//! parent, whatever its `z`, and never above a different top-level component.
//! Hit testing compares every candidate by the same key instead. The two agree
//! for top-level components, and for a child created after its parent with no
//! `z` of its own; they can disagree for a child with a nonzero `z`, or one
//! moved under a parent created later.
//!
//! Virtual, hidden and `Disabled` entities, and transitioning entities without
//! `TransitioningConfig::allow_input`, are never hit: they receive no events
//! and don't block the components behind them.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::component::{Disabled, Lifecycle, TransitioningConfig, Virtual};
use crate::hierarchy::{resolve_world_position_query, EffectiveVisibility};
use crate::spawn_order::SpawnOrder;
use crate::{QuadState, Visibility};

// ---------------------------------------------------------------------------
// PointerInput resource
// ---------------------------------------------------------------------------

/// The pointer's state, written by the host before each tick.
///
/// Whoever drives the world clears `just_pressed` and `just_released` after
/// each tick, so each is true for one tick only; `Proteus::tick` does this.
///
/// `position` is in world units, like `QuadState::position`: origin at the
/// center of the viewport, x right, y up. From window coordinates:
///
/// ```text
/// world_x = cursor_x - viewport_width  / 2
/// world_y = viewport_height / 2 - cursor_y
/// ```
#[derive(Resource, Default)]
pub struct PointerInput {
    /// The pointer's position in world units, or `None` when it is outside
    /// the viewport.
    pub position: Option<Vec2>,
    /// True on the tick the pointer was pressed.
    pub just_pressed: bool,
    /// True on the tick the pointer was released.
    pub just_released: bool,
    /// True while the pointer is pressed, including the `just_pressed` tick
    /// but not the `just_released` one, so a drag and a release never happen
    /// in the same tick.
    pub is_pressed: bool,
}

// ---------------------------------------------------------------------------
// InteractionEvents resource
// ---------------------------------------------------------------------------

/// This tick's input events, recorded by [`hit_test_system`]. Cleared and
/// filled again every tick.
#[derive(Resource, Default)]
pub struct InteractionEvents {
    /// Entities the pointer was pressed on: clicked.
    pub clicked: Vec<Entity>,
    /// Entities the pointer moved onto.
    pub hover_entered: Vec<Entity>,
    /// Entities the pointer moved off.
    pub hover_exited: Vec<Entity>,
    /// Entities that were pressed.
    pub pressed: Vec<Entity>,
    /// Entities that were released: whatever was pressed, wherever the pointer
    /// is now.
    pub released: Vec<Entity>,
    /// Entities that gained focus.
    pub focused: Vec<Entity>,
    /// Entities that lost focus because another entity gained it. Clicking
    /// empty space doesn't remove focus.
    pub blurred: Vec<Entity>,
    /// The pressed entity and how far the pointer moved since the last tick,
    /// every tick while it is pressed. `(0, 0)` on the tick of the press.
    pub dragged: Vec<(Entity, Vec2)>,
}

// ---------------------------------------------------------------------------
// HoveredEntity / PressedEntity / FocusState resources
// ---------------------------------------------------------------------------

/// The entity under the pointer last tick, if any. Used to detect the pointer
/// moving onto and off entities.
#[derive(Resource, Default)]
pub struct HoveredEntity(pub Option<Entity>);

/// The entity currently pressed, if any, and where the pointer was last tick,
/// for measuring drags.
#[derive(Resource, Default)]
pub struct PressedEntity {
    /// The pressed entity.
    pub entity: Option<Entity>,
    last_position: Option<Vec2>,
}

/// The entity that has focus, if any. Clicking an entity gives it focus;
/// clicking empty space leaves focus where it is.
#[derive(Resource, Default)]
pub struct FocusState {
    /// The focused entity.
    pub focused: Option<Entity>,
}

// ---------------------------------------------------------------------------
// Interactable component
// ---------------------------------------------------------------------------

/// Marks an entity as something the pointer can hit. Entities without it never
/// appear in [`InteractionEvents`].
///
/// Callbacks are not stored here; the SDK registers and calls them, using the
/// events in [`InteractionEvents`].
#[derive(Component, Default)]
pub struct Interactable;

// ---------------------------------------------------------------------------
// Hit test helper
// ---------------------------------------------------------------------------

/// Whether `point`, in world units, is inside the area  `qs` ([`QuadState`]) is drawn in,
/// taking its anchor, rotation and scale into account.
///
/// `qs.position` is where the anchor point is, and the quad rotates about it:
/// the shader scales, shifts by the anchor, rotates, then moves to
/// `position`. So this does the reverse: moves `point` relative to the anchor,
/// rotates it back by `-rotation`, and tests it against the scaled,
/// anchor-relative extents.
///
/// ## Anchor is Y-down; these extents are Y-up
///
/// `anchor` uses the screen convention — `[0,0]` is the component's *top*-left,
/// `[1,1]` its bottom-right — while `local` here is world-space, Y-up. The two
/// disagree on Y, so the Y extents are **not** the mirror of the X ones. Derived
/// from `quad.wgsl`'s own `anchor_shift` (`(anchor - 0.5) * size * vec2(1, -1)`,
/// applied to a unit quad vertex `v ∈ [-0.5, 0.5]`, Y-up):
///
/// ```text
/// x: size.x * (v.x - anchor.x + 0.5)  →  [-anchor.x·w, (1 - anchor.x)·w]
/// y: size.y * (v.y + anchor.y - 0.5)  →  [-(1 - anchor.y)·h, anchor.y·h]
/// ```
///
/// Both are `±size/2` at the default center anchor, so getting y backwards only
/// shows with another anchor: at `(0, 0)`, a button would be clickable in the
/// rectangle above it.
pub fn quad_contains(qs: &QuadState, point: Vec2) -> bool {
    let delta = point - qs.position.truncate();
    let local = Vec2::from_angle(-qs.rotation).rotate(delta);

    let scaled_size = qs.size * qs.scale;
    // Not `-anchor * size` / `(1 - anchor) * size`: see the note on y above.
    let min = Vec2::new(
        -qs.anchor.x * scaled_size.x,
        -(1.0 - qs.anchor.y) * scaled_size.y,
    );
    let max = Vec2::new(
        (1.0 - qs.anchor.x) * scaled_size.x,
        qs.anchor.y * scaled_size.y,
    );

    local.x >= min.x && local.x < max.x && local.y >= min.y && local.y < max.y
}

// ---------------------------------------------------------------------------
// hit_test_system
// ---------------------------------------------------------------------------

/// The entities [`hit_test_system`] considers: every interactable entity that
/// isn't virtual. `Lifecycle`, `TransitioningConfig` and `Disabled` are
/// optional; an entity without them is never excluded by them.
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
        Option<&'static SpawnOrder>,
    ),
    (With<Interactable>, Without<Virtual>),
>;

/// Finds the entity under the pointer and records this tick's input events in
/// [`InteractionEvents`], updating [`HoveredEntity`], [`PressedEntity`] and
/// [`FocusState`]. Runs in [`crate::schedule::ProteusSet::Input`].
///
/// A child is tested at its position in the world, from
/// [`resolve_world_position_query`]. Which entities are excluded, and which
/// wins when several overlap, is described in the module docs.
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
        // The pointer left the viewport: end any hover. A press or focus
        // stays, since leaving doesn't end either.
        if let Some(prev) = hovered.0.take() {
            events.hover_exited.push(prev);
        }
        return;
    };

    // Find the topmost entity containing the pointer, by `(z, SpawnOrder)`,
    // the order `collect_instances` draws in.
    let mut hit: Option<(Entity, f32, SpawnOrder)> = None;
    for (e, qs, vis, eff_vis, lifecycle, transitioning_config, disabled, spawn_order) in
        query.iter()
    {
        // The cascaded visibility if it has been computed, else the entity's
        // own, for tests that run this system without the full schedule.
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
        if !quad_contains(&world_qs, pos) {
            continue;
        }
        // An entity without a SpawnOrder, such as one in a test world with no
        // hooks, sorts last among equals, as in `collect_instances`: drawn on
        // top there, so it wins here.
        let order = spawn_order.copied().unwrap_or(SpawnOrder(u64::MAX));
        let z = world_qs.position.z;
        let wins = match hit {
            None => true,
            // `is_ge`, not `is_gt`: in an exact tie, the last one checked
            // wins.
            Some((_, best_z, best_order)) => z
                .partial_cmp(&best_z)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(order.cmp(&best_order))
                .is_ge(),
        };
        if wins {
            hit = Some((e, z, order));
        }
    }
    let hit = hit.map(|(e, _, _)| e);

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

            // Clicking gives the entity focus, and the old one loses it.
            // Clicking empty space doesn't change focus.
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

    // The release goes to whatever was pressed, wherever the pointer is now:
    // dragging off a button and releasing still releases that button.
    if pointer.just_released {
        if let Some(e) = pressed_entity.entity.take() {
            events.released.push(e);
        }
        pressed_entity.last_position = None;
    }
}
