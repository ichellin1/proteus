//! Space allocation within the transition atlas.
//!
//! Every transition-atlas region is short-lived: allocated when a split or
//! merge starts and freed when it ends, with several possibly in progress at
//! once. `etagere::AtlasAllocator`, a shelf packer that frees and reuses
//! regions, handles that; this module wraps it, so other crates only see the
//! [`TransitionAllocId`] handle.

/// Identifies one allocated transition-atlas region. Returned by
/// [`crate::QuadPipeline::allocate_transition_region`]; pass it to
/// [`crate::QuadPipeline::free_transition_region`] when the region isn't needed
/// any more.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransitionAllocId(etagere::AllocId);

/// A `width × height` region of `transition_atlas`, in atlas pixel coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TransitionRegion {
    /// Left edge, in pixels.
    pub x: u32,
    /// Top edge, in pixels.
    pub y: u32,
    /// Width, in pixels.
    pub width: u32,
    /// Height, in pixels.
    pub height: u32,
}

impl TransitionRegion {
    /// The region as `(x, y, width, height)`, as
    /// [`crate::QuadPipeline::bake_instances_to_transition_atlas`] and
    /// [`crate::QuadPipeline::bake_instances_to_main_atlas`] take it.
    pub fn as_tuple(&self) -> (u32, u32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }
}

/// A transparent border, in atlas pixels, reserved around every allocation.
///
/// A rounded corner's edge samples then read transparent pixels, which the
/// bake paints, rather than a neighboring region's content, which would make
/// the corner look slightly opaque. The allocator reserves the space and
/// `QuadPipeline::bake_instances_to_atlas` paints it.
pub(crate) const TRANSITION_BAKE_PAD: u32 = 2;

/// Allocates regions within the transition atlas, using
/// `etagere::AtlasAllocator`.
pub struct TransitionAtlasAllocator {
    inner: etagere::AtlasAllocator,
}

impl TransitionAtlasAllocator {
    /// Creates an allocator for a `size` × `size` atlas.
    pub fn new(size: u32) -> Self {
        Self {
            inner: etagere::AtlasAllocator::new(etagere::size2(size as i32, size as i32)),
        }
    }

    /// Allocates a `width × height` region, with a
    /// `TRANSITION_BAKE_PAD`-pixel transparent border reserved around it. The
    /// returned region is the requested size, without the border. Returns
    /// `None` if the atlas is full; the caller then uses a plain colored shape
    /// instead.
    pub fn allocate(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(TransitionAllocId, TransitionRegion)> {
        let pad = TRANSITION_BAKE_PAD;
        let alloc = self.inner.allocate(etagere::size2(
            (width + 2 * pad) as i32,
            (height + 2 * pad) as i32,
        ))?;
        let rect = alloc.rectangle;
        let region = TransitionRegion {
            x: rect.min.x as u32 + pad,
            y: rect.min.y as u32 + pad,
            width,
            height,
        };
        Some((TransitionAllocId(alloc.id), region))
    }

    /// Frees a region allocated by
    /// [`TransitionAtlasAllocator::allocate`].
    pub fn free(&mut self, id: TransitionAllocId) {
        self.inner.deallocate(id.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_returns_at_least_the_requested_size() {
        let mut a = TransitionAtlasAllocator::new(1024);
        let (_, region) = a.allocate(200, 200).expect("allocation should succeed");
        // Shelf packers commonly round up (bucketing/padding to reduce
        // fragmentation) — callers must use the *returned* region, not assume
        // it matches the request exactly.
        assert!(region.width >= 200, "width {} < 200", region.width);
        assert!(region.height >= 200, "height {} < 200", region.height);
    }

    #[test]
    fn successive_allocations_do_not_overlap() {
        let mut a = TransitionAtlasAllocator::new(1024);
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
        // Atlas sized to exactly fit one padded 256x256 request (256 + 2*pad).
        let mut a = TransitionAtlasAllocator::new(256 + 2 * TRANSITION_BAKE_PAD);
        let (id, _) = a
            .allocate(256, 256)
            .expect("first allocation fills the atlas");
        // Atlas is full — a second same-size allocation must fail.
        assert!(a.allocate(256, 256).is_none());
        a.free(id);
        // Freed — the same allocation should succeed again.
        assert!(a.allocate(256, 256).is_some());
    }

    #[test]
    fn allocation_beyond_capacity_returns_none() {
        let mut a = TransitionAtlasAllocator::new(128);
        assert!(a.allocate(200, 200).is_none());
    }
}
