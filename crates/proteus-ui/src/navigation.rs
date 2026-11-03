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
//! A component with a [`FocusGroup`] makes the components under it a group.
//! Focus moves among a group's members first, and when it would go past the
//! group's edge, the group lets it leave, stops it, or wraps it round, for
//! each action. Focus entering a group lands on the nearest member, the one
//! focused last, or the one marked [`DefaultFocus`]. Groups nest: the
//! innermost group's rules apply first. A group's members come together in
//! Tab order, and a member of a group focus is in can take focus off screen,
//! so a scrolling row's next item can be reached.
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

/// What a focus group does with an action that would take focus past its
/// edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Containment {
    /// Let focus leave the group, to the next group out, or the app.
    #[default]
    Leave,
    /// Keep focus where it is.
    Stop,
    /// Wrap focus round to the other end of the group.
    Wrap,
}

/// Where focus lands when it enters a focus group from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusEntry {
    /// On the member nearest where focus came from.
    #[default]
    Nearest,
    /// On the member that had focus last, as a row of a TV home page does;
    /// the nearest the first time.
    LastFocused,
    /// On the member marked [`DefaultFocus`]; the nearest if none is.
    Default,
}

/// Makes the components under a component a focus group; see the module
/// docs.
///
/// ```
/// use proteus_ui::{Action, FocusEntry, FocusGroup};
/// // A row of a TV home page: Left and Right stay in it, Up and Down leave it,
/// // and coming back lands where the user was.
/// let row = FocusGroup::new()
///     .stop(&[Action::Left, Action::Right])
///     .entry(FocusEntry::LastFocused);
/// // A dialog: nothing leaves it, and Tab wraps round.
/// let dialog = FocusGroup::new()
///     .stop(&[Action::Up, Action::Down, Action::Left, Action::Right])
///     .wrap(&[Action::Next, Action::Previous]);
/// # let _ = (row, dialog);
/// ```
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct FocusGroup {
    // For each of `Action::MOVING_FOCUS`, in its order.
    containment: [Containment; 6],
    entry: FocusEntry,
    last_focused: Option<Entity>,
}

impl FocusGroup {
    /// A group that lets every action leave it, and that focus enters at the
    /// nearest member.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps focus in the group for `actions`: at its edge, they don't move
    /// focus.
    pub fn stop(self, actions: &[Action]) -> Self {
        self.with(actions, Containment::Stop)
    }

    /// Wraps `actions` round at the group's edge: past the last member, focus
    /// goes to the first, and the other way.
    pub fn wrap(self, actions: &[Action]) -> Self {
        self.with(actions, Containment::Wrap)
    }

    /// Lets `actions` take focus out of the group at its edge, as every action
    /// does by default.
    pub fn leave(self, actions: &[Action]) -> Self {
        self.with(actions, Containment::Leave)
    }

    /// Sets where focus lands when it enters the group.
    pub fn entry(mut self, entry: FocusEntry) -> Self {
        self.entry = entry;
        self
    }

    fn with(mut self, actions: &[Action], containment: Containment) -> Self {
        for action in actions {
            if let Some(i) = Action::MOVING_FOCUS.iter().position(|a| a == action) {
                self.containment[i] = containment;
            }
        }
        self
    }

    /// What the group does with `action` at its edge. [`Containment::Leave`]
    /// for an action that doesn't move focus.
    pub fn containment(&self, action: Action) -> Containment {
        Action::MOVING_FOCUS
            .iter()
            .position(|a| *a == action)
            .map_or(Containment::Leave, |i| self.containment[i])
    }

    /// Where focus lands when it enters the group.
    pub fn entry_rule(&self) -> FocusEntry {
        self.entry
    }

    /// The member that had focus last, if any.
    pub fn last_focused(&self) -> Option<Entity> {
        self.last_focused
    }
}

/// The focus groups `entity` is in, innermost first.
fn groups_of(world: &World, entity: Entity) -> Vec<Entity> {
    let mut groups = Vec::new();
    let mut next = world.get::<ChildOf>(entity).map(|c| c.parent());
    while let Some(e) = next {
        if world.get::<FocusGroup>(e).is_some() {
            groups.push(e);
        }
        next = world.get::<ChildOf>(e).map(|c| c.parent());
    }
    groups
}

/// Whether `entity` is under `ancestor`.
fn is_under(world: &World, entity: Entity, ancestor: Entity) -> bool {
    let mut next = world.get::<ChildOf>(entity).map(|c| c.parent());
    while let Some(e) = next {
        if e == ancestor {
            return true;
        }
        next = world.get::<ChildOf>(e).map(|c| c.parent());
    }
    false
}

/// The outermost focus group `entity` is in under `scope`, or under the app
/// for `None`.
fn outermost_group_under(world: &World, entity: Entity, scope: Option<Entity>) -> Option<Entity> {
    let mut outermost = None;
    let mut next = world.get::<ChildOf>(entity).map(|c| c.parent());
    while let Some(e) = next {
        if Some(e) == scope {
            break;
        }
        if world.get::<FocusGroup>(e).is_some() {
            outermost = Some(e);
        }
        next = world.get::<ChildOf>(e).map(|c| c.parent());
    }
    outermost
}

/// `items`, all under `scope`, in Tab order: reading order, with each focus
/// group's members together, read in their own order, where the group
/// appears.
fn tab_order(
    world: &World,
    items: &[(Entity, Rect)],
    scope: Option<Entity>,
    direction: ReadingDirection,
) -> Vec<(Entity, Rect)> {
    let mut units: Vec<(Entity, Rect)> = Vec::new();
    let mut groups: Vec<(Entity, Vec<(Entity, Rect)>)> = Vec::new();
    for item in items {
        match outermost_group_under(world, item.0, scope) {
            None => units.push(*item),
            Some(g) => match groups.iter_mut().find(|(e, _)| *e == g) {
                Some((_, members)) => members.push(*item),
                None => groups.push((g, vec![*item])),
            },
        }
    }
    for (g, members) in &groups {
        let rect = world_rect(world, *g).unwrap_or_else(|| bounds(members));
        units.push((*g, rect));
    }
    let mut ordered = Vec::new();
    for unit in reading_order(&units, direction) {
        match groups.iter().find(|(g, _)| *g == unit.0) {
            Some((g, members)) => {
                ordered.extend(tab_order(world, members, Some(*g), direction));
            }
            None => ordered.push(unit),
        }
    }
    ordered
}

/// The box around `items`.
fn bounds(items: &[(Entity, Rect)]) -> Rect {
    items.iter().fold(
        Rect {
            min: Vec2::INFINITY,
            max: Vec2::NEG_INFINITY,
        },
        |b, (_, r)| Rect {
            min: b.min.min(r.min),
            max: b.max.max(r.max),
        },
    )
}

/// The member a group wraps round to for an arrow in `dir`: the one furthest
/// the other way among those in line with `from`, or among all if none is.
fn wrap_in_direction(from: &Rect, dir: Vec2, members: &[(Entity, Rect)]) -> Option<Entity> {
    let in_line: Vec<&(Entity, Rect)> = members
        .iter()
        .filter(|(_, r)| {
            if dir.x != 0.0 {
                r.min.y < from.max.y && from.min.y < r.max.y
            } else {
                r.min.x < from.max.x && from.min.x < r.max.x
            }
        })
        .collect();
    let pool: Vec<&(Entity, Rect)> = if in_line.is_empty() {
        members.iter().collect()
    } else {
        in_line
    };
    pool.into_iter()
        .min_by(|(_, a), (_, b)| a.center().dot(dir).total_cmp(&b.center().dot(dir)))
        .map(|(e, _)| *e)
}

/// Where focus lands in the groups `target` is in that `focused` isn't, from
/// the outermost in, by each group's [`FocusEntry`].
fn enter(world: &World, target: Entity, focused: Option<Entity>) -> Entity {
    let entered: Vec<Entity> = groups_of(world, target)
        .into_iter()
        .rev()
        .filter(|g| focused.is_none_or(|f| !is_under(world, f, *g)))
        .collect();
    let mut target = target;
    for g in entered {
        if !is_under(world, target, g) {
            continue;
        }
        let Some(group) = world.get::<FocusGroup>(g) else {
            continue;
        };
        let can_take = |e: &Entity| is_under(world, *e, g) && focus_blocked(world, *e).is_none();
        target = match group.entry {
            FocusEntry::Nearest => target,
            FocusEntry::LastFocused => group.last_focused.filter(can_take).unwrap_or(target),
            FocusEntry::Default => {
                let mut query = world.try_query_filtered::<Entity, With<DefaultFocus>>();
                query
                    .as_mut()
                    .and_then(|q| q.iter(world).find(can_take))
                    .unwrap_or(target)
            }
        };
    }
    target
}

/// Records, in each focus group the focused component is in, that it had
/// focus last. Runs after [`navigation_system`].
pub fn remember_group_focus_system(
    focus: Res<FocusState>,
    parents: Query<&ChildOf>,
    mut groups: Query<&mut FocusGroup>,
) {
    let Some(focused) = focus.focused else {
        return;
    };
    let mut next = parents.get(focused).ok().map(|c| c.parent());
    while let Some(e) = next {
        if let Ok(mut group) = groups.get_mut(e) {
            if group.last_focused != Some(focused) {
                group.last_focused = Some(focused);
            }
        }
        next = parents.get(e).ok().map(|c| c.parent());
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
            // The place goes before the first component that doesn't read
            // before it.
            let marker = Entity::PLACEHOLDER;
            let reads_before = |r: Rect| {
                reading_order(&[(marker, place), (Entity::PLACEHOLDER, r)], direction)[0].1 != place
            };
            let i = ordered
                .iter()
                .position(|(_, r)| !reads_before(*r))
                .unwrap_or(ordered.len());
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
/// screen, or off screen in one of `groups`, with the box each is drawn in.
fn candidates(world: &World, groups: &[Entity]) -> Vec<(Entity, Rect)> {
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
            let shown = viewport.is_none_or(|v| v.overlaps(&rect))
                || groups.iter().any(|g| is_under(world, e, *g));
            shown.then_some((e, rect))
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

    let groups = focused.map_or_else(Vec::new, |f| groups_of(world, f));
    let all: Vec<(Entity, Rect)> = candidates(world, &groups)
        .into_iter()
        .filter(|(e, _)| Some(*e) != focused)
        .collect();
    let from = match focused.and_then(|f| world_rect(world, f)) {
        Some(rect) => Some((focused, rect)),
        None => focus.lost_at.as_ref().map(|qs| (None, Rect::around(qs))),
    };
    let Some((from_entity, from_rect)) = from else {
        return start_target(world, action, &all).map(|t| enter(world, t, None));
    };

    let reading = *world.resource::<ReadingDirection>();
    // From the innermost group focus is in, out to the app: the first that
    // has a member to move to, or stops or wraps the action, decides.
    for scope in groups.iter().map(|g| Some(*g)).chain([None]) {
        let members: Vec<(Entity, Rect)> = all
            .iter()
            .filter(|(e, _)| scope.is_none_or(|g| is_under(world, *e, g)))
            .copied()
            .collect();
        let containment = match scope {
            Some(g) => world
                .get::<FocusGroup>(g)
                .map_or(Containment::Leave, |group| group.containment(action)),
            None if world.resource::<TabWraps>().0 => Containment::Wrap,
            None => Containment::Stop,
        };
        let target = if let Some(dir) = direction(action) {
            nearest_in_direction(&from_rect, dir, &members).or_else(|| {
                // The arrows don't wrap at the app's edge, whatever Tab does.
                (scope.is_some() && containment == Containment::Wrap)
                    .then(|| wrap_in_direction(&from_rect, dir, &members))
                    .flatten()
            })
        } else {
            let forward = match action {
                Action::Next => true,
                Action::Previous => false,
                _ => return None,
            };
            let mut ordered = members;
            let from = match from_entity {
                Some(e) => {
                    ordered.push((e, from_rect));
                    Ok(e)
                }
                None => Err(from_rect),
            };
            let ordered = tab_order(world, &ordered, scope, reading);
            step(
                &ordered,
                from,
                forward,
                containment == Containment::Wrap,
                reading,
            )
        };
        if let Some(target) = target {
            return Some(enter(world, target, focused));
        }
        if containment != Containment::Leave {
            return None;
        }
    }
    None
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
    let ordered = tab_order(world, candidates, None, reading);
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
