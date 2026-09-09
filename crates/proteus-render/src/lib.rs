//! `proteus-render` — Layer 1: scene graph and instanced GPU render pipeline.
//!
//! Builds on [`proteus_gpu`] to provide:
//! - The instanced quad pipeline — one buffer upload, one draw call per frame
//! - The WGSL shader set (SDF corner radius, borders, texture crossfade)
//! - The texture registry (reference counting, LRU eviction)
//! - The offscreen render-to-texture pipeline used by static and transition bakes

// This crate's RGBA8-pixel code walks pixels via `chunks_exact(4)`/
// `chunks_exact_mut(4)` at several call sites (`static_texture`,
// `font_atlas`) — exactly what clippy's `chunks_exact_to_as_chunks` lint
// (new as of a stable release newer than every toolchain this workspace
// has otherwise needed so far — CI's `dtolnay/rust-toolchain@stable`
// always tracks current stable, so it saw this before any locally
// installed toolchain did) wants written as `as_chunks::<4>()` instead. A
// pure style suggestion, not a correctness one, and not worth bumping this
// crate's effective MSRV to adopt across every call site for. `unknown_
// lints` is allowed alongside it, in this order, since an older local
// clippy that's never heard of `chunks_exact_to_as_chunks` would otherwise
// turn *this very allow* into a hard error under `-D warnings`.
#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

pub mod font_atlas;
pub mod main_atlas_allocator;
pub mod material;
pub mod mesh;
pub mod pipeline;
pub mod scene;
pub mod static_texture;
pub mod texture_registry;
pub mod transition_atlas;

pub use font_atlas::{FontAtlas, RasterizedGlyphs, EMBEDDED_FONT_BYTES};
pub use main_atlas_allocator::{MainAtlasAllocId, MainAtlasAllocator, MainAtlasRegion};
pub use mesh::{
    pack_atlas_page, unpack_atlas_page, QuadInstance, QuadVertex, ATLAS_PAGE_SHIFT,
    ATLAS_SELECTOR_MAIN, ATLAS_SELECTOR_MASK, ATLAS_SELECTOR_TRANSITION, ATLAS_SELECTOR_VIDEO,
    QUAD_INDICES, QUAD_VERTICES,
};
pub use pipeline::{
    validate_atlas_config, GpuContext, QuadPipeline, VideoFrameSender, DEFAULT_VIDEO_HEIGHT,
    DEFAULT_VIDEO_WIDTH, MAIN_ATLAS_PAGE_COUNT, MAIN_ATLAS_SIZE, TRANSITION_ATLAS_SIZE,
};
pub use static_texture::{decode_image, resize_to_fit, DecodedImage};
pub use texture_registry::{
    AtlasConfig, AtlasRegion, MainAtlasPlacement, MainAtlasUv, TextureId, TextureKind,
    TextureRegistry, TextureState,
};
pub use transition_atlas::{TransitionAllocId, TransitionAtlasAllocator, TransitionRegion};

/// The WGSL source for the instanced quad shader, embedded at compile time.
pub const QUAD_SHADER_SRC: &str = include_str!("shaders/quad.wgsl");
