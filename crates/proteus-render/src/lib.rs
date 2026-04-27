//! `proteus-render` — Layer 1: retained-mode scene graph and GPU render pipeline.
//!
//! Builds on [`proteus_gpu`] to provide:
//! - A mesh registry for UI geometry (quads, paths, glyphs, arbitrary meshes)
//! - A material system with hot-reloadable WGSL shaders
//! - A compute pipeline for physics-driven transitions (springs, fluid, morphing)
//! - An SDF renderer for resolution-independent shapes and typography

pub mod mesh;
pub mod material;
pub mod scene;
