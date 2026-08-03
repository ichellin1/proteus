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
//! If every `main_atlas` page is full, [`TextureRegistry::register_static`] evicts unreferenced
//! (`ref_count == 0`), non-`eternal` entries — oldest-`last_used`-first, **globally across every
//! page**, not scoped to one — until either enough room opens or none remain eligible. It never
//! evicts a still-referenced entry: `BakedText`/`BakedImage`/`BakedComposite` cache their UV
//! coordinates directly on the component for a fast render path (no per-frame registry lookup),
//! so evicting a referenced region and letting a *different* texture reuse it later would make
//! the original component silently render the wrong (new occupant's) pixels — wrong content with
//! no error, not a crash. If unreferenced eviction isn't enough, registration simply fails
//! (`None`, logged), same as every other atlas-full path in this codebase.
//!
//! Evicting still-referenced content (with a restoration mechanism to make that safe — the
//! prerequisite for general atlas repacking/compaction too, not just eviction) and
//! backgrounding-driven eviction beyond video are Post-V1 — see `ROADMAP.md`.
//!
//! ## Multi-page pool (M11.2)
//!
//! `main_atlas` is a `wgpu::TextureViewDimension::D2Array` with a fixed page count decided at
//! construction (see [`AtlasConfig`]) — capacity scales with page count, but the pool never grows
//! after creation (a `D2Array`'s layer count can't grow without recreating the whole texture).
//! Instead, capacity is *held* bounded by eviction: "thousands of images available, not thousands
//! resident" is what makes unbounded content tractable, not an ever-growing pool. Each page is an
//! independent [`MainAtlasAllocator`]; allocation tries a preferred page first, then every other
//! page in order, then falls back to the eviction path above (still keyed by global recency, not
//! per-page).
//!
//! No explicit "repack/defragment a page" step exists, and testing found none is needed for the
//! case that seemed likeliest to require it: fully emptying a page (mixed-size regions, freed in
//! scattered order) already recovers its *entire* original space via `etagere`'s own coalescing —
//! see [`TextureRegistry::free_internal`]'s doc for how this was verified rather than assumed. The
//! harder case — repacking a page that's still *partially* full, with some content still
//! referenced — is a real gap, but not one this fixes: it needs the same referenced-content
//! relocation mechanism already deferred to Post-V1 above (moving a referenced region's pixels
//! without updating every component's cached UV silently corrupts rendering). See `ROADMAP.md`.

use crate::main_atlas_allocator::{MainAtlasAllocId, MainAtlasAllocator, MainAtlasRegion};

/// Configuration for `main_atlas`'s sizing — how big each page is, and how many pages exist.
///
/// Defaults (`2048`, `4`) match what's safe on every target this project currently ships to,
/// WebGL2 included. Override only when you know your deployment target's real headroom: a
/// native-only build targeting a desktop/TV GPU can raise `page_size` well past 2048 (native's
/// plain `wgpu::Limits::default()` guarantees 8192); a memory-constrained or video-free target
/// can lower `page_count` (each page is `page_size² × 4` bytes, eagerly committed at texture
/// creation — wgpu cannot lazily back array layers).
///
/// **Known limitation, not fixed by this type**: `page_size` is still bounded by whatever the
/// real device's `max_texture_dimension_2d` allows — 2048 on every WebGL2/downlevel target. A
/// single baked region (one image, one text run, one composite) can never exceed `page_size` on
/// any axis, so a genuinely UHD-resolution (3840×2160+) source image cannot be baked as one
/// contiguous region on web today, no matter how this is tuned. Closing that gap for real would
/// mean tiling a large source image across multiple ≤`page_size` regions at decode time and
/// stitching them at render time — a materially bigger feature, not built here; see `ROADMAP.md`'s
/// Post-V1 entry.
///
/// Validate a chosen config against the real device before use — see
/// [`crate::validate_atlas_config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasConfig {
    pub page_size: u32,
    pub page_count: u32,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            page_size: crate::MAIN_ATLAS_SIZE,
            page_count: crate::MAIN_ATLAS_PAGE_COUNT,
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
    /// A sub-region of one `main_atlas` array layer, owned by that layer's [`MainAtlasAllocator`].
    Main {
        alloc_id: MainAtlasAllocId,
        /// Which `main_atlas` array layer ("page") this region lives on (M11.2) — indexes
        /// `TextureRegistry::main_atlas_pages`, and is the value encoded into the high bits of
        /// `QuadInstance::atlas_page` at render time (see `mesh::pack_atlas_page`).
        page: u32,
        x: u32,
        y: u32,
    },
    /// The whole `video_atlas` texture *is* the resource — no packed sub-region.
    Video,
}

/// Where a `Static` entry's pixels physically live in `main_atlas` — which array layer ("page"),
/// and the pixel rect within it. Returned by [`TextureRegistry::main_atlas_region`] and consumed
/// directly by [`crate::QuadPipeline::write_to_main_atlas`]/
/// [`crate::QuadPipeline::bake_instances_to_main_atlas`].
///
/// `width`/`height` are the caller's originally requested content dimensions, not the allocator's
/// possibly-padded footprint — see [`AtlasRegion`] for why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainAtlasPlacement {
    pub page: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A `Static` entry's `main_atlas` region as normalised UVs plus its page index — exactly the
/// three values a `proteus_ui::BakedImage`/`BakedText`/`BakedComposite` stores. Destructure it to
/// fill one of those with field-init shorthand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MainAtlasUv {
    pub page: u32,
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
}

#[derive(Debug)]
struct TextureEntry {
    kind: TextureKind,
    atlas_region: AtlasRegion,
    ref_count: u32,
    /// Opts out of eviction-under-pressure. Both shells' animated-logo frame registrations pass
    /// `eternal: true` (they must survive the whole idle loop, not just whichever frame is
    /// currently shown); every other `register_static` call passes `false`.
    /// `register_video`'s single slot is always `eternal` since there's no packed region for
    /// eviction to reclaim anyway.
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
/// (`Video`). A *metadata* store plus the CPU-side `main_atlas` sub-region allocators — the actual
/// `wgpu::Texture` objects live in [`crate::QuadPipeline`], which owns one `TextureRegistry`.
pub struct TextureRegistry {
    entries: slotmap::SlotMap<TextureId, TextureEntry>,
    /// One allocator per `main_atlas` array layer (M11.2). `Vec` index == GPU array-layer index
    /// == the `page` stored in `AtlasRegion::Main`. Fixed at construction — see the module docs'
    /// "Multi-page pool" section for why this doesn't grow.
    main_atlas_pages: Vec<MainAtlasAllocator>,
    /// Every page's width/height in pixels (all pages are the same size — see [`AtlasConfig`]).
    /// Kept here (rather than asking a `MainAtlasAllocator`) so [`Self::main_atlas_uv`] can
    /// normalise UVs without the caller needing to separately track/pass the configured size.
    page_size: u32,
    /// Page tried first by the next allocation — whichever page satisfied the previous one. Pure
    /// optimisation: in the common case (steady-state registration into a page with room) this
    /// makes allocation a single `etagere` call instead of a scan. Never affects correctness —
    /// every page is tried before allocation is declared failed.
    preferred_page: usize,
    frame_counter: u64,
}

impl TextureRegistry {
    /// Create an empty registry backing a `main_atlas` pool sized per `config` (see
    /// [`AtlasConfig`]). `page_count` is clamped to at least 1. Layer 0's allocator reserves the
    /// origin guard (the 1×1 white-pixel sentinel lives there); layers 1.. do not — see
    /// [`MainAtlasAllocator::new_without_guard`].
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

    /// Register a `width × height` region of `main_atlas` for permanent content (baked text,
    /// images, or composites). Returns `None` if every page has no room even after evicting every
    /// eligible unreferenced entry, globally, across the whole pool (see the module docs'
    /// eviction-safety note).
    ///
    /// `ref_count` starts at 0 — the caller inserts a `proteus_ui::TextureRef` component
    /// immediately after, which increments it to 1 via its `ComponentHooks`, so a freshly
    /// registered entry is never observably zero for longer than one call.
    ///
    /// `eternal: true` opts this entry out of eviction-under-pressure entirely.
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

    /// Try `preferred_page` first, then every other page in ascending order. Returns the page that
    /// satisfied the request and updates `preferred_page` to it. Never evicts — an
    /// all-pages-full result is what escalates to [`Self::evict_to_make_room`].
    ///
    /// Deliberately first-fit-by-page rather than best-fit-by-remaining-space: `etagere` has no
    /// cheap "remaining space" query, the pool is single-digit pages, and first-fit keeps the
    /// steady-state (page has room) path to exactly one allocator call.
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

    /// Internal-only: called from [`register_static`](Self::register_static) when
    /// [`allocate_across_pages`](Self::allocate_across_pages) fails on every page. Frees
    /// unreferenced, non-`eternal` `Static` entries **globally oldest-`last_used`-first across
    /// every page** — not scoped to one — retrying the allocation after each free (preferring
    /// whichever page was just freed), and returns as soon as one succeeds. Never considers
    /// `ref_count > 0` entries (see the module docs). If every eligible entry across every page is
    /// freed and the allocation still doesn't fit, returns `None` — the caller's registration
    /// simply fails, matching every other atlas-full path in this codebase. The pool does not
    /// grow: its page count is fixed at construction (see [`AtlasConfig`]).
    ///
    /// Intentionally *not* public: it's inherently tied to one specific in-progress allocation
    /// attempt (a size to satisfy), an awkward, leaky shape for a general-purpose API —
    /// [`evict_unused`](Self::evict_unused)/[`free`](Self::free) cover the actual developer-facing
    /// use cases without exposing this internal.
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

    /// Removes `id` and frees its `main_atlas` region (a no-op for a `Video` entry). Returns the
    /// page the region was released back to, or `None` for a `Video` entry / unknown id.
    ///
    /// No explicit "reset this page if it's now entirely empty" step: an earlier draft of this
    /// milestone added one (as a cheap, safe partial defragmentation — no live content ever
    /// moves, so none of the hazards around repacking *referenced* content apply), but direct
    /// testing against `etagere` — filling a page with both uniform and heterogeneous-sized
    /// regions, freeing them in scattered (non-LIFO) order, then comparing the next allocation's
    /// placement against a truly-fresh allocator of the same size — found the two identical in
    /// every case tried. `etagere` already fully reclaims a page's free space once every region on
    /// it is freed; an explicit reset would have paid a linear scan of every resident entry on
    /// every single free, for a defragmentation `etagere` was already doing for free. See
    /// `ROADMAP.md`'s Post-V1 notes for the fragmentation case this *doesn't* cover — a page
    /// that's partially (not fully) empty, with some content still referenced.
    fn free_internal(&mut self, id: TextureId) -> Option<u32> {
        let entry = self.entries.remove(id)?;
        let AtlasRegion::Main { alloc_id, page, .. } = entry.atlas_region else {
            return None;
        };
        // `get_mut` rather than indexing: `page` is always in range today (it only ever comes
        // from `allocate_across_pages`, which can't return an out-of-bounds index), but a stale
        // value must not panic the render loop.
        self.main_atlas_pages.get_mut(page as usize)?.free(alloc_id);
        Some(page)
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

    /// The [`MainAtlasPlacement`] (page plus pixel rect) for a `Static` entry, ready to pass to
    /// [`crate::QuadPipeline::write_to_main_atlas`]/
    /// [`crate::QuadPipeline::bake_instances_to_main_atlas`]. `width`/`height` are the caller's
    /// originally requested content dimensions (see the [`AtlasRegion`] docs for why, not the
    /// allocator's possibly-padded footprint). `None` for a `Video` entry or an unknown id.
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

    /// UV offset/scale for a `Static` entry's `main_atlas` region, normalised by this registry's
    /// own configured page size (see [`AtlasConfig`] — there's no need for the caller to track or
    /// pass that separately, which would otherwise be easy to get out of sync with a non-default
    /// config), plus the page those UVs address. `None` for a `Video` entry or an unknown id.
    pub fn main_atlas_uv(&self, id: TextureId) -> Option<MainAtlasUv> {
        let p = self.main_atlas_region(id)?;
        let s = self.page_size as f32;
        Some(MainAtlasUv {
            page: p.page,
            uv_offset: [p.x as f32 / s, p.y as f32 / s],
            uv_scale: [p.width as f32 / s, p.height as f32 / s],
        })
    }

    /// How many `main_atlas` array layers this registry manages.
    pub fn page_count(&self) -> u32 {
        self.main_atlas_pages.len() as u32
    }

    /// How many `main_atlas` regions are currently resident (registered and not yet freed or
    /// evicted), across every page. Observability, and the handle a "working set stays bounded
    /// under sustained churn" test needs.
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

    /// Single-page config — shorthand for tests unconcerned with the multi-page pool itself.
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
        // "reset the allocator" step is needed to get this. If a future `etagere` upgrade
        // regressed this, a much larger allocation (comfortably bigger than any one of the
        // small tiles that used to occupy the page, but well within its total area) would
        // start failing here.
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
        // Real-scale regression for the bug this milestone fixes: the demo's actual
        // working set (light+dark logo frames, tiles, backgrounds, baked text, and 12
        // gallery images at full MAX_TILE_IMAGE_SIDE quality, not the emergency
        // GALLERY_IMAGE_MAX_SIDE downscale) must all fit simultaneously in the default
        // pool. Everything stays referenced (mirrors real `TextureRef` usage), so this
        // only passes if raw capacity — not eviction — is what accommodates it.
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
            register(400, 400); // gallery images at MAX_TILE_IMAGE_SIDE (real quality, not
                                // the emergency GALLERY_IMAGE_MAX_SIDE downscale)
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
