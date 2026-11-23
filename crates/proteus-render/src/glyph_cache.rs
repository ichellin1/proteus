//! The coverage of each glyph, kept after it's rasterized so it can be used
//! again.
//!
//! [`FontAtlas`](crate::FontAtlas) rasterizes text one glyph at a time. Most
//! texts share glyphs with others, since every "e" in every label at 16 pixels
//! has the same pixels. The cache keeps each glyph's pixels, keyed by
//! everything that changes a glyph's pixels: the font, the glyph, its size in
//! pixels, the font's variation, such as its weight, its horizontal offset
//! within a pixel, in quarters, and whether it's slanted or thickened to stand
//! in for an italic or bold face.
//!
//! ## Eviction
//!
//! The cache has a budget in bytes. When a new glyph would take the cache past
//! the budget, the cache drops the glyphs used longest ago, until it holds at
//! most three quarters of the budget with the new glyph. Dropping a batch at a
//! time means it seldom has to sort the glyphs by age. A glyph that's dropped
//! and needed again is rasterized again, with the same pixels.
//!
//! A text's image gets its own copy of each glyph's pixels when the text is
//! baked. Nothing on screen depends on the cache, so any glyph can be dropped
//! at any time, and [`GlyphCache::clear`] drops them all.

use std::collections::HashMap;
use std::sync::Arc;

/// A rough count of the bytes each glyph costs beyond its pixels. It covers
/// the glyph's key, its position and size, and the map's own bookkeeping.
const ENTRY_OVERHEAD: usize = 64;

/// Everything that changes a glyph's pixels.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    /// The font file's id. The font's index within the file is `index`.
    pub font: u64,
    pub index: u32,
    pub glyph: u32,
    /// The size in pixels, as `f32` bits.
    pub size_bits: u32,
    /// The font's variation coordinates. This is empty for a font that doesn't
    /// vary.
    pub coords: Box<[i16]>,
    /// The glyph's horizontal offset within a pixel, in quarters of a pixel,
    /// from 0 to 3.
    pub quarter: u8,
    /// Whether the glyph is slanted to stand in for an italic face.
    pub slanted: bool,
    /// Whether the glyph is thickened to stand in for a bold face.
    pub emboldened: bool,
}

/// A rasterized glyph's coverage, and where the glyph sits relative to its pen
/// position, with y pointing down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CachedGlyph {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub coverage: Arc<[u8]>,
}

struct Entry {
    /// `None` for a glyph with no pixels, such as a space. Keeping it means
    /// the glyph isn't rasterized again.
    glyph: Option<CachedGlyph>,
    last_used: u64,
    bytes: usize,
}

/// A cache of glyphs' coverage by [`GlyphKey`]. Past a budget in bytes, it
/// drops the glyphs used longest ago. See the module docs.
pub(crate) struct GlyphCache {
    entries: HashMap<GlyphKey, Entry>,
    bytes: usize,
    budget: usize,
    /// Counts lookups. Each entry records the count at its last use.
    clock: u64,
}

impl GlyphCache {
    /// Creates an empty cache that keeps up to `budget` bytes of glyphs. A
    /// budget of 0 keeps none.
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            budget,
            clock: 0,
        }
    }

    /// Returns the glyph for `key`, from the cache if it's there. Otherwise
    /// this calls `rasterize` and keeps the result if it fits the budget.
    pub fn get_or_rasterize(
        &mut self,
        key: GlyphKey,
        rasterize: impl FnOnce() -> Option<CachedGlyph>,
    ) -> Option<CachedGlyph> {
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = self.clock;
            return entry.glyph.clone();
        }
        let glyph = rasterize();
        let bytes = ENTRY_OVERHEAD + glyph.as_ref().map_or(0, |g| g.coverage.len());
        if bytes > self.budget {
            return glyph;
        }
        if self.bytes + bytes > self.budget {
            self.evict_to((self.budget * 3 / 4).saturating_sub(bytes));
        }
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                glyph: glyph.clone(),
                last_used: self.clock,
                bytes,
            },
        );
        glyph
    }

    /// Drops the glyphs used longest ago until the cache holds at most
    /// `target` bytes.
    fn evict_to(&mut self, target: usize) {
        let mut by_age: Vec<(u64, GlyphKey)> = self
            .entries
            .iter()
            .map(|(key, e)| (e.last_used, key.clone()))
            .collect();
        by_age.sort_unstable_by_key(|(last_used, _)| *last_used);
        for (_, key) in by_age {
            if self.bytes <= target {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
            }
        }
    }

    /// Changes the budget. If the cache holds more than the new budget, it
    /// drops the glyphs used longest ago until it fits.
    pub fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
        if self.bytes > budget {
            self.evict_to(budget);
        }
    }

    /// Drops every glyph.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Returns the bytes the cache holds, counted as eviction counts them.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Returns the number of glyphs the cache holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(glyph: u32) -> GlyphKey {
        GlyphKey {
            font: 1,
            index: 0,
            glyph,
            size_bits: 16.0f32.to_bits(),
            coords: Box::new([]),
            quarter: 0,
            slanted: false,
            emboldened: false,
        }
    }

    // Returns a glyph with `size` bytes of coverage.
    fn glyph(size: usize) -> Option<CachedGlyph> {
        Some(CachedGlyph {
            left: 0,
            top: 0,
            width: size as u32,
            height: 1,
            coverage: vec![255; size].into(),
        })
    }

    // Each entry costs 36 bytes of pixels and 64 bytes of overhead, 100 in all.
    const ENTRY: usize = 36 + ENTRY_OVERHEAD;

    #[test]
    fn a_glyph_is_rasterized_once() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        let mut rasterized = 0;
        for _ in 0..3 {
            cache.get_or_rasterize(key(1), || {
                rasterized += 1;
                glyph(36)
            });
        }
        assert_eq!(rasterized, 1);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), ENTRY);
    }

    #[test]
    fn a_glyph_without_pixels_is_kept_too() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        let mut rasterized = 0;
        for _ in 0..2 {
            cache.get_or_rasterize(key(1), || {
                rasterized += 1;
                None
            });
        }
        assert_eq!(rasterized, 1);
    }

    #[test]
    fn the_cache_stays_within_its_budget() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        for g in 0..100 {
            cache.get_or_rasterize(key(g), || glyph(36));
            assert!(cache.bytes() <= 10 * ENTRY, "{} bytes", cache.bytes());
        }
    }

    // Past the budget, the glyphs used longest ago go first. A glyph that is
    // used again stays, while the glyphs added around the same time as it go.
    #[test]
    fn the_glyphs_used_longest_ago_are_dropped_first() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        for g in 0..10 {
            cache.get_or_rasterize(key(g), || glyph(36));
        }
        cache.get_or_rasterize(key(0), || panic!("glyph 0 is cached"));
        cache.get_or_rasterize(key(10), || glyph(36));

        let held = |g| cache.entries.contains_key(&key(g));
        assert!(held(0), "glyph 0, used again, was kept");
        assert!(held(10), "glyph 10, the newest, was kept");
        assert!(!held(1), "glyph 1, the oldest unused, was dropped");
        assert!(held(9), "the newest of the rest were kept");
    }

    #[test]
    fn a_glyph_larger_than_the_budget_isnt_kept() {
        let mut cache = GlyphCache::new(ENTRY);
        let g = cache.get_or_rasterize(key(1), || glyph(1000));
        assert!(g.is_some(), "it's still returned");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_budget_of_zero_keeps_nothing() {
        let mut cache = GlyphCache::new(0);
        cache.get_or_rasterize(key(1), || glyph(36));
        cache.get_or_rasterize(key(2), || None);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes(), 0);
    }

    #[test]
    fn a_smaller_budget_drops_glyphs_to_fit() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        for g in 0..10 {
            cache.get_or_rasterize(key(g), || glyph(36));
        }
        cache.set_budget(4 * ENTRY);
        assert!(cache.bytes() <= 4 * ENTRY);
        assert_eq!(cache.len(), 4);
    }

    #[test]
    fn clear_drops_every_glyph() {
        let mut cache = GlyphCache::new(10 * ENTRY);
        for g in 0..5 {
            cache.get_or_rasterize(key(g), || glyph(36));
        }
        cache.clear();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes(), 0);
    }
}
