//! Space allocation within one main-atlas page.
//!
//! It works like [`crate::transition_atlas::TransitionAtlasAllocator`]: both
//! wrap `etagere::AtlasAllocator`, a shelf packer that can free and reuse
//! regions.
//!
//! ## Guard region
//!
//! [`MainAtlasAllocator::new`] permanently reserves a small block at the
//! origin, where `QuadPipeline::create_atlases` paints the white pixel that
//! untextured components sample. Without it, the first allocation, which
//! `etagere` places at the origin, would overwrite it.
//!
//! Only page 0 has the white pixel, so only its allocator reserves the block.
//! The other pages use [`MainAtlasAllocator::new_without_guard`].

/// Identifies one allocated main-atlas region. The
/// [`crate::texture_registry::TextureRegistry`] allocates and frees these;
/// callers don't handle them.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MainAtlasAllocId(etagere::AllocId);

/// A region of a main-atlas page, in pixels.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MainAtlasRegion {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// The size, in pixels, of the block reserved at page 0's origin; see the
/// module docs. [`crate::QuadPipeline::create_atlases`] paints exactly this
/// block white, so the two must agree.
pub(crate) const WHITE_PIXEL_GUARD_SIZE: u32 = 4;

/// Allocates regions within one main-atlas page, using
/// `etagere::AtlasAllocator`.
pub struct MainAtlasAllocator {
    inner: etagere::AtlasAllocator,
}

impl MainAtlasAllocator {
    /// An allocator for page 0, which reserves the white-pixel block at the
    /// origin; see the module docs.
    pub fn new(size: u32) -> Self {
        Self::with_origin_guard(size, true)
    }

    /// An allocator for pages after the first, which reserves nothing: only
    /// page 0 has the white pixel.
    pub fn new_without_guard(size: u32) -> Self {
        Self::with_origin_guard(size, false)
    }

    fn with_origin_guard(size: u32, guard: bool) -> Self {
        let mut inner = etagere::AtlasAllocator::new(etagere::size2(size as i32, size as i32));
        if guard {
            // Reserve the origin corner for good; see the module docs. The ID is
            // discarded, since the region is never freed.
            let _ = inner.allocate(etagere::size2(
                WHITE_PIXEL_GUARD_SIZE as i32,
                WHITE_PIXEL_GUARD_SIZE as i32,
            ));
        }
        Self { inner }
    }

    /// Allocates a `width × height` region, or returns `None` if the page is
    /// full.
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

    /// Frees a region allocated by [`MainAtlasAllocator::allocate`].
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
        // The reverse of `new_reserves_the_origin_guard_region`: a page after the
        // first has no white pixel, so the first allocation can be at the
        // origin.
        let mut a = MainAtlasAllocator::new_without_guard(64);
        let (_, region) = a.allocate(4, 4).expect("allocation should succeed");
        assert_eq!(
            (region.x, region.y),
            (0, 0),
            "a page without the origin guard should allocate starting at (0, 0)"
        );
    }
}
