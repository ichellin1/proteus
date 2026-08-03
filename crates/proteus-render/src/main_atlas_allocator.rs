//! Sub-region allocator for `main_atlas` (M11 — Resource Management).
//!
//! Structurally identical to [`crate::transition_atlas::TransitionAtlasAllocator`] — both wrap
//! `etagere::AtlasAllocator`, a maintained, dependency-free shelf-based packer supporting real
//! `allocate`/`deallocate`. Before M11, `main_atlas` was packed by `FontAtlas`'s own append-only
//! shelf cursor (never freed); this allocator replaces that, giving `main_atlas` regions the same
//! real free/reuse `transition_atlas` regions already had.
//!
//! ## Guard region
//!
//! Unlike `TransitionAtlasAllocator`, [`MainAtlasAllocator::new`] immediately claims (and never
//! frees) a small region at the atlas origin. `QuadPipeline::create_atlases` writes a 1×1 white
//! pixel directly into `main_atlas` **layer 0** at `(0, 0)` outside of any allocator's
//! bookkeeping — without this guard, the very first real allocation on that layer could land
//! exactly on that texel and silently overwrite it (etagere's shelf packer starts allocating
//! from the origin).
//!
//! Since M11.2, `main_atlas` is a multi-page pool (one `MainAtlasAllocator` per array layer) —
//! the white-pixel sentinel only ever lives on layer 0, so only that layer's allocator needs the
//! guard. Layers 1.. use [`MainAtlasAllocator::new_without_guard`], where `(0, 0)` is ordinary
//! free space.

/// Opaque handle to one allocated region within `main_atlas`.
///
/// Returned by [`crate::texture_registry::TextureRegistry::register_static`]; freed automatically
/// by the registry's `free`/`evict_unused`/eviction-under-pressure paths — callers never handle
/// this directly.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MainAtlasAllocId(etagere::AllocId);

/// A `width × height` region of `main_atlas`, in atlas pixel coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MainAtlasRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Pixel dimensions of the guard region reserved at atlas origin — see the module docs.
const WHITE_PIXEL_GUARD_SIZE: u32 = 4;

/// Wraps `etagere::AtlasAllocator`, sized to [`crate::pipeline::MAIN_ATLAS_SIZE`].
pub struct MainAtlasAllocator {
    inner: etagere::AtlasAllocator,
}

impl MainAtlasAllocator {
    /// Allocator for `main_atlas` **layer 0**. Reserves the origin guard — see the module docs:
    /// `QuadPipeline::create_atlases` writes the 1×1 white-pixel sentinel to `(0, 0)` of layer 0
    /// outside any allocator's bookkeeping, and `QuadPipeline::WHITE_PIXEL_UV_OFFSET` hard-codes
    /// that location.
    pub fn new(size: u32) -> Self {
        Self::with_origin_guard(size, true)
    }

    /// Allocator for `main_atlas` layers **1..N** (M11.2). No origin guard: the white-pixel
    /// sentinel lives on layer 0 only, so `(0, 0)` on every other page is ordinary free space.
    pub fn new_without_guard(size: u32) -> Self {
        Self::with_origin_guard(size, false)
    }

    fn with_origin_guard(size: u32, guard: bool) -> Self {
        let mut inner = etagere::AtlasAllocator::new(etagere::size2(size as i32, size as i32));
        if guard {
            // Permanently reserve the origin corner — see the module docs. The
            // returned id is intentionally discarded: this region is never freed.
            let _ = inner.allocate(etagere::size2(
                WHITE_PIXEL_GUARD_SIZE as i32,
                WHITE_PIXEL_GUARD_SIZE as i32,
            ));
        }
        Self { inner }
    }

    /// Allocate a `width × height` region. Returns `None` if the atlas is full —
    /// callers should treat this the same as any other bake failure.
    pub fn allocate(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(MainAtlasAllocId, MainAtlasRegion)> {
        let alloc = self
            .inner
            .allocate(etagere::size2(width as i32, height as i32))?;
        let rect = alloc.rectangle;
        let region = MainAtlasRegion {
            x: rect.min.x as u32,
            y: rect.min.y as u32,
            width: rect.width() as u32,
            height: rect.height() as u32,
        };
        Some((MainAtlasAllocId(alloc.id), region))
    }

    /// Release a previously allocated region back to the packer.
    pub fn free(&mut self, id: MainAtlasAllocId) {
        self.inner.deallocate(id.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_returns_at_least_the_requested_size() {
        let mut a = MainAtlasAllocator::new(1024);
        let (_, region) = a.allocate(200, 200).expect("allocation should succeed");
        assert!(region.width >= 200, "width {} < 200", region.width);
        assert!(region.height >= 200, "height {} < 200", region.height);
    }

    #[test]
    fn successive_allocations_do_not_overlap() {
        let mut a = MainAtlasAllocator::new(1024);
        let (_, r1) = a.allocate(200, 200).unwrap();
        let (_, r2) = a.allocate(200, 200).unwrap();
        let overlap = r1.x < r2.x + r2.width
            && r1.x + r1.width > r2.x
            && r1.y < r2.y + r2.height
            && r1.y + r1.height > r2.y;
        assert!(!overlap, "allocations overlap: {r1:?} vs {r2:?}");
    }

    #[test]
    fn free_allows_the_space_to_be_reused() {
        // Fill the atlas by repeated small allocations rather than guessing
        // exact packing math — robust regardless of etagere's internal
        // shelf/guillotine layout.
        let mut a = MainAtlasAllocator::new(64);
        let mut ids = Vec::new();
        while let Some((id, _)) = a.allocate(8, 8) {
            ids.push(id);
        }
        assert!(!ids.is_empty(), "at least one allocation should have fit");
        assert!(a.allocate(8, 8).is_none(), "atlas should now be exhausted");

        let freed = ids.pop().unwrap();
        a.free(freed);
        assert!(a.allocate(8, 8).is_some(), "freed space should be reusable");
    }

    #[test]
    fn allocation_beyond_capacity_returns_none() {
        let mut a = MainAtlasAllocator::new(128);
        assert!(a.allocate(200, 200).is_none());
    }

    #[test]
    fn new_reserves_the_origin_guard_region() {
        let mut a = MainAtlasAllocator::new(64);
        // The guard already claimed a WHITE_PIXEL_GUARD_SIZE² corner, so the
        // first *caller* allocation must not land exactly at (0, 0).
        let (_, region) = a.allocate(4, 4).expect("allocation should succeed");
        assert_ne!(
            (region.x, region.y),
            (0, 0),
            "first real allocation must not overlap the reserved origin guard"
        );
    }

    #[test]
    fn new_without_guard_allows_allocating_at_the_origin() {
        // Mirror of `new_reserves_the_origin_guard_region` — a non-layer-0 page has no
        // white-pixel sentinel, so (0, 0) is ordinary free space and the very first
        // allocation should be able to land there.
        let mut a = MainAtlasAllocator::new_without_guard(64);
        let (_, region) = a.allocate(4, 4).expect("allocation should succeed");
        assert_eq!(
            (region.x, region.y),
            (0, 0),
            "a page without the origin guard should allocate starting at (0, 0)"
        );
    }
}
