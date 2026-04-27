//! `proteus-context` — Layer 3: the context engine.
//!
//! Maintains the live user context model and drives adaptation decisions
//! for [`proteus_ui`] components.
//!
//! ## Key concepts
//!
//! - [`UserContext`] — the current snapshot: role, task, environment
//! - [`RoleRegistry`] — maps user identifiers to declared or inferred roles
//! - [`EnvironmentProbe`] — observes device, viewport, and input modality
//! - [`AdaptationEngine`] — evaluates rules to select component forms
//! - [`InferenceAdapter`] — optional trait for AI-augmented intent inference

pub mod user_context;
pub mod role;
pub mod environment;
pub mod adaptation;
pub mod inference;
