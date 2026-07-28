//! [`TextureRegistry`] — reference-counted, generation-safe texture tracking (M11).
//!
//! Before M11 this was a plain `Vec<Entry>` keyed by a raw `u32`, with no way to ever free an
//! entry. `main_atlas` (baked text/images/composites), `transition_atlas` (ephemeral crossfade
//! snapshots), and `video_atlas` (the streamed video slot) are genuinely different lifecycles, so
//! this registry does not force them into one physical atlas — it's the single source of truth
//! for *metadata* (kind, ref count, state, recency) sitting above whichever low-level allocator is
//! appropriate per kind. `transition_atlas`/`TransitionAtlasAllocator` are already correctly
//! self-managed (real allocate/free, tied to transition start/completion) and are intentionally
//! **not** tracked here — this closes the `main_atlas`/`video_atlas` gap specifically.
//!
//! ## Reference counting
//!
//! `main_atlas` entries (`TextureKind::Static`) are ref-counted against
//! `proteus_ui::TextureRef` component lifecycle — see that type's docs for the `ComponentHooks`
//! wiring. A ref count of zero does **not** mean immediately freed; it means the entry becomes a
//! reclaim *candidate*, actually freed only via an explicit [`TextureRegistry::free`] call or via
//! eviction under allocation pressure. `TextureKind::Video` is metadata/observability only — the
//! single video texture slot is managed by ordinary Rust `Drop` in [`crate::QuadPipeline`],
//! independent of this registry's ref-counting.
//!
//! ## Eviction never touches referenced content
//!
//! If `main_atlas` is full, [`TextureRegistry::register_static`] evicts unreferenced
//! (`ref_count == 0`), non-`eternal` entries — oldest-`last_used`-first — until either enough room
//! opens or none remain eligible. It never evicts a still-referenced entry: `BakedText`/
//! `BakedImage`/`BakedComposite` cache their UV coordinates directly on the component for a fast
//! render path (no per-frame registry lookup), so evicting a referenced region and letting a
//! *different* texture reuse it later would make the original component silently render the wrong
//! (new occupant's) pixels — wrong content with no error, not a crash. If unreferenced eviction
//! isn't enough, registration simply fails (`None`, logged), same as every other atlas-full path in
//! this codebase.
//!
//! Evicting still-referenced content (with a restoration mechanism to make that safe) and
//! backgrounding-driven eviction beyond video are Post-V1 — see `ROADMAP.md`.

use crate::main_atlas_allocator::{MainAtlasAllocId, MainAtlasAllocator, MainAtlasRegion};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

slotmap::new_key_type! {
    /// Opaque, generation-safe handle to a registered texture.
    pub struct TextureId;
}

/// The category of a registered texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureKind {
    /// Permanent content packed into `main_atlas` — baked text, images, and composites.
    Static,
    /// A streaming video feed — pixel data is replaced each frame via
    /// [`crate::QuadPipeline::upload_video_frame`].
    Video,
    /// Reserved for future animated GIF/sprite-sheet playback — see `ROADMAP.md`'s Post-V1
    /// entry for the full design (each frame decoded once and packed into `main_atlas` like
    /// `Static`, cycled via a playhead rather than re-decoded like `Video`). Not constructed by
    /// any `register_*` method yet; reserved now so the enum's public shape doesn't need to
    /// break again once it is.
    Animated,
}

/// Lifecycle state of a registered texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureState {
    /// Registered but not yet uploaded.
    Loading,
    /// Uploaded and safe to sample.
    Ready,
    /// GPU memory released (eviction, or `suspend_video`) — not safe to sample.
    Evicted,
    /// Registration or upload failed.
    Failed,
}

/// Where a registered texture's pixels physically live.
///
/// `Main`'s `x`/`y` are the allocator's real placement, but deliberately carries no
/// `width`/`height` of its own: `etagere`'s shelf allocator may round a request up to a larger
/// bucket (documented on `MainAtlasAllocator`/`TransitionAtlasAllocator`'s own tests), and the
/// *caller's originally requested* dimensions — already tracked in `TextureEntry::size` — are what
/// upload/UV math must use. Using the allocator's (possibly padded) size there instead would
/// upload fewer content pixels than the region claims, or stretch UVs into unwritten padding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasRegion {
    /// A sub-region of `main_atlas`, owned by the shared [`MainAtlasAllocator`].
    Main {
        alloc_id: MainAtlasAllocId,
        x: u32,
        y: u32,
    },
    /// The whole `video_atlas` texture *is* the resource — no packed sub-region.
    Video,
}

#[derive(Debug)]
struct TextureEntry {
    kind: TextureKind,
    atlas_region: AtlasRegion,
    ref_count: u32,
    /// Opts out of eviction-under-pressure. No current caller sets this `true` — every
    /// `register_static` call registers with `eternal: false`; `register_video`'s single slot is
    /// always `eternal` since there's no packed region for eviction to reclaim anyway.
    eternal: bool,
    /// Frame counter (not wall-clock) of this entry's last real use — see
    /// [`TextureRegistry::touch`]. Only ever advances while `ref_count > 0`; once a zero-ref entry
    /// becomes a reclaim candidate this value freezes, which is exactly the LRU ordering eviction
    /// wants (longest-dormant reclaim candidate evicted first).
    last_used: u64,
    size: (u32, u32),
    state: TextureState,
}

// ---------------------------------------------------------------------------
// TextureRegistry
// ---------------------------------------------------------------------------

/// Tracks GPU texture allocations for `main_atlas` (`Static`) and the streamed video slot
/// (`Video`). A *metadata* store plus the CPU-side `main_atlas` sub-region allocator — the actual
/// `wgpu::Texture` objects live in [`crate::QuadPipeline`], which owns one `TextureRegistry`.
pub struct TextureRegistry {
    entries: slotmap::SlotMap<TextureId, TextureEntry>,
    main_atlas_allocator: MainAtlasAllocator,
    frame_counter: u64,
}

impl TextureRegistry {
    /// Create an empty registry backing a `main_atlas_size × main_atlas_size` atlas (see
    /// [`crate::MAIN_ATLAS_SIZE`]).
    pub fn new(main_atlas_size: u32) -> Self {
        Self {
            entries: slotmap::SlotMap::with_key(),
            main_atlas_allocator: MainAtlasAllocator::new(main_atlas_size),
            frame_counter: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Registration
    // -----------------------------------------------------------------------

    /// Register a `width × height` region of `main_atlas` for permanent content (baked text,
    /// images, or composites). Returns `None` if the atlas has no room even after evicting every
    /// eligible unreferenced entry (see the module docs' eviction-safety note).
    ///
    /// `ref_count` starts at 0 — the caller inserts a `proteus_ui::TextureRef` component
    /// immediately after, which increments it to 1 via its `ComponentHooks`, so a freshly
    /// registered entry is never observably zero for longer than one call.
    ///
    /// `eternal: true` opts this entry out of eviction-under-pressure entirely (no current caller
    /// needs this — always pass `false` — but the field exists per Phase B's spec).
    pub fn register_static(&mut self, width: u32, height: u32, eternal: bool) -> Option<TextureId> {
        let (alloc_id, region) = match self.main_atlas_allocator.allocate(width, height) {
            Some(r) => r,
            None => self.evict_to_make_room(width, height)?,
        };
        let id = self.entries.insert(TextureEntry {
            kind: TextureKind::Static,
            atlas_region: AtlasRegion::Main {
                alloc_id,
                x: region.x,
                y: region.y,
            },
            ref_count: 0,
            eternal,
            last_used: self.frame_counter,
            size: (width, height),
            state: TextureState::Ready,
        });
        Some(id)
    }

    /// Register the single streaming video slot. Metadata-only — mirrors the pre-M11 `register`
    /// call `QuadPipeline::init_video` made. Always `eternal`: there is no packed atlas region for
    /// eviction to reclaim here (the whole `video_atlas` texture is the resource, freed by ordinary
    /// Rust `Drop` when `QuadPipeline` replaces it — see the module docs).
    pub(crate) fn register_video(&mut self, width: u32, height: u32) -> TextureId {
        self.entries.insert(TextureEntry {
            kind: TextureKind::Video,
            atlas_region: AtlasRegion::Video,
            ref_count: 0,
            eternal: true,
            last_used: self.frame_counter,
            size: (width, height),
            state: TextureState::Ready,
        })
    }

    // -----------------------------------------------------------------------
    // Reference counting — driven by `proteus_ui::TextureRef`'s ComponentHooks
    // -----------------------------------------------------------------------

    /// Increment `id`'s reference count. Called only from `proteus_ui::TextureRef`'s
    /// `on_insert` hook — not intended for direct application use.
    pub fn incref(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.ref_count += 1;
        }
    }

    /// Decrement `id`'s reference count (floored at 0). Called only from
    /// `proteus_ui::TextureRef`'s `on_replace` hook (covers reassignment, explicit removal, and
    /// despawn uniformly) — not intended for direct application use. Reaching zero does *not*
    /// free the entry — see the module docs.
    pub fn decref(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.ref_count = e.ref_count.saturating_sub(1);
        }
    }

    /// Bump `id`'s recency to the current frame. Called once per frame for every live
    /// `TextureRef` by `proteus_ui`'s touch system.
    pub fn touch(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.last_used = self.frame_counter;
        }
    }

    /// Advance the internal frame counter. Call once per tick, before this frame's `touch` calls.
    pub fn advance_frame(&mut self) {
        self.frame_counter += 1;
    }

    // -----------------------------------------------------------------------
    // Freeing / eviction
    // -----------------------------------------------------------------------

    /// Explicitly release a zero-ref entry back to the allocator (`freeResources()`/`.free()` in
    /// Phase B's vocabulary). No-ops (with a warning) on a still-referenced entry — this is the
    /// primary way a reclaim candidate gets its GPU space back before allocation pressure forces
    /// the question.
    pub fn free(&mut self, id: TextureId) {
        let Some(entry) = self.entries.get(id) else {
            return;
        };
        if entry.ref_count > 0 {
            log::warn!(
                "TextureRegistry::free: entry {id:?} still has {} reference(s) — ignoring",
                entry.ref_count
            );
            return;
        }
        self.free_internal(id);
    }

    /// Free every current zero-ref, non-`eternal` `Static` entry, regardless of any specific size
    /// need. Returns how many were freed. For proactively reclaiming memory (e.g. before loading a
    /// large batch of new content) without waiting for an allocation to fail first.
    pub fn evict_unused(&mut self) -> usize {
        let candidates: Vec<TextureId> = self
            .entries
            .iter()
            .filter(|(_, e)| Self::is_eviction_candidate(e))
            .map(|(id, _)| id)
            .collect();
        let freed = candidates.len();
        for id in candidates {
            self.free_internal(id);
        }
        freed
    }

    /// Internal-only: called from [`register_static`](Self::register_static) when the initial
    /// allocation attempt fails. Frees unreferenced, non-`eternal` `Static` entries oldest-
    /// `last_used`-first, retrying the allocation after each free, and returns as soon as one
    /// succeeds. Never considers `ref_count > 0` entries (see the module docs). If every eligible
    /// entry is freed and the allocation still doesn't fit, returns `None` — the caller's
    /// registration simply fails, matching every other atlas-full path in this codebase; this
    /// milestone does not grow a second backing texture (see `ROADMAP.md`'s Post-V1 entry).
    ///
    /// Intentionally *not* public: it's inherently tied to one specific in-progress allocation
    /// attempt (a size to satisfy), an awkward, leaky shape for a general-purpose API —
    /// [`evict_unused`](Self::evict_unused)/[`free`](Self::free) cover the actual developer-facing
    /// use cases without exposing this internal.
    fn evict_to_make_room(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(MainAtlasAllocId, MainAtlasRegion)> {
        let mut candidates: Vec<(TextureId, u64)> = self
            .entries
            .iter()
            .filter(|(_, e)| Self::is_eviction_candidate(e))
            .map(|(id, e)| (id, e.last_used))
            .collect();
        candidates.sort_by_key(|&(_, last_used)| last_used);

        for (id, _) in candidates {
            self.free_internal(id);
            if let Some(region) = self.main_atlas_allocator.allocate(width, height) {
                return Some(region);
            }
        }
        None
    }

    fn is_eviction_candidate(entry: &TextureEntry) -> bool {
        entry.ref_count == 0
            && !entry.eternal
            && matches!(entry.atlas_region, AtlasRegion::Main { .. })
    }

    fn free_internal(&mut self, id: TextureId) {
        if let Some(entry) = self.entries.remove(id) {
            if let AtlasRegion::Main { alloc_id, .. } = entry.atlas_region {
                self.main_atlas_allocator.free(alloc_id);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Video suspend/resume (M9, kept working against the real registry)
    // -----------------------------------------------------------------------

    /// Mark the video texture as suspended (GPU memory freed or replaced with placeholder).
    pub(crate) fn mark_suspended(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.state = TextureState::Evicted;
        }
    }

    /// Mark the video texture as active again after a `QuadPipeline::resume_video` call.
    pub(crate) fn mark_active(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.state = TextureState::Ready;
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Returns `true` if the texture is registered and currently `Ready`.
    pub fn is_active(&self, id: TextureId) -> bool {
        self.entries
            .get(id)
            .is_some_and(|e| e.state == TextureState::Ready)
    }

    /// Returns the kind and pixel dimensions of a registered texture, if found.
    pub fn info(&self, id: TextureId) -> Option<(TextureKind, u32, u32)> {
        self.entries.get(id).map(|e| (e.kind, e.size.0, e.size.1))
    }

    /// The `(x, y, width, height)` pixel region for a `Static` entry, ready to pass to
    /// [`crate::QuadPipeline::write_to_main_atlas`]/[`crate::QuadPipeline::bake_instances_to_main_atlas`].
    /// `width`/`height` are the caller's originally requested content dimensions (see the
    /// [`AtlasRegion`] docs for why, not the allocator's possibly-padded footprint). `None` for a
    /// `Video` entry or an unknown id.
    pub fn main_atlas_region(&self, id: TextureId) -> Option<(u32, u32, u32, u32)> {
        let entry = self.entries.get(id)?;
        match entry.atlas_region {
            AtlasRegion::Main { x, y, .. } => Some((x, y, entry.size.0, entry.size.1)),
            AtlasRegion::Video => None,
        }
    }

    /// UV offset/scale for a `Static` entry's `main_atlas` region, normalised by `atlas_size` (see
    /// [`crate::MAIN_ATLAS_SIZE`]). `None` for a `Video` entry or an unknown id.
    pub fn main_atlas_uv(&self, id: TextureId, atlas_size: u32) -> Option<([f32; 2], [f32; 2])> {
        let (x, y, width, height) = self.main_atlas_region(id)?;
        let s = atlas_size as f32;
        Some((
            [x as f32 / s, y as f32 / s],
            [width as f32 / s, height as f32 / s],
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_static_returns_distinct_non_overlapping_regions() {
        let mut reg = TextureRegistry::new(1024);
        let a = reg.register_static(100, 50, false).unwrap();
        let b = reg.register_static(80, 40, false).unwrap();
        let c = reg.register_static(60, 60, false).unwrap();

        let regions: Vec<(u32, u32, u32, u32)> = [a, b, c]
            .iter()
            .map(|&id| reg.main_atlas_region(id).unwrap())
            .collect();

        for i in 0..regions.len() {
            for j in (i + 1)..regions.len() {
                let (ax, ay, aw, ah) = regions[i];
                let (bx, by, bw, bh) = regions[j];
                let overlap = ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by;
                assert!(
                    !overlap,
                    "regions {i} and {j} overlap: {:?} vs {:?}",
                    regions[i], regions[j]
                );
            }
        }
    }

    #[test]
    fn incref_decref_arithmetic() {
        let mut reg = TextureRegistry::new(256);
        let id = reg.register_static(16, 16, false).unwrap();
        reg.incref(id);
        reg.incref(id);
        reg.decref(id);
        // One reference remains — free() must refuse.
        reg.free(id);
        assert!(
            reg.main_atlas_region(id).is_some(),
            "entry with 1 remaining ref must not be freed"
        );
        reg.decref(id);
        reg.free(id);
        assert!(
            reg.main_atlas_region(id).is_none(),
            "entry at 0 refs should be freed by an explicit free() call"
        );
    }

    #[test]
    fn decref_below_zero_saturates_instead_of_panicking() {
        let mut reg = TextureRegistry::new(256);
        let id = reg.register_static(16, 16, false).unwrap();
        // No matching incref — must not underflow/panic.
        reg.decref(id);
        reg.free(id);
        assert!(reg.main_atlas_region(id).is_none());
    }

    #[test]
    fn free_then_reuse_round_trip() {
        // Fill the atlas by repeated small allocations rather than guessing
        // exact packing math. Every entry is kept referenced during the fill
        // so nothing is eviction-eligible yet — otherwise register_static
        // would just keep evicting an earlier entry and succeeding forever,
        // and the loop would never see a real `None`.
        let mut reg = TextureRegistry::new(64);
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            ids.push(id);
        }
        assert!(!ids.is_empty(), "at least one registration should have fit");
        assert!(
            reg.register_static(8, 8, false).is_none(),
            "atlas should now be exhausted"
        );

        let freed = ids.pop().unwrap();
        reg.decref(freed);
        reg.free(freed);
        assert!(
            reg.register_static(8, 8, false).is_some(),
            "freed space should be reusable"
        );
    }

    #[test]
    fn evict_unused_frees_every_zero_ref_entry_and_reports_the_count() {
        let mut reg = TextureRegistry::new(512);
        let a = reg.register_static(64, 64, false).unwrap();
        let b = reg.register_static(64, 64, false).unwrap();
        let referenced = reg.register_static(64, 64, false).unwrap();
        reg.incref(referenced);

        let freed = reg.evict_unused();
        assert_eq!(freed, 2, "only the two zero-ref entries should be freed");
        assert!(reg.main_atlas_region(a).is_none());
        assert!(reg.main_atlas_region(b).is_none());
        assert!(
            reg.main_atlas_region(referenced).is_some(),
            "referenced entry must survive evict_unused"
        );
    }

    #[test]
    fn eviction_under_pressure_prefers_oldest_unreferenced_entry() {
        let mut reg = TextureRegistry::new(256);
        // Fill the (guard-adjusted) atlas with two same-size unreferenced entries.
        let old = reg.register_static(120, 120, false).unwrap();
        reg.advance_frame();
        let newer = reg.register_static(120, 120, false).unwrap();
        reg.advance_frame();
        // Both are zero-ref reclaim candidates; `old` has the earlier last_used.

        // This registration doesn't fit without evicting exactly one entry.
        let third = reg
            .register_static(120, 120, false)
            .expect("should succeed after evicting the oldest unreferenced entry");

        assert!(
            reg.main_atlas_region(old).is_none(),
            "the oldest unreferenced entry should have been evicted"
        );
        assert!(
            reg.main_atlas_region(newer).is_some(),
            "the newer unreferenced entry should survive"
        );
        assert!(reg.main_atlas_region(third).is_some());
    }

    #[test]
    fn eviction_never_touches_referenced_or_eternal_entries() {
        let mut reg = TextureRegistry::new(64);
        // Fill the atlas while every entry is referenced, so nothing is
        // eviction-eligible yet — guarantees real exhaustion (if some entries
        // were already zero-ref, register_static would just keep evicting
        // and succeeding, and the atlas would never actually fill).
        let mut all = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            all.push(id);
        }
        assert!(!all.is_empty(), "at least one registration should have fit");
        assert!(
            reg.register_static(8, 8, false).is_none(),
            "exhausted while everything is still referenced"
        );

        // Make half of them eviction-eligible; the other half stay referenced.
        let mut referenced = Vec::new();
        let mut unreferenced = Vec::new();
        for (i, id) in all.into_iter().enumerate() {
            if i % 2 == 0 {
                referenced.push(id);
            } else {
                reg.decref(id);
                unreferenced.push(id);
            }
        }
        assert!(
            !unreferenced.is_empty(),
            "test setup needs an evictable entry"
        );

        // Plenty of unreferenced entries exist to evict, so this should
        // succeed by reclaiming one of those — never a referenced entry.
        assert!(
            reg.register_static(8, 8, false).is_some(),
            "should succeed by evicting an unreferenced entry"
        );
        for id in &referenced {
            assert!(
                reg.main_atlas_region(*id).is_some(),
                "referenced entry must never be evicted"
            );
        }
    }

    #[test]
    fn atlas_exhaustion_fails_gracefully_without_growing_a_second_page() {
        let mut reg = TextureRegistry::new(64);
        // Fill the tiny atlas; nothing is zero-ref-evictable because these
        // entries are still referenced.
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            ids.push(id);
        }
        assert!(!ids.is_empty(), "at least one region should have fit");

        // Exhausted: the next registration must return None, not panic or
        // silently allocate from a second backing texture (none exists).
        assert!(reg.register_static(8, 8, false).is_none());
    }

    #[test]
    fn register_video_is_metadata_only_and_always_active() {
        let mut reg = TextureRegistry::new(256);
        let id = reg.register_video(1280, 720);
        assert!(reg.is_active(id));
        assert_eq!(reg.info(id), Some((TextureKind::Video, 1280, 720)));
        assert!(
            reg.main_atlas_region(id).is_none(),
            "video has no packed main_atlas region"
        );

        reg.mark_suspended(id);
        assert!(!reg.is_active(id));
        reg.mark_active(id);
        assert!(reg.is_active(id));
    }

    #[test]
    fn main_atlas_uv_is_normalized_and_within_unit_range() {
        let mut reg = TextureRegistry::new(1024);
        let id = reg.register_static(64, 32, false).unwrap();
        let (offset, scale) = reg.main_atlas_uv(id, 1024).unwrap();
        assert!((0.0..=1.0).contains(&offset[0]));
        assert!((0.0..=1.0).contains(&offset[1]));
        assert!(scale[0] > 0.0 && scale[0] <= 1.0);
        assert!(scale[1] > 0.0 && scale[1] <= 1.0);
    }
}
