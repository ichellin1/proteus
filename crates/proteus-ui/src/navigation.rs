//! Moving focus with the keys: the arrows, Tab and Shift+Tab, through the
//! actions they're bound to.
//!
//! [`navigation_system`] takes the [`NavigationRequests`] no key callback
//! claimed and moves focus with each:
//!
//! - A component can name its neighbour for an action with [`Neighbours`],
//!   which wins over the one navigation would choose.
//! - `Up`, `Down`, `Left` and `Right` go to the nearest component in their
//!   direction. Those in line with the focused one, overlapping it across the
//!   direction, come first, the nearest along it winning; without one, the
//!   distance along the direction and a weighted distance across it decide.
//!   They don't wrap.
//! - `Next` and `Previous` follow reading order: rows from top to bottom, two
//!   components being in one row when their heights mostly overlap, and each
//!   row in the app's [`ReadingDirection`]. They wrap round when [`TabWraps`]
//!   says so.
//!
//! Only components that can take focus, and are on screen, are candidates.
//! Components are measured where they're drawn, a rotated one by the box
//! around it.
//!
//! With nothing focused, navigation starts from where focus was lost,
//! [`FocusState::lost_at`]; or else focuses the hovered component, the one
//! marked [`DefaultFocus`], or the first in reading order, the last for
//! `Previous`. The first navigation after the app was pointing only shows
//! focus; see [`NavigationRequests::reveal`].

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::focus::{focus_blocked, DefaultFocus};
use crate::hierarchy::resolve_world_position_query;
use crate::input::{FocusState, HoveredEntity, Interactable, InteractionEvents};
use crate::keymap::{Action, NavigationRequests, TabWraps};
use crate::QuadState;

/// The order components are read in along a row, for `Next` and `Previous`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReadingDirection {
    /// Left to right, as in English.
    #[default]
    LeftToRight,
    /// Right to left, as in Arabic and Hebrew.
    RightToLeft,
}

/// The viewport's size in world units, for telling which components are on
/// screen. Without it, as in tests, every component counts as on screen.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct ViewportSize(pub Option<Vec2>);

/// The components a component names as its neighbours, for the actions that
/// move focus. A neighbour named here wins over the one navigation would
/// choose, as long as it can take focus.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Neighbours(pub Vec<(Action, Entity)>);

impl Neighbours {
    /// The neighbour named for `action`, if any.
    pub fn get(&self, action: Action) -> Option<Entity> {
        self.0.iter().find(|(a, _)| *a == action).map(|(_, e)| *e)
    }

    /// Names `neighbour` for `action`, or removes the one named with `None`.
    pub fn set(&mut self, action: Action, neighbour: Option<Entity>) {
        self.0.retain(|(a, _)| *a != action);
        if let Some(e) = neighbour {
            self.0.push((action, e));
        }
    }
}

/// An axis-aligned box in world units, y up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// The bottom-left corner.
    pub min: Vec2,
    /// The top-right corner.
    pub max: Vec2,
}

impl Rect {
    /// The box around `qs`, drawn where it is: its anchor, scale and rotation
    /// taken into account. `qs` is in world units, as from
    /// [`resolve_world_position_query`].
    pub fn around(qs: &QuadState) -> Rect {
        let size = qs.size * qs.scale;
        // As in `crate::input::quad_contains`: the anchor is y-down.
        let lo = Vec2::new(-qs.anchor.x * size.x, -(1.0 - qs.anchor.y) * size.y);
        let hi = Vec2::new((1.0 - qs.anchor.x) * size.x, qs.anchor.y * size.y);
        let rotation = Vec2::from_angle(qs.rotation);
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]
            .map(|c| qs.position.truncate() + rotation.rotate(c));
        let min = corners.iter().fold(Vec2::INFINITY, |m, c| m.min(*c));
        let max = corners.iter().fold(Vec2::NEG_INFINITY, |m, c| m.max(*c));
        Rect { min, max }
    }

    fn center(&self) -> Vec2 {
        (self.min + self.max) / 2.0
    }

    fn overlaps(&self, other: &Rect) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.y < other.max.y
            && other.min.y < self.max.y
    }
}

/// The direction an arrow action moves in, as a unit vector, y up.
fn direction(action: Action) -> Option<Vec2> {
    match action {
        Action::Up => Some(Vec2::Y),
        Action::Down => Some(Vec2::NEG_Y),
        Action::Left => Some(Vec2::NEG_X),
        Action::Right => Some(Vec2::X),
        _ => None,
    }
}

// How much a candidate's distance across the direction counts against it,
// against its distance along it, when none is in line. As on Android.
const ACROSS_WEIGHT: f32 = 13.0;

/// The candidate `Up`, `Down`, `Left` or `Right` goes to from `from`, if any.
pub fn nearest_in_direction(
    from: &Rect,
    dir: Vec2,
    candidates: &[(Entity, Rect)],
) -> Option<Entity> {
    // A small allowance, so components that touch count as beyond each
    // other's edge.
    const EPS: f32 = 0.5;
    let across = Vec2::new(dir.y.abs(), dir.x.abs());
    let mut best_in_line: Option<(f32, f32, Entity)> = None;
    let mut best_other: Option<(f32, Entity)> = None;
    for (e, r) in candidates {
        // How far `r`'s near edge is beyond `from`'s far edge, along `dir`.
        let gap = if dir.x > 0.0 {
            r.min.x - from.max.x
        } else if dir.x < 0.0 {
            from.min.x - r.max.x
        } else if dir.y > 0.0 {
            r.min.y - from.max.y
        } else {
            from.min.y - r.max.y
        };
        if gap < -EPS {
            continue;
        }
        let gap = gap.max(0.0);
        let offset = ((r.center() - from.center()) * across).length();
        let in_line = if across.x > 0.0 {
            r.min.x < from.max.x && from.min.x < r.max.x
        } else {
            r.min.y < from.max.y && from.min.y < r.max.y
        };
        if in_line {
            if best_in_line.is_none_or(|(g, o, _)| (gap, offset) < (g, o)) {
                best_in_line = Some((gap, offset, *e));
            }
        } else {
            let score = ACROSS_WEIGHT * gap * gap + offset * offset;
            if best_other.is_none_or(|(s, _)| score < s) {
                best_other = Some((score, *e));
            }
        }
    }
    best_in_line
        .map(|(_, _, e)| e)
        .or(best_other.map(|(_, e)| e))
}

/// `candidates` in reading order: rows from top to bottom, two being in one
/// row when their heights overlap by at least half the shorter one, and each
/// row in `direction`.
pub fn reading_order(
    candidates: &[(Entity, Rect)],
    direction: ReadingDirection,
) -> Vec<(Entity, Rect)> {
    let mut by_top: Vec<(Entity, Rect)> = candidates.to_vec();
    by_top.sort_by(|a, b| b.1.max.y.total_cmp(&a.1.max.y));

    let mut rows: Vec<Vec<(Entity, Rect)>> = Vec::new();
    for item in by_top {
        let joins = rows.last().is_some_and(|row| {
            row.iter().any(|(_, r)| {
                let overlap = r.max.y.min(item.1.max.y) - r.min.y.max(item.1.min.y);
                let shorter = (r.max.y - r.min.y).min(item.1.max.y - item.1.min.y);
                overlap >= shorter / 2.0
            })
        });
        if joins {
            rows.last_mut().expect("checked above").push(item);
        } else {
            rows.push(vec![item]);
        }
    }
    for row in &mut rows {
        match direction {
            ReadingDirection::LeftToRight => row.sort_by(|a, b| a.1.min.x.total_cmp(&b.1.min.x)),
            ReadingDirection::RightToLeft => row.sort_by(|a, b| b.1.max.x.total_cmp(&a.1.max.x)),
        }
    }
    rows.into_iter().flatten().collect()
}

/// The component `Next`, or `Previous` if `forward` is false, goes to from
/// `from` in `ordered`, wrapping round if `wraps`. `from` is a component in
/// `ordered`, or a place, for the component focus was lost from, which is
/// put into the order where it would read.
fn step(
    ordered: &[(Entity, Rect)],
    from: Result<Entity, Rect>,
    forward: bool,
    wraps: bool,
    direction: ReadingDirection,
) -> Option<Entity> {
    if ordered.is_empty() {
        return None;
    }
    // Where `from` is in the order: between `before` and `before + 1`.
    let (before, at): (isize, bool) = match from {
        Ok(e) => match ordered.iter().position(|(c, _)| *c == e) {
            Some(i) => (i as isize, true),
            None => (-1, false),
        },
        Err(place) => {
            let marker = Entity::PLACEHOLDER;
            let mut with_place = ordered.to_vec();
            with_place.push((marker, place));
            let order = reading_order(&with_place, direction);
            let i = order.iter().position(|(c, _)| *c == marker).expect("added");
            (i as isize - 1, false)
        }
    };
    let n = ordered.len() as isize;
    let next = if forward {
        before + 1
    } else if at {
        before - 1
    } else {
        before
    };
    if (0..n).contains(&next) {
        Some(ordered[next as usize].0)
    } else if wraps {
        Some(ordered[next.rem_euclid(n) as usize].0)
    } else {
        None
    }
}

/// The components focus can move to: those that can take focus and are on
/// screen, with the box each is drawn in.
fn candidates(world: &World) -> Vec<(Entity, Rect)> {
    let viewport = world.resource::<ViewportSize>().0.map(|size| Rect {
        min: -size / 2.0,
        max: size / 2.0,
    });
    let mut query = world.try_query_filtered::<Entity, With<Interactable>>();
    let Some(query) = query.as_mut() else {
        return Vec::new();
    };
    query
        .iter(world)
        .filter(|e| focus_blocked(world, *e).is_none())
        .filter_map(|e| {
            let rect = world_rect(world, e)?;
            viewport
                .is_none_or(|v| v.overlaps(&rect))
                .then_some((e, rect))
        })
        .collect()
}

/// The box `entity` is drawn in, in world units, if it has geometry.
pub fn world_rect(world: &World, entity: Entity) -> Option<Rect> {
    let qs = world.get::<QuadState>(entity)?;
    let mut state = world.try_query::<&QuadState>()?;
    let mut parents = world.try_query::<&ChildOf>()?;
    let qs = resolve_world_position_query(entity, qs, &state.query(world), &parents.query(world));
    Some(Rect::around(&qs))
}

/// Moves focus with each of this tick's [`NavigationRequests`]; see the
/// module docs. Runs in [`crate::schedule::ProteusSet::Navigation`], after
/// [`crate::focus::focus_validity_system`].
pub fn navigation_system(world: &mut World) {
    let requests = std::mem::take(&mut world.resource_mut::<NavigationRequests>().actions);
    let mut reveal = std::mem::take(&mut world.resource_mut::<NavigationRequests>().reveal);
    for action in requests {
        let target = if reveal {
            reveal = false;
            reveal_target(world, action)
        } else {
            move_target(world, action)
        };
        if let Some(target) = target {
            give_focus(world, target);
        }
    }
}

/// Where the first navigation after the app was pointing puts focus: on the
/// hovered component, if it can take focus; nowhere new if a component has
/// focus already, since navigating shows it; or else where navigation with
/// nothing focused starts.
fn reveal_target(world: &World, action: Action) -> Option<Entity> {
    let hovered = world.resource::<HoveredEntity>().0;
    if let Some(h) = hovered.filter(|h| focus_blocked(world, *h).is_none()) {
        return Some(h);
    }
    if world.resource::<FocusState>().focused.is_some() {
        return None;
    }
    move_target(world, action)
}

/// Where `action` moves focus to, if anywhere.
fn move_target(world: &World, action: Action) -> Option<Entity> {
    let focus = world.resource::<FocusState>();
    let focused = focus.focused;
    if let Some(named) = focused
        .and_then(|f| world.get::<Neighbours>(f))
        .and_then(|n| n.get(action))
        .filter(|n| focus_blocked(world, *n).is_none())
    {
        return Some(named);
    }

    let candidates: Vec<(Entity, Rect)> = candidates(world)
        .into_iter()
        .filter(|(e, _)| Some(*e) != focused)
        .collect();
    let from = match focused.and_then(|f| world_rect(world, f)) {
        Some(rect) => Some((focused, rect)),
        None => focus.lost_at.as_ref().map(|qs| (None, Rect::around(qs))),
    };

    let Some((from_entity, from_rect)) = from else {
        return start_target(world, action, &candidates);
    };
    if let Some(dir) = direction(action) {
        return nearest_in_direction(&from_rect, dir, &candidates);
    }
    let forward = match action {
        Action::Next => true,
        Action::Previous => false,
        _ => return None,
    };
    let reading = *world.resource::<ReadingDirection>();
    let wraps = world.resource::<TabWraps>().0;
    let mut ordered = candidates;
    let from = match from_entity {
        Some(e) => {
            ordered.push((e, from_rect));
            Ok(e)
        }
        None => Err(from_rect),
    };
    let ordered = reading_order(&ordered, reading);
    step(&ordered, from, forward, wraps, reading)
}

/// Where navigation starts with nothing focused and nowhere focus was lost:
/// the hovered component, the one marked [`DefaultFocus`], or the first in
/// reading order, the last for `Previous`.
fn start_target(world: &World, action: Action, candidates: &[(Entity, Rect)]) -> Option<Entity> {
    let hovered = world.resource::<HoveredEntity>().0;
    if let Some(h) = hovered.filter(|h| candidates.iter().any(|(c, _)| c == h)) {
        return Some(h);
    }
    let reading = *world.resource::<ReadingDirection>();
    let ordered = reading_order(candidates, reading);
    if let Some((e, _)) = ordered
        .iter()
        .find(|(e, _)| world.get::<DefaultFocus>(*e).is_some())
    {
        return Some(*e);
    }
    let pick = if action == Action::Previous {
        ordered.last()
    } else {
        ordered.first()
    };
    pick.map(|(e, _)| *e)
}

/// Gives `target` focus, taking it from the component that has it.
fn give_focus(world: &mut World, target: Entity) {
    let mut focus = world.resource_mut::<FocusState>();
    let previous = focus.focused;
    if previous == Some(target) {
        return;
    }
    focus.focused = Some(target);
    focus.lost_at = None;
    focus.geometry = None;
    let mut events = world.resource_mut::<InteractionEvents>();
    if let Some(p) = previous {
        events.blurred.push(p);
    }
    events.focused.push(target);
}
