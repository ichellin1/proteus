//! Proteus's GPU rendering: the instanced quad pipeline, its shader, and the
//! texture atlases.
//!
//! It provides:
//! - the quad pipeline: one buffer upload and one draw call per frame;
//! - the WGSL shader: rounded corners, borders, shadows and glows, and fading
//!   between textures;
//! - the texture registry: reference counting and eviction;
//! - rendering into a texture, for baking components and transitions.

#![warn(missing_docs)]
// Pixel code uses `chunks_exact(4)`, which newer clippy suggests writing as
// `as_chunks::<4>()`. That is only a style change, and would raise the minimum
// Rust version. `unknown_lints` comes first, because an older clippy that
// doesn't know this lint would otherwise reject the `allow` itself.
#![allow(unknown_lints, clippy::chunks_exact_to_as_chunks)]

pub mod font_atlas;
pub mod main_atlas_allocator;
pub mod mesh;
pub mod pipeline;
pub mod static_texture;
pub mod texture_registry;
pub mod transition_atlas;

pub use font_atlas::{FontAtlas, FontError, RasterizedGlyphs, EMBEDDED_FONT_BYTES};
pub use main_atlas_allocator::{MainAtlasAllocId, MainAtlasAllocator, MainAtlasRegion};
pub use mesh::{
    pack_atlas_page, unpack_atlas_page, QuadInstance, QuadVertex, ATLAS_PAGE_SHIFT,
    ATLAS_SELECTOR_MAIN, ATLAS_SELECTOR_MASK, ATLAS_SELECTOR_TRANSITION, ATLAS_SELECTOR_VIDEO,
    QUAD_INDICES, QUAD_VERTICES,
};
pub use pipeline::{
    GpuContext, QuadPipeline, DEFAULT_MAIN_ATLAS_PAGE_COUNT, DEFAULT_MAIN_ATLAS_SIZE,
    DEFAULT_TRANSITION_ATLAS_SIZE, DEFAULT_VIDEO_HEIGHT, DEFAULT_VIDEO_WIDTH,
};
pub use static_texture::{decode_image, resize_to_fit, DecodedImage};
pub use texture_registry::{
    AtlasConfig, AtlasRegion, MainAtlasPlacement, MainAtlasUv, TextureId, TextureKind,
    TextureRegistry, TextureState,
};
pub use transition_atlas::{TransitionAllocId, TransitionAtlasAllocator, TransitionRegion};

/// The WGSL source for the instanced quad shader, embedded at compile time.
pub const QUAD_SHADER_SRC: &str = include_str!("shaders/quad.wgsl");
