//! `proteus-render` — Layer 1: scene graph and instanced GPU render pipeline.
//!
//! Builds on [`proteus_gpu`] to provide:
//! - The instanced quad pipeline — one buffer upload, one draw call per frame
//! - The WGSL shader set (SDF corner radius, borders, texture crossfade)
//! - The texture registry (reference counting, LRU eviction)
//! - The offscreen render-to-texture pipeline used by static and transition bakes

pub mod font_atlas;
pub mod material;
pub mod mesh;
pub mod pipeline;
pub mod scene;

pub use font_atlas::{BakedRegion, FontAtlas, EMBEDDED_FONT_BYTES};
pub use mesh::{QuadInstance, QuadVertex, QUAD_INDICES, QUAD_VERTICES};
pub use pipeline::{QuadPipeline, MAIN_ATLAS_SIZE};

/// The WGSL source for the instanced quad shader, embedded at compile time.
pub const QUAD_SHADER_SRC: &str = include_str!("shaders/quad.wgsl");
