//! [`InstanceCache`]: every component's instances, kept from frame to frame
//! and redone only for what changed.
//!
//! [`collect_instances`](crate::collect_instances) builds every instance from
//! scratch, which at 100,000 components takes milliseconds even when nothing
//! moved. The cache keeps each entity's instances, its place in drawing order
//! and its geometry in world space, and each frame [`InstanceCache::update`]
//! redoes only the entities whose data changed, found with `bevy_ecs`'s change
//! detection:
//!
//! - **The drawing order** is worked out again, with every entity's instances,
//!   only when an entity gets or loses a `QuadState`, gets a new parent or
//!   loses one, or its `z` changes. Then it's the same walk as
//!   `collect_instances`.
//! - **Otherwise,** an entity is redone when anything its instances are made
//!   from changes or is removed: its geometry, visibility, opacity, effects,
//!   image, video, text, bake, transition, or a piece's corners. One whose geometry in world space
//!   changed also redoes its children, which are placed relative to it.
//!   Entities are redone in drawing order, so a parent is always done before
//!   its children. When more than a quarter of them changed, as when
//!   everything moves, they're all done again in one walk instead.
//!
//! The changes are reported as ranges of [`InstanceCache::instances`], so the
//! renderer uploads only those. When an entity gains or loses an instance, such
//! as text that has just been baked or an entity hidden, everything after it
//! moves, and the whole list is reported as changed.
//!
//! The instances are always exactly what `collect_instances` would return;
//! this module's tests check that through many frames of changes.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use bevy_ecs::entity::EntityHashMap;
use bevy_ecs::hierarchy::{ChildOf, Children};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;

use proteus_render::QuadInstance;

use crate::{
    bake::BakedComposite,
    draw_order::rank_cmp,
    effects::{Border, DropShadow, Glow},
    hierarchy::{compose_with_parent, EffectiveOpacity, EffectiveVisibility, Opacity},
    spawn_order::SpawnOrder,
    topology::PieceCorners,
    video::{VideoCrossfade, VideoPlayer},
    ActiveTransition, BakedImage, BakedText, BakedTexture, QuadState, Text, Virtual, Visibility,
};

/// The most separate ranges reported before the whole list is reported
/// instead: past it, one upload is cheaper than many small ones.
const MAX_RANGES: usize = 64;

/// Past this share of the entities changed, all of them are done again in one
/// walk, as when the drawing order changes: keeping track of which ones need
/// doing then costs more than it saves.
const MOST_CHANGED: f32 = 0.25;

/// What [`InstanceCache::update`] changed in [`InstanceCache::instances`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceChanges {
    /// Nothing: the instances are the same as last frame's.
    None,
    /// These ranges of the instances changed, and nothing else. They are
    /// sorted and don't touch.
    Ranges(Vec<Range<usize>>),
    /// The whole list changed, or moved: upload all of it.
    All,
}

/// One entity's cached data, at its place in drawing order.
struct Entry {
    entity: Entity,
    /// Its geometry in world space, which its children are placed against.
    world: QuadState,
    /// Its own `z`, which orders it among its siblings.
    z: f32,
    /// Where its instances start in [`InstanceCache::instances`].
    start: usize,
    /// How many instances it has: none while hidden, two with text.
    count: usize,
}

/// What changed since the last update, as `bevy_ecs` tracks it.
type ChangeParams = (
    Query<
        'static,
        'static,
        Entity,
        Or<(
            Changed<QuadState>,
            Changed<Visibility>,
            Changed<EffectiveVisibility>,
            Changed<Opacity>,
            Changed<EffectiveOpacity>,
            Changed<DropShadow>,
            Changed<Glow>,
            Changed<Border>,
        )>,
    >,
    Query<
        'static,
        'static,
        Entity,
        Or<(
            Changed<VideoPlayer>,
            Changed<BakedImage>,
            Changed<VideoCrossfade>,
            Changed<BakedComposite>,
            Changed<BakedTexture>,
            Changed<BakedText>,
            Changed<Text>,
            Changed<Virtual>,
            Changed<ActiveTransition>,
            Changed<PieceCorners>,
        )>,
    >,
    Query<'static, 'static, Entity, Changed<ChildOf>>,
    (
        RemovedComponents<'static, 'static, QuadState>,
        RemovedComponents<'static, 'static, ChildOf>,
    ),
    (
        RemovedComponents<'static, 'static, Visibility>,
        RemovedComponents<'static, 'static, EffectiveVisibility>,
        RemovedComponents<'static, 'static, Opacity>,
        RemovedComponents<'static, 'static, EffectiveOpacity>,
        RemovedComponents<'static, 'static, DropShadow>,
        RemovedComponents<'static, 'static, Glow>,
        RemovedComponents<'static, 'static, Border>,
        RemovedComponents<'static, 'static, VideoPlayer>,
    ),
    (
        RemovedComponents<'static, 'static, BakedImage>,
        RemovedComponents<'static, 'static, VideoCrossfade>,
        RemovedComponents<'static, 'static, BakedComposite>,
        RemovedComponents<'static, 'static, BakedTexture>,
        RemovedComponents<'static, 'static, BakedText>,
        RemovedComponents<'static, 'static, Text>,
        RemovedComponents<'static, 'static, Virtual>,
        RemovedComponents<'static, 'static, ActiveTransition>,
    ),
    RemovedComponents<'static, 'static, PieceCorners>,
);

/// Every component's instances, in drawing order, kept from frame to frame.
/// See the module docs.
#[derive(Default)]
pub struct InstanceCache {
    /// Every entity reached from a top-level entity, in drawing order, hidden
    /// ones included.
    entries: Vec<Entry>,
    /// Each entity's place in `entries`.
    index: EntityHashMap<usize>,
    /// Every entity's instances, in drawing order.
    instances: Vec<QuadInstance>,
    /// `bevy_ecs`'s change detection, from the last update to this one.
    /// Created on the first update.
    changes: Option<SystemState<ChangeParams>>,
}

impl InstanceCache {
    /// Every visible entity's instances, in drawing order, as of the last
    /// [`InstanceCache::update`].
    pub fn instances(&self) -> &[QuadInstance] {
        &self.instances
    }

    /// Brings the instances up to date with `world`, and returns what changed
    /// since the last update. The first update reports everything.
    ///
    /// It also clears `world`'s record of removed components, which it reads,
    /// so that the record doesn't grow for as long as the app runs.
    pub fn update(&mut self, world: &mut World) -> InstanceChanges {
        let first = self.changes.is_none();
        let state = self.changes.get_or_insert_with(|| SystemState::new(world));
        let (content, more_content, reparented, structure, removed, more_removed, mut corners) =
            state.get_mut(world);

        let (mut removed_quads, mut orphaned) = structure;
        let mut restructured = first
            || !reparented.is_empty()
            || removed_quads.read().next().is_some()
            || orphaned.read().next().is_some();

        let mut dirty: Vec<Entity> = content.iter().chain(more_content.iter()).collect();
        let (mut r0, mut r1, mut r2, mut r3, mut r4, mut r5, mut r6, mut r7) = removed;
        dirty.extend(r0.read().chain(r1.read()).chain(r2.read()).chain(r3.read()));
        dirty.extend(r4.read().chain(r5.read()).chain(r6.read()).chain(r7.read()));
        let (mut r0, mut r1, mut r2, mut r3, mut r4, mut r5, mut r6, mut r7) = more_removed;
        dirty.extend(r0.read().chain(r1.read()).chain(r2.read()).chain(r3.read()));
        dirty.extend(r4.read().chain(r5.read()).chain(r6.read()).chain(r7.read()));
        dirty.extend(corners.read());

        if !restructured {
            // A new entity, or one whose `z` changed, needs a new drawing
            // order.
            restructured = dirty.iter().any(|&e| match self.index.get(&e) {
                Some(&i) => world
                    .get::<QuadState>(e)
                    .is_some_and(|qs| qs.position.z != self.entries[i].z),
                None => world.get::<QuadState>(e).is_some(),
            });
        }
        world.clear_trackers();

        if restructured || dirty.len() as f32 > self.entries.len() as f32 * MOST_CHANGED {
            self.rebuild(world);
            return InstanceChanges::All;
        }
        self.redo(world, dirty)
    }

    /// Works out the drawing order and every entity's instances from
    /// scratch.
    pub(crate) fn rebuild(&mut self, world: &mut World) {
        self.entries.clear();
        self.index.clear();
        self.instances.clear();
        // Room for every entity at once, rather than growing as they're
        // added: counting them costs next to nothing.
        let count = world.query::<&QuadState>().iter(world).len();
        self.entries.reserve(count);
        self.index.reserve(count);
        self.instances.reserve(count);

        // Top-level entities: a QuadState and no ChildOf.
        let mut roots: Vec<(Entity, f32, SpawnOrder)> = world
            .query_filtered::<(Entity, &QuadState, Option<&SpawnOrder>), Without<ChildOf>>()
            .iter(world)
            .map(|(e, qs, spawn_order)| {
                // An entity without a SpawnOrder, such as one in a test world
                // with no hooks, sorts last among equals.
                let spawn = spawn_order.copied().unwrap_or(SpawnOrder(u64::MAX));
                (e, qs.position.z, spawn)
            })
            .collect();
        roots.sort_by(|a, b| rank_cmp((a.1, a.2), (b.1, b.2)));

        for (e, _, _) in roots {
            // A top-level entity's QuadState is already in world space.
            let world_qs = world
                .get::<QuadState>(e)
                .expect("queried with a QuadState")
                .clone();
            self.add_subtree(world, e, world_qs);
        }
    }

    /// Adds `entity`, placed at `world_qs`, then its children in drawing
    /// order, depth first.
    fn add_subtree(&mut self, world: &World, entity: Entity, world_qs: QuadState) {
        let start = self.instances.len();
        push_instances(world, entity, &world_qs, &mut self.instances);
        let z = world
            .get::<QuadState>(entity)
            .map_or(0.0, |qs| qs.position.z);
        self.index.insert(entity, self.entries.len());
        self.entries.push(Entry {
            entity,
            world: world_qs,
            z,
            start,
            count: self.instances.len() - start,
        });

        let Some(children) = world.get::<Children>(entity) else {
            return;
        };
        // Siblings are drawn by their own `z`, then creation order, like
        // top-level entities; see `crate::draw_order`.
        let mut children: Vec<Entity> = children.iter().collect();
        let rank = |e: Entity| {
            let z = world.get::<QuadState>(e).map_or(0.0, |qs| qs.position.z);
            let spawn = world
                .get::<SpawnOrder>(e)
                .copied()
                .unwrap_or(SpawnOrder(u64::MAX));
            (z, spawn)
        };
        children.sort_by(|a, b| rank_cmp(rank(*a), rank(*b)));
        let parent = self.index[&entity];
        for child in children {
            let Some(child_local) = world.get::<QuadState>(child) else {
                continue;
            };
            let child_world = compose_with_parent(&self.entries[parent].world, child_local);
            self.add_subtree(world, child, child_world);
        }
    }

    /// Redoes the instances of `dirty` and, where their world geometry
    /// changed, of their children, in drawing order. The drawing order itself
    /// hasn't changed.
    fn redo(&mut self, world: &World, dirty: Vec<Entity>) -> InstanceChanges {
        let mut pending: BTreeSet<usize> = dirty
            .into_iter()
            .filter_map(|e| self.index.get(&e).copied())
            .collect();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        // The new instances of entities whose number of instances changed,
        // by their place in drawing order. They move the instances after
        // them, so they're laid out once the rest are done.
        let mut resized: BTreeMap<usize, Vec<QuadInstance>> = BTreeMap::new();
        let mut scratch: Vec<QuadInstance> = Vec::new();

        while let Some(i) = pending.pop_first() {
            let entity = self.entries[i].entity;
            let Some(local) = world.get::<QuadState>(entity) else {
                continue;
            };
            let parent = world
                .get::<ChildOf>(entity)
                .and_then(|c| self.index.get(&c.parent()));
            let world_qs = match parent {
                Some(&parent) => compose_with_parent(&self.entries[parent].world, local),
                None => local.clone(),
            };
            scratch.clear();
            push_instances(world, entity, &world_qs, &mut scratch);

            let entry = &mut self.entries[i];
            if entry.world != world_qs {
                entry.world = world_qs;
                if let Some(children) = world.get::<Children>(entity) {
                    pending.extend(children.iter().filter_map(|c| self.index.get(&c)));
                }
            }
            let entry = &self.entries[i];
            let range = entry.start..entry.start + entry.count;
            if scratch.len() != entry.count {
                resized.insert(i, scratch.clone());
            } else if scratch[..] != self.instances[range.clone()] {
                self.instances[range.clone()].copy_from_slice(&scratch);
                ranges.push(range);
            }
        }

        if !resized.is_empty() {
            self.relayout(resized);
            return InstanceChanges::All;
        }
        if ranges.is_empty() {
            return InstanceChanges::None;
        }
        merge_ranges(ranges)
    }

    /// Lays the instances out again, with `resized`'s entities given their
    /// new instances, which number differently from their old ones and move
    /// the instances after them.
    fn relayout(&mut self, mut resized: BTreeMap<usize, Vec<QuadInstance>>) {
        let old = std::mem::take(&mut self.instances);
        self.instances.reserve(old.len());
        for (i, entry) in self.entries.iter_mut().enumerate() {
            let start = self.instances.len();
            match resized.remove(&i) {
                Some(new) => self.instances.extend_from_slice(&new),
                None => self
                    .instances
                    .extend_from_slice(&old[entry.start..entry.start + entry.count]),
            }
            entry.start = start;
            entry.count = self.instances.len() - start;
        }
    }
}

/// Appends `entity`'s instances, placed at `world_qs`, to `out`: none if it's
/// hidden.
fn push_instances(
    world: &World,
    entity: Entity,
    world_qs: &QuadState,
    out: &mut Vec<QuadInstance>,
) {
    // The cascaded visibility if computed, else the entity's own, for tests
    // that skip the schedule, else visible.
    let visible = world
        .get::<EffectiveVisibility>(entity)
        .map(|v| v.0)
        .unwrap_or_else(|| world.get::<Visibility>(entity).is_none_or(|v| v.visible));
    if visible {
        crate::collect::push_entity_instances(world, entity, world_qs, out);
    }
}

/// Sorts `ranges` and merges those that touch. Too many separate ones are
/// reported as [`InstanceChanges::All`].
fn merge_ranges(mut ranges: Vec<Range<usize>>) -> InstanceChanges {
    ranges.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    if merged.len() > MAX_RANGES {
        InstanceChanges::All
    } else {
        InstanceChanges::Ranges(merged)
    }
}

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3, Vec4};

    use super::*;
    use crate::schedule::ProteusWorld;

    /// A small random-number generator (xorshift), so a failure repeats.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn f32(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * (self.next() % 10_000) as f32 / 10_000.0
        }

        fn chance(&mut self, percent: u64) -> bool {
            self.next() % 100 < percent
        }
    }

    fn geometry(random: &mut Random) -> QuadState {
        QuadState {
            position: Vec3::new(
                random.f32(-300.0, 300.0),
                random.f32(-300.0, 300.0),
                // A few levels of `z`, so that some entities share one.
                random.below(3) as f32,
            ),
            size: Vec2::new(random.f32(4.0, 80.0), random.f32(4.0, 80.0)),
            color: Vec4::new(random.f32(0.0, 1.0), 0.5, 0.5, 1.0),
            corner_radius: random.f32(0.0, 10.0),
            ..Default::default()
        }
    }

    /// Makes one random change to the world, of the kinds an app or the
    /// engine makes.
    fn change(world: &mut World, entities: &mut Vec<Entity>, random: &mut Random) {
        entities.retain(|&e| world.get_entity(e).is_ok());
        if entities.is_empty() || random.chance(8) {
            let qs = geometry(random);
            entities.push(world.spawn(qs).id());
            return;
        }
        let e = entities[random.below(entities.len())];
        let other = entities[random.below(entities.len())];
        let mut entity = world.entity_mut(e);
        match random.below(18) {
            0 => {
                let child = geometry(random);
                let child = world.spawn((child, ChildOf(e))).id();
                entities.push(child);
            }
            1 => {
                let mut qs = entity.get_mut::<QuadState>().unwrap();
                qs.position.x += random.f32(-20.0, 20.0);
            }
            2 => {
                let z = random.below(3) as f32;
                entity.get_mut::<QuadState>().unwrap().position.z = z;
            }
            3 => {
                let visible = random.chance(50);
                entity.insert(Visibility { visible });
            }
            4 => {
                entity.insert(Opacity(random.f32(0.0, 1.0)));
            }
            5 => {
                if random.chance(50) {
                    entity.insert(Border {
                        width: random.f32(1.0, 4.0),
                        color: Vec4::ONE,
                        offset: -1.0,
                    });
                } else {
                    entity.remove::<Border>();
                }
            }
            6 => {
                if random.chance(50) {
                    entity.insert(DropShadow {
                        offset: Vec2::new(2.0, -2.0),
                        color: Vec4::new(0.0, 0.0, 0.0, 0.5),
                        softness: 4.0,
                        spread: 0.0,
                    });
                } else {
                    entity.remove::<DropShadow>();
                }
            }
            7 => {
                if random.chance(50) {
                    entity.insert(Glow {
                        radius: 6.0,
                        color: Vec4::ONE,
                        intensity: 0.5,
                    });
                } else {
                    entity.remove::<Glow>();
                }
            }
            8 => {
                if random.chance(50) {
                    entity.insert((
                        Text::new("Hi", 12.0),
                        BakedText {
                            uv_offset: [0.1, 0.1],
                            uv_scale: [0.05, 0.02],
                            page: 0,
                            pixel_size: [20.0, 14.0],
                        },
                    ));
                } else {
                    entity.remove::<BakedText>();
                }
            }
            9 => {
                if random.chance(50) {
                    entity.insert(BakedImage {
                        uv_offset: [0.2, 0.2],
                        uv_scale: [0.1, 0.1],
                        page: 1,
                        pixel_size: [64.0, 64.0],
                        full_uv_offset: [0.2, 0.2],
                        full_uv_scale: [0.1, 0.1],
                    });
                } else {
                    entity.remove::<BakedImage>();
                }
            }
            10 => {
                if random.chance(50) {
                    entity.insert(VideoPlayer);
                } else {
                    entity.remove::<VideoPlayer>();
                }
            }
            11 => {
                entity.insert(VideoCrossfade {
                    video_t: random.f32(0.0, 1.0),
                });
            }
            12 => {
                entity.despawn();
            }
            13 => {
                // Moves a top-level entity under another top-level entity,
                // which can't be one of its descendants.
                let both_top_level =
                    world.get::<ChildOf>(e).is_none() && world.get::<ChildOf>(other).is_none();
                if e != other && both_top_level {
                    world.entity_mut(e).insert(ChildOf(other));
                }
            }
            14 => {
                entity.remove::<ChildOf>();
            }
            15 => {
                entity.get_mut::<QuadState>().unwrap().size *= 1.1;
            }
            16 => {
                if random.chance(50) {
                    entity.insert(PieceCorners {
                        from: [random.f32(0.0, 10.0), 0.0, 4.0, 0.0],
                        to: [random.f32(0.0, 10.0); 4],
                    });
                } else {
                    entity.remove::<PieceCorners>();
                }
            }
            _ => {
                // A change to nothing an instance reads.
                let qs = entity.get::<QuadState>().unwrap().clone();
                entity.insert(qs);
            }
        }
    }

    /// Through many frames of random changes, the cache always holds exactly
    /// what a full collect returns, and reports every change.
    #[test]
    fn the_cache_always_matches_a_full_collect() {
        for seed in 1..=20u64 {
            let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut app = ProteusWorld::new();
            let mut cache = InstanceCache::default();
            // Enough entities that a frame's few changes stay under
            // `MOST_CHANGED`, so the changes are tracked one by one.
            let mut entities: Vec<Entity> = (0..60)
                .map(|_| {
                    let qs = geometry(&mut random);
                    app.world.spawn(qs).id()
                })
                .collect();
            let mut uploaded: Vec<QuadInstance> = Vec::new();

            for frame in 0..300 {
                for _ in 0..random.below(6) {
                    change(&mut app.world, &mut entities, &mut random);
                }
                app.update(1.0 / 60.0);
                app.refresh_cascades();

                // What a renderer uploads, from the reported changes.
                match cache.update(&mut app.world) {
                    InstanceChanges::None => {}
                    InstanceChanges::All => uploaded = cache.instances().to_vec(),
                    InstanceChanges::Ranges(ranges) => {
                        for range in ranges {
                            uploaded[range.clone()].copy_from_slice(&cache.instances()[range]);
                        }
                    }
                }
                let expected = crate::collect_instances(&mut app.world);
                assert_eq!(
                    cache.instances(),
                    expected.as_slice(),
                    "seed {seed}, frame {frame}: the cache differs from a full collect"
                );
                assert_eq!(
                    uploaded, expected,
                    "seed {seed}, frame {frame}: a change wasn't reported"
                );
            }
        }
    }

    #[test]
    fn a_still_frame_reports_no_changes() {
        let mut app = ProteusWorld::new();
        let mut random = Random(7);
        for _ in 0..50 {
            let qs = geometry(&mut random);
            app.world.spawn(qs);
        }
        let mut cache = InstanceCache::default();
        app.update(1.0 / 60.0);
        assert_eq!(cache.update(&mut app.world), InstanceChanges::All);
        for _ in 0..3 {
            app.update(1.0 / 60.0);
            app.refresh_cascades();
            assert_eq!(cache.update(&mut app.world), InstanceChanges::None);
        }
    }

    #[test]
    fn a_moved_parent_moves_its_children() {
        let mut app = ProteusWorld::new();
        let parent = app.world.spawn(QuadState::default()).id();
        app.world.spawn((QuadState::default(), ChildOf(parent)));
        // Others that don't move, so two changes are few enough to track one
        // by one.
        for _ in 0..10 {
            app.world.spawn(QuadState {
                position: Vec3::new(0.0, 0.0, 1.0),
                ..Default::default()
            });
        }
        let mut cache = InstanceCache::default();
        app.update(1.0 / 60.0);
        cache.update(&mut app.world);

        app.world.get_mut::<QuadState>(parent).unwrap().position.x = 50.0;
        app.update(1.0 / 60.0);
        let changes = cache.update(&mut app.world);
        assert_eq!(
            changes,
            InstanceChanges::Ranges(std::iter::once(0..2).collect()),
            "the parent's instance and its child's"
        );
        assert_eq!(cache.instances()[1].position[0], 50.0);
    }
}
