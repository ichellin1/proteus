//! [`TextureRegistry`]: which textures are in the main atlas and the video
//! slot, how many entities use each, and which to evict when space runs out.
//!
//! It holds metadata (kind, reference count, state, when last used) and the
//! main atlas's space allocators; the GPU textures themselves are in
//! [`crate::QuadPipeline`]. The transition atlas manages its own space and
//! isn't tracked here.
//!
//! ## Reference counting
//!
//! Main-atlas entries are counted by `proteus_ui::TextureRef` components. A
//! count of zero doesn't free an entry: it makes it a candidate, freed by
//! [`TextureRegistry::free`] or when space is needed. The video entry is only
//! metadata; `QuadPipeline` owns the video texture itself.
//!
//! ## Eviction never touches a texture in use
//!
//! When every page is full, [`TextureRegistry::register_static`] frees
//! unreferenced, non-`eternal` entries, least recently used first across all
//! pages, until the new texture fits. It never frees a referenced one:
//! components keep their texture coordinates, so a freed region reused by
//! another texture would make them draw the wrong image. If freeing
//! unreferenced entries isn't enough, registration fails and returns `None`,
//! with a log message.
//!
//! ## Pages
//!
//! The main atlas has a fixed number of pages, set by [`AtlasConfig`], since an
//! array texture can't grow without being recreated. Eviction keeps usage
//! within it: many images can be available without all being loaded at once.
//! Each page has its own [`MainAtlasAllocator`]; allocation tries the page that
//! worked last time, then all the others.
//!
//! Emptying a page recovers all its space, since `etagere`, the allocator,
//! merges freed regions. A page that is only partly empty can't be repacked,
//! because moving a region still in use would need every component using it
//! updated.

use crate::main_atlas_allocator::{MainAtlasAllocId, MainAtlasAllocator, MainAtlasRegion};

/// The size of each main-atlas page and how many there are.
///
/// The defaults, 2048 and 4, work everywhere, including WebGL2. A native-only
/// app can raise `page_size` (native devices allow at least 8192); an app short
/// of memory can lower `page_count`. Each page takes `page_size² × 4` bytes of
/// GPU memory, allocated up front.
///
/// No single texture can be larger than `page_size` on either side, and
/// WebGL2 limits `page_size` to 2048. So an image larger than 2048 pixels
/// can't be shown at full size on the web; it would need to be split across
/// several regions.
///
/// Check a configuration against the device with
/// [`crate::validate_atlas_config`] before using it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasConfig {
    /// Width and height of each page, in pixels.
    pub page_size: u32,
    /// Number of pages.
    pub page_count: u32,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            page_size: crate::DEFAULT_MAIN_ATLAS_SIZE,
            page_count: crate::DEFAULT_MAIN_ATLAS_PAGE_COUNT,
        }
    }
}

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
    /// In the main atlas: baked text, images and components.
    Static,
    /// The video, whose pixels are replaced each frame by
    /// [`crate::QuadPipeline::upload_video_frame`].
    Video,
    /// Not used yet; reserved for animated images, such as GIFs, whose frames
    /// would be packed into the main atlas and shown in turn.
    Animated,
}

/// Whether a registered texture can be drawn.
///
/// Registering a texture uploads it at the same time, so there is no loading
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureState {
    /// Uploaded and safe to sample.
    Ready,
    /// Its GPU memory has been released, by eviction or `suspend_video`; don't
    /// draw it.
    Evicted,
}

/// Where a registered texture's pixels are.
///
/// `Main` has the allocator's position but not its size: the allocator may
/// round a request up, and uploads and texture coordinates must use the size
/// that was requested, which the entry keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasRegion {
    /// A region of one main-atlas page, allocated by that page's
    /// [`MainAtlasAllocator`].
    Main {
        /// The page allocator's ID for the region.
        alloc_id: MainAtlasAllocId,
        /// The page the region is on, which is also the texture's array layer.
        page: u32,
        /// Left edge, in pixels.
        x: u32,
        /// Top edge, in pixels.
        y: u32,
    },
    /// The whole video texture.
    Video,
}

/// Where a main-atlas texture is: its page and pixel rectangle. Returned by
/// [`TextureRegistry::main_atlas_region`], and passed to
/// [`crate::QuadPipeline::write_to_main_atlas`] and
/// [`crate::QuadPipeline::bake_instances_to_main_atlas`].
///
/// The size is the one requested, not the allocator's possibly larger one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainAtlasPlacement {
    /// The page.
    pub page: u32,
    /// Left edge, in pixels.
    pub x: u32,
    /// Top edge, in pixels.
    pub y: u32,
    /// Width, in pixels.
    pub width: u32,
    /// Height, in pixels.
    pub height: u32,
}

/// Where a main-atlas texture is, as texture coordinates and a page: the three
/// values `proteus_ui::BakedImage`, `BakedText` and `BakedComposite` store.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainAtlasUv {
    /// The page.
    pub page: u32,
    /// Texture coordinates of the top-left corner.
    pub uv_offset: [f32; 2],
    /// Size in texture coordinates.
    pub uv_scale: [f32; 2],
}

#[derive(Debug)]
struct TextureEntry {
    kind: TextureKind,
    atlas_region: AtlasRegion,
    ref_count: u32,
    // Never evicted. Set for textures that must stay loaded, such as the
    // frames of an animation, and always for the video entry.
    eternal: bool,
    // The frame counter when the entry was last used; see `touch`. It stops
    // advancing once nothing references the entry, which gives eviction its
    // least-recently-used order.
    last_used: u64,
    size: (u32, u32),
    state: TextureState,
}

// ---------------------------------------------------------------------------
// TextureRegistry
// ---------------------------------------------------------------------------

/// Tracks the textures in the main atlas and the video slot.
///
/// It holds metadata and the main atlas's space allocators; the GPU textures
/// are in [`crate::QuadPipeline`], which owns the registry.
pub struct TextureRegistry {
    entries: slotmap::SlotMap<TextureId, TextureEntry>,
    // One allocator per page; the index is the page and array layer.
    main_atlas_pages: Vec<MainAtlasAllocator>,
    // Every page's size, for converting regions to texture coordinates.
    page_size: u32,
    // The page to try first: the one the last allocation used. Only an
    // optimization; every page is tried before allocation fails.
    preferred_page: usize,
    frame_counter: u64,
}

impl TextureRegistry {
    /// Creates an empty registry for a main atlas sized by `config`.
    /// `page_count` is at least 1. Page 0 reserves the white guard block at its
    /// origin; see [`MainAtlasAllocator::new_without_guard`].
    pub fn new(config: AtlasConfig) -> Self {
        let page_count = config.page_count.max(1) as usize;
        let mut main_atlas_pages = Vec::with_capacity(page_count);
        main_atlas_pages.push(MainAtlasAllocator::new(config.page_size));
        for _ in 1..page_count {
            main_atlas_pages.push(MainAtlasAllocator::new_without_guard(config.page_size));
        }
        Self {
            entries: slotmap::SlotMap::with_key(),
            main_atlas_pages,
            page_size: config.page_size,
            preferred_page: 0,
            frame_counter: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Registration
    // -----------------------------------------------------------------------

    /// Allocates a `width × height` region of the main atlas for text, an
    /// image or a baked component. Returns `None` if nothing fits, even after
    /// evicting every unreferenced texture; see the module docs.
    ///
    /// The reference count starts at 0; the caller adds a
    /// `proteus_ui::TextureRef` straight away, which raises it to 1. With
    /// `eternal`, the texture is never evicted.
    pub fn register_static(&mut self, width: u32, height: u32, eternal: bool) -> Option<TextureId> {
        let (page, alloc_id, region) = self
            .allocate_across_pages(width, height)
            .or_else(|| self.evict_to_make_room(width, height))?;
        let id = self.entries.insert(TextureEntry {
            kind: TextureKind::Static,
            atlas_region: AtlasRegion::Main {
                alloc_id,
                page,
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

    /// Tries `preferred_page`, then all the other pages in order, and records
    /// the page that fits. Never evicts.
    ///
    /// It takes the first page that fits, not the best fit: `etagere` can't
    /// cheaply report free space, and there are only a few pages.
    fn allocate_across_pages(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(u32, MainAtlasAllocId, MainAtlasRegion)> {
        let n = self.main_atlas_pages.len();
        for i in 0..n {
            let page = (self.preferred_page + i) % n;
            if let Some((alloc_id, region)) = self.main_atlas_pages[page].allocate(width, height) {
                self.preferred_page = page;
                return Some((page as u32, alloc_id, region));
            }
        }
        None
    }

    /// Registers the video slot. Metadata only, and always `eternal`, since
    /// there is no atlas region to reclaim: the whole video texture is freed
    /// when `QuadPipeline` replaces it.
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
    // Reference counting, by `proteus_ui::TextureRef`'s hooks
    // -----------------------------------------------------------------------

    /// Adds a reference to `id`. Called by `proteus_ui::TextureRef`'s hooks,
    /// not by apps.
    pub fn incref(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.ref_count += 1;
        }
    }

    /// Removes a reference to `id`, never going below zero. Called by
    /// `proteus_ui::TextureRef`'s hooks, not by apps. Reaching zero doesn't
    /// free the entry; see the module docs.
    pub fn decref(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.ref_count = e.ref_count.saturating_sub(1);
        }
    }

    /// Marks `id` as used this frame. `proteus_ui` calls it every frame for
    /// every texture in use.
    pub fn touch(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.last_used = self.frame_counter;
        }
    }

    /// Advances the frame counter. Call once per tick, before that tick's
    /// `touch` calls.
    pub fn advance_frame(&mut self) {
        self.frame_counter += 1;
    }

    // -----------------------------------------------------------------------
    // Freeing / eviction
    // -----------------------------------------------------------------------

    /// Frees an entry that nothing references, returning its space. Does
    /// nothing, with a warning, if something still references it.
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

    /// Frees every unreferenced, non-`eternal` main-atlas entry, and returns how
    /// many. Useful before loading many new textures, rather than waiting for
    /// space to run out.
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

    /// Makes room for a `width × height` region when no page has space: frees
    /// unreferenced, non-`eternal` entries, least recently used first across all
    /// pages, trying the allocation after each. Returns `None` if it still
    /// doesn't fit.
    fn evict_to_make_room(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(u32, MainAtlasAllocId, MainAtlasRegion)> {
        let mut candidates: Vec<(TextureId, u64)> = self
            .entries
            .iter()
            .filter(|(_, e)| Self::is_eviction_candidate(e))
            .map(|(id, e)| (id, e.last_used))
            .collect();
        candidates.sort_by_key(|&(_, last_used)| last_used);

        for (id, _) in candidates {
            if let Some(freed_page) = self.free_internal(id) {
                // The freed page is the only one that gained space — try it first.
                self.preferred_page = freed_page as usize;
            }
            if let Some(hit) = self.allocate_across_pages(width, height) {
                return Some(hit);
            }
        }
        None
    }

    fn is_eviction_candidate(entry: &TextureEntry) -> bool {
        entry.ref_count == 0
            && !entry.eternal
            && matches!(entry.atlas_region, AtlasRegion::Main { .. })
    }

    /// Removes `id` and frees its main-atlas region. Returns the page the
    /// region was on, or `None` for the video entry or an unknown `id`.
    ///
    /// A page doesn't need resetting when it becomes empty: tests showed that
    /// `etagere` already recovers all of an empty page's space, however its
    /// regions were freed.
    fn free_internal(&mut self, id: TextureId) -> Option<u32> {
        let entry = self.entries.remove(id)?;
        let AtlasRegion::Main { alloc_id, page, .. } = entry.atlas_region else {
            return None;
        };
        // `get_mut` rather than indexing, so a bad page can't panic.
        self.main_atlas_pages.get_mut(page as usize)?.free(alloc_id);
        Some(page)
    }

    // -----------------------------------------------------------------------
    // Video suspend and resume
    // -----------------------------------------------------------------------

    /// Marks the video texture as evicted: its GPU memory was released.
    pub(crate) fn mark_suspended(&mut self, id: TextureId) {
        if let Some(e) = self.entries.get_mut(id) {
            e.state = TextureState::Evicted;
        }
    }

    /// Marks the video texture as active again, after
    /// `QuadPipeline::resume_video`.
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

    /// Where a main-atlas texture is, to pass to
    /// [`crate::QuadPipeline::write_to_main_atlas`] or
    /// [`crate::QuadPipeline::bake_instances_to_main_atlas`]. `None` for the
    /// video entry or an unknown `id`.
    pub fn main_atlas_region(&self, id: TextureId) -> Option<MainAtlasPlacement> {
        let entry = self.entries.get(id)?;
        match entry.atlas_region {
            AtlasRegion::Main { page, x, y, .. } => Some(MainAtlasPlacement {
                page,
                x,
                y,
                width: entry.size.0,
                height: entry.size.1,
            }),
            AtlasRegion::Video => None,
        }
    }

    /// Where a main-atlas texture is, as texture coordinates and a page,
    /// converted with this registry's page size. `None` for the video entry or
    /// an unknown `id`.
    pub fn main_atlas_uv(&self, id: TextureId) -> Option<MainAtlasUv> {
        let p = self.main_atlas_region(id)?;
        let s = self.page_size as f32;
        Some(MainAtlasUv {
            page: p.page,
            uv_offset: [p.x as f32 / s, p.y as f32 / s],
            uv_scale: [p.width as f32 / s, p.height as f32 / s],
        })
    }

    /// How many main-atlas pages there are.
    pub fn page_count(&self) -> u32 {
        self.main_atlas_pages.len() as u32
    }

    /// How many textures are in the main atlas now, across all pages.
    pub fn resident_static_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.atlas_region, AtlasRegion::Main { .. }))
            .count()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // A one-page configuration, for tests that aren't about pages.
    fn single_page(page_size: u32) -> AtlasConfig {
        AtlasConfig {
            page_size,
            page_count: 1,
        }
    }

    #[test]
    fn register_static_returns_distinct_non_overlapping_regions() {
        let mut reg = TextureRegistry::new(single_page(1024));
        let a = reg.register_static(100, 50, false).unwrap();
        let b = reg.register_static(80, 40, false).unwrap();
        let c = reg.register_static(60, 60, false).unwrap();

        let regions: Vec<MainAtlasPlacement> = [a, b, c]
            .iter()
            .map(|&id| reg.main_atlas_region(id).unwrap())
            .collect();

        for i in 0..regions.len() {
            for j in (i + 1)..regions.len() {
                let a = regions[i];
                let b = regions[j];
                let overlap = a.x < b.x + b.width
                    && a.x + a.width > b.x
                    && a.y < b.y + b.height
                    && a.y + a.height > b.y;
                assert!(!overlap, "regions {i} and {j} overlap: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn incref_decref_arithmetic() {
        let mut reg = TextureRegistry::new(single_page(256));
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
        let mut reg = TextureRegistry::new(single_page(256));
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
        let mut reg = TextureRegistry::new(single_page(64));
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
        let mut reg = TextureRegistry::new(single_page(512));
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
        let mut reg = TextureRegistry::new(single_page(256));
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
        let mut reg = TextureRegistry::new(single_page(64));
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
    fn allocation_spills_to_the_next_page_once_page_zero_fills() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 3,
        });
        // Every entry stays referenced so nothing is eviction-eligible — a
        // registration that succeeds here must genuinely have found room on
        // some page, not evicted its way to success.
        let mut pages_seen = std::collections::HashSet::new();
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            pages_seen.insert(reg.main_atlas_region(id).unwrap().page);
            ids.push(id);
        }
        assert!(!ids.is_empty());
        assert!(
            pages_seen.contains(&0),
            "some entries should have landed on page 0"
        );
        assert!(
            pages_seen.len() > 1,
            "filling page 0 should have spilled onto at least one more page instead of \
             failing early — pages seen: {pages_seen:?}"
        );
    }

    #[test]
    fn exhaustion_across_every_page_still_fails_gracefully() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 3,
        });
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            ids.push(id);
        }
        assert!(!ids.is_empty());
        // Every page is now genuinely full of referenced (non-evictable) content — the
        // next registration must return None, not panic or invent a fourth page.
        assert!(reg.register_static(8, 8, false).is_none());
        assert_eq!(reg.page_count(), 3);
    }

    #[test]
    fn eviction_under_pressure_evicts_the_globally_oldest_entry_across_pages() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 2,
        });
        // Fill both pages with referenced entries, one of which (the oldest, on page 0)
        // we'll decref, plus a newer one on page 1 we'll also decref — both become
        // eviction candidates, but the page-0 one is strictly older.
        let mut all = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            all.push(id);
            reg.advance_frame();
        }
        assert!(all.len() >= 2, "test needs at least 2 resident entries");

        let on_page = |reg: &TextureRegistry, id: TextureId, page: u32| {
            reg.main_atlas_region(id).unwrap().page == page
        };
        let oldest_on_page_0 = *all
            .iter()
            .find(|&&id| on_page(&reg, id, 0))
            .expect("at least one entry should be on page 0");
        let newer_on_page_1 = *all
            .iter()
            .rev()
            .find(|&&id| on_page(&reg, id, 1))
            .expect("at least one entry should be on page 1");

        reg.decref(oldest_on_page_0);
        reg.decref(newer_on_page_1);

        // One more registration — should succeed by evicting the globally oldest
        // candidate (oldest_on_page_0), not just scan page 0 first and stop there.
        assert!(reg.register_static(8, 8, false).is_some());
        assert!(
            reg.main_atlas_region(oldest_on_page_0).is_none(),
            "the globally oldest unreferenced entry (on page 0) should have been evicted"
        );
        assert!(
            reg.main_atlas_region(newer_on_page_1).is_some(),
            "the newer unreferenced entry (on page 1) should have survived"
        );
    }

    #[test]
    fn eviction_never_touches_referenced_or_eternal_entries_on_any_page() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 3,
        });
        // Same two-phase shape as the single-page `eviction_never_touches_referenced_or_
        // eternal_entries`, generalised across pages and to cover `eternal` too: fill every
        // page completely with *only* non-evictable content first (half referenced, half
        // eternal), so pressure is genuine (nothing evictable exists yet) rather than
        // artificial. Interleaving already-zero-ref entries into this same fill loop would be
        // wrong — eviction would just keep recycling them as they're added, and by the time
        // the pool saturates none would remain resident (that's `exhaustion_across_every_
        // page_still_fails_gracefully`'s scenario, not this one).
        let mut referenced = Vec::new();
        let mut eternal = Vec::new();
        let mut i = 0;
        loop {
            let is_eternal = i % 2 == 1;
            let Some(id) = reg.register_static(8, 8, is_eternal) else {
                break;
            };
            if is_eternal {
                eternal.push(id);
            } else {
                reg.incref(id);
                referenced.push(id);
            }
            i += 1;
        }
        assert!(!referenced.is_empty() && !eternal.is_empty());
        assert!(
            reg.register_static(8, 8, false).is_none(),
            "exhausted while everything is either referenced or eternal"
        );

        // Make half the *referenced* entries eviction-eligible; eternal entries can't be
        // "un-eternaled" by any API, so they stay permanently protected throughout.
        let mut still_referenced = Vec::new();
        let mut now_unreferenced = Vec::new();
        for (i, id) in referenced.into_iter().enumerate() {
            if i % 2 == 0 {
                still_referenced.push(id);
            } else {
                reg.decref(id);
                now_unreferenced.push(id);
            }
        }
        assert!(
            !now_unreferenced.is_empty(),
            "test setup needs an evictable entry"
        );

        // Plenty of unreferenced entries exist to evict, spread across every page — this
        // should succeed by reclaiming one of those, never a referenced or eternal entry.
        assert!(
            reg.register_static(8, 8, false).is_some(),
            "should succeed by evicting an unreferenced entry"
        );
        for id in &still_referenced {
            assert!(
                reg.main_atlas_region(*id).is_some(),
                "referenced entry must never be evicted, on any page"
            );
        }
        for id in &eternal {
            assert!(
                reg.main_atlas_region(*id).is_some(),
                "eternal entry must never be evicted, on any page"
            );
        }
    }

    #[test]
    fn regions_on_different_pages_may_share_an_offset_without_conflicting() {
        // Pages 1 and 2 both lack the origin guard (only page 0 has it), so filling
        // page 0 completely and continuing spills onto page 1, then page 2 — and the
        // very first tile placed on each of those guard-free pages naturally lands at
        // the same (0, 0) offset. Proves the two pages are independent namespaces.
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 3,
        });
        let mut first_on_page = std::collections::HashMap::new();
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            let page = reg.main_atlas_region(id).unwrap().page;
            first_on_page.entry(page).or_insert(id);
            ids.push(id);
        }
        let a = *first_on_page.get(&1).expect("page 1 should have been used");
        let b = *first_on_page.get(&2).expect("page 2 should have been used");

        let ra = reg.main_atlas_region(a).unwrap();
        let rb = reg.main_atlas_region(b).unwrap();
        assert_eq!(
            (ra.x, ra.y),
            (rb.x, rb.y),
            "first allocation on each guard-free page should share the same offset"
        );
        assert_ne!(ra.page, rb.page);

        // Freeing one must not disturb the other, despite the identical (x, y).
        reg.decref(a);
        reg.free(a);
        assert!(reg.main_atlas_region(a).is_none());
        assert_eq!(
            reg.main_atlas_region(b),
            Some(rb),
            "freeing a's region must not affect b's, even though they share (x, y) on a \
             different page"
        );
    }

    #[test]
    fn freeing_returns_space_to_its_own_page_only() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 64,
            page_count: 2,
        });
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            ids.push(id);
        }
        // Every page is full and referenced now.
        assert!(reg.register_static(8, 8, false).is_none());

        let on_page_1 = *ids
            .iter()
            .find(|&&id| reg.main_atlas_region(id).unwrap().page == 1)
            .expect("test needs an entry on page 1");
        reg.decref(on_page_1);
        reg.free(on_page_1);

        // The freed space is on page 1 — the very next registration should land there,
        // not fail (which it would if free_internal mis-routed the free to the wrong
        // page's allocator, leaving page 1 still "full" as far as etagere is concerned).
        let id = reg
            .register_static(8, 8, false)
            .expect("freed page-1 space should be immediately reusable");
        assert_eq!(reg.main_atlas_region(id).unwrap().page, 1);
    }

    #[test]
    fn emptying_a_page_of_many_small_tiles_fully_reclaims_its_space() {
        // Pins the empirical finding `free_internal`'s doc comment describes: after fully
        // emptying a page fragmented by many small allocations, `etagere` already recovers
        // the *entire* original space, not just room for same-size pieces — no explicit
        // "reset the allocator" step is needed to get this. If an `etagere` upgrade
        // changed this, a much larger allocation (comfortably bigger than any one of the
        // small tiles that filled the page, but well within its total area) would start
        // failing here.
        let mut reg = TextureRegistry::new(single_page(64));
        let mut ids = Vec::new();
        while let Some(id) = reg.register_static(8, 8, false) {
            reg.incref(id);
            ids.push(id);
        }
        assert!(
            ids.len() > 1,
            "test needs multiple small tiles to fragment the page"
        );

        for id in ids {
            reg.decref(id);
            reg.free(id);
        }

        assert!(
            reg.register_static(48, 48, false).is_some(),
            "a fully emptied page should recover its entire original space, not just \
             room for pieces the size of what used to occupy it"
        );
    }

    #[test]
    fn page_count_is_clamped_to_at_least_one() {
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 256,
            page_count: 0,
        });
        assert_eq!(reg.page_count(), 1);
        assert!(reg.register_static(16, 16, false).is_some());
    }

    #[test]
    fn sustained_registration_churn_keeps_the_resident_set_bounded() {
        // The "bounded working set" property: registering far more images than the pool
        // could ever hold simultaneously should keep succeeding forever (eviction
        // keeping pace), never failing once the pool first saturates.
        let mut reg = TextureRegistry::new(AtlasConfig {
            page_size: 128,
            page_count: 2,
        });
        for i in 0..500 {
            let id = reg
                .register_static(32, 32, false)
                .unwrap_or_else(|| panic!("registration {i} should not fail — eviction should keep the working set bounded, not let it fail once saturated"));
            // Never referenced — immediately eviction-eligible, simulating a stream of
            // transient content (e.g. repeated gallery re-fetches) rather than content
            // that stays pinned forever.
            let _ = id;
            reg.advance_frame();
        }
        // The pool's page count never grew to accommodate this — it's still exactly 2.
        assert_eq!(reg.page_count(), 2);
    }

    #[test]
    fn the_gallery_workload_fits_in_the_default_page_pool() {
        // The demo's real working set (light and dark logo frames, tiles, backgrounds,
        // baked text, and 12 gallery images at the demo's `MAX_TILE_IMAGE_SIDE_PX`) must
        // all fit at once in the default pool. Everything stays referenced, as with real
        // `TextureRef`s, so this only passes if the capacity, not eviction, holds it.
        let mut reg = TextureRegistry::new(AtlasConfig::default());
        let mut register = |w: u32, h: u32| {
            let id = reg
                .register_static(w, h, true)
                .unwrap_or_else(|| panic!("failed to register a {w}x{h} region — the default page pool no longer fits the demo's real working set"));
            reg.incref(id);
        };

        for _ in 0..38 {
            register(220, 220); // light + dark animated logo frames, resized to LOGO_FRAME_MAX_SIDE
        }
        for _ in 0..3 {
            register(400, 400); // video tile box art
        }
        for _ in 0..2 {
            register(400, 400); // light/dark background crossfade layers
        }
        for _ in 0..40 {
            register(200, 40); // assorted baked text runs (nav/tile labels, etc.)
        }
        for _ in 0..12 {
            register(400, 400); // gallery images at MAX_TILE_IMAGE_SIDE_PX
        }
    }

    #[test]
    fn register_video_is_metadata_only_and_always_active() {
        let mut reg = TextureRegistry::new(single_page(256));
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

    // A finished video's registry entry must actually go away.
    //
    // `QuadPipeline::suspend_video` only marks the entry `Evicted`, so that it
    // can resume, and nothing else reclaims it: eviction only considers
    // `main_atlas` entries (`is_eviction_candidate` requires `AtlasRegion::Main`).
    // Unless stopping frees it, every play leaves a permanent row in the
    // slotmap.
    #[test]
    fn a_video_entry_can_be_freed_once_playback_is_over() {
        let mut reg = TextureRegistry::new(single_page(256));
        let id = reg.register_video(1280, 720);
        assert!(reg.info(id).is_some());

        // What `suspend_video` does on its own: still registered, just not
        // safe to sample.
        reg.mark_suspended(id);
        assert!(!reg.is_active(id));
        assert!(
            reg.info(id).is_some(),
            "suspend alone must not drop the entry — it's resumable"
        );

        // What stopping for good also does.
        reg.free(id);
        assert!(
            reg.info(id).is_none(),
            "a freed video entry must be gone from the registry"
        );
    }

    #[test]
    fn main_atlas_uv_is_normalized_and_within_unit_range() {
        let mut reg = TextureRegistry::new(single_page(1024));
        let id = reg.register_static(64, 32, false).unwrap();
        let uv = reg.main_atlas_uv(id).unwrap();
        assert!((0.0..=1.0).contains(&uv.uv_offset[0]));
        assert!((0.0..=1.0).contains(&uv.uv_offset[1]));
        assert!(uv.uv_scale[0] > 0.0 && uv.uv_scale[0] <= 1.0);
        assert!(uv.uv_scale[1] > 0.0 && uv.uv_scale[1] <= 1.0);
        assert_eq!(uv.page, 0);
    }
}
