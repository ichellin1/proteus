//! `proteus-render` — Layer 1: scene graph and instanced GPU render pipeline.
//!
//! Builds on [`proteus_gpu`] to provide:
//! - The instanced quad pipeline — one buffer upload, one draw call per frame
//! - The WGSL shader set (SDF corner radius, borders, texture crossfade)
//! - The texture registry (reference counting, LRU eviction)
//! - The offscreen render-to-texture pipeline used by static and transition bakes

pub mod mesh;
pub mod material;
pub mod scene;
