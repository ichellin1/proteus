//! Sub-region allocator for `transition_atlas`.
//!
//! `main_atlas`'s only occupant that needs dynamic packing is [`crate::font_atlas::FontAtlas`],
//! which uses its own simple shelf packer (baked text glyphs live for the entity's lifetime,
//! never freed individually). `transition_atlas` is different: every region is ephemeral —
//! allocated when a Slice group transition starts, freed the moment it completes — and
//! multiple transitions can be in flight at once. That needs a real allocator that supports
//! `allocate`/`deallocate`, not just append-only packing. `etagere::AtlasAllocator` is exactly
//! that (a maintained, dependency-free shelf-based packer); this module wraps it so
//! `etagere` stays an implementation detail of `proteus-render` — `proteus-ui` only ever
//! sees the opaque [`TransitionAllocId`] handle.

/// Opaque handle to one allocated region within `transition_atlas`.
///
/// Returned by [`crate::QuadPipeline::allocate_transition_region`]; pass back to
/// [`crate::QuadPipeline::free_transition_region`] once the region's content is no longer
/// needed (e.g. when a group transition completes).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransitionAllocId(etagere::AllocId);

/// A `width × height` region of `transition_atlas`, in atlas pixel coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TransitionRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl TransitionRegion {
    /// As an `(x, y, width, height)` tuple — the shape [`crate::QuadPipeline::bake_instances_to_transition_atlas`]
    /// and [`crate::QuadPipeline::bake_instances_to_main_atlas`] already take.
    pub fn as_tuple(&self) -> (u32, u32, u32, u32) {
        (self.x, self.y, self.width, self.height)
    }
}

/// Transparent gutter (in atlas pixels) reserved on every side of each
/// allocation, so a rounded corner's near-edge bilinear sample lands in
/// guaranteed-transparent padding that `QuadPipeline::bake_instances_to_atlas`
/// writes as part of *this* bake, rather than in an adjacent allocation's
/// opaque content (or stale pixels from a since-freed one). Without this, a
/// Slice-transition entity's rounded corner reads as "slightly opaque"
/// instead of transparent whenever another baked region happens to land
/// pixel-adjacent to it — see the corner-bleed bug this was added to fix.
///
/// Also used by `pipeline.rs`'s bake path (kept in sync via this constant —
/// the allocator reserves the space, the pipeline is what actually paints it
/// transparent).
pub(crate) const TRANSITION_BAKE_PAD: u32 = 2;

/// Wraps `etagere::AtlasAllocator`, sized to [`crate::TRANSITION_ATLAS_SIZE`].
pub struct TransitionAtlasAllocator {
    inner: etagere::AtlasAllocator,
}

impl TransitionAtlasAllocator {
    pub fn new(size: u32) -> Self {
        Self {
            inner: etagere::AtlasAllocator::new(etagere::size2(size as i32, size as i32)),
        }
    }

    /// Allocate a `width × height` region, reserving an extra
    /// [`TRANSITION_BAKE_PAD`]-pixel transparent gutter on every side (not
    /// reflected in the returned [`TransitionRegion`] — callers still bake
    /// and UV-address exactly the `width × height` they asked for; the
    /// gutter is an implementation detail shared only with
    /// `QuadPipeline::bake_instances_to_atlas`). Returns `None` if the atlas
    /// is full — callers should treat this the same as any other bake
    /// failure (e.g. skip the crossfade and fall back to flat-color geometry
    /// for that slice).
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

    /// Release a previously allocated region back to the packer.
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
