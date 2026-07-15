//! `proteus-ui` — Layer 2: the metamorphic component model.
//!
//! A component is an *identity* — a stable reference that exists independently
//! of its current visual form. Transitions declare a new target geometric
//! state; the framework morphs continuously between forms across three
//! topologies: 1→1, 1→N, and N→1.
//!
//! ## Key concepts
//!
//! - [`component::QuadState`] — the visual geometry of one component, lerped during transitions
//! - [`component::Lifecycle`] — two-state machine: `Idle` / `Transitioning`
//! - [`transition::ActiveTransition`] — per-entity transition state managed by the ECS systems
//! - [`transition::TransitionConfig`] — duration, delay, easing declared at the call site
//! - [`transition::TransitionComplete`] — record of one completed transition
//! - [`transition::CompletedTransitions`] — resource; drain after `world.update()` to react
//! - [`schedule::ProteusWorld`] — the ECS world + schedule; call `update(dt)` once per frame

pub mod collect;
pub mod component;
pub mod effects;
pub mod input;
pub mod schedule;
pub mod signal;
pub mod text;
pub mod topology;
pub mod transition;
pub mod video;

// Convenience re-exports for the most commonly used types.
pub use bevy_ecs::prelude::Entity;
pub use collect::{collect_instances, quad_state_to_instance};
pub use component::{Lifecycle, QuadState, TransitionRequest, Virtual, Visibility};
pub use effects::{DropShadow, Glow};
pub use input::{quad_contains, Interactable, InteractionEvents, PointerInput};
pub use schedule::ProteusWorld;
pub use text::{BakedText, Text};
pub use topology::{
    ActiveGroupTransition, ChildBehaviorFn, GroupSource, GroupTarget, NToOneRequest, OneToNRequest,
    PartOfGroup, SplitStrategy,
};
pub use transition::{
    ease_in_out_quad, ease_in_quad, ease_out_cubic, ease_out_quad, linear, ActiveTransition,
    CompletedTransitions, EasingFn, FrameTime, TransitionComplete, TransitionConfig,
};
pub use video::VideoPlayer;
