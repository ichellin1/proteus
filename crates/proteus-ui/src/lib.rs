//! `proteus-ui` — Layer 2: the metamorphic component model.
//!
//! A component is an *identity* — a stable reference that exists independently
//! of its current visual form. Transitions declare a new target geometric
//! state; the framework morphs continuously between forms across three
//! topologies: 1→1, 1→N, and N→1.
//!
//! ## Key concepts
//!
//! - [`Component`] — an identity with geometric state and an interaction definition
//! - [`Transition`] — an interpolated morph between two geometric states,
//!   declared at the call site (duration, easing, delay)
//! - [`Signal`] — the trigger layer: `signal.set([to, from], config)` hands off
//!   to the ECS transition system, which drives the morph frame by frame

pub mod component;
pub mod signal;
pub mod transition;
