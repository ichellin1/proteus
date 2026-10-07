//! The Proteus component model and transition engine, built on `bevy_ecs`.
//!
//! A component is an identity that lasts, whatever its current appearance.
//! A transition gives it new geometry, and the engine moves it there
//! continuously, in one of three shapes: 1→1, 1→N and N→1. App code usually
//! uses this through `proteus-sdk`.
//!
//! ## Key concepts
//!
//! - [`component::QuadState`]: a component's geometry, interpolated during
//!   transitions.
//! - [`component::Lifecycle`]: whether it is `Idle` or `Transitioning`.
//! - [`transition::ActiveTransition`]: a transition in progress.
//! - [`transition::TransitionConfig`]: a transition's duration, delay and
//!   easing.
//! - [`transition::CompletedTransitions`]: the transitions that finished this
//!   tick.
//! - [`schedule::ProteusWorld`]: the ECS world and schedule; `update(dt)` runs
//!   one tick.

#![warn(missing_docs)]

pub mod bake;
pub mod channel;
pub mod collect;
pub mod component;
pub mod effects;
pub mod hierarchy;
pub mod image;
pub mod input;
pub mod interaction;
pub mod schedule;
pub mod spawn_order;
pub mod text;
pub mod texture_ref;
pub mod topology;
pub mod transition;
pub mod video;

// Convenience re-exports for the most commonly used types.
pub use bake::{bake_system, Baked, BakedComposite};
pub use bevy_ecs::hierarchy::{ChildOf, Children};
pub use bevy_ecs::prelude::Entity;
pub use channel::{
    channel_dispatch_system, create_channel, destroy_channel, set as set_channel, ChannelRegistry,
    DropReason, DroppedRequests, OwnedChannels, TransitionChannelId, TransitionDropped,
};
pub use collect::{collect_instances, quad_state_to_instance, BakedTexture};
pub use component::{
    Disabled, Lifecycle, QuadState, TransitionInteractionConfig, TransitionRequest, Virtual,
    Visibility,
};
pub use effects::{Border, DropShadow, Glow};
pub use hierarchy::{
    opacity_system, resolve_world_position, resolve_world_position_query, visibility_system,
    EffectiveOpacity, EffectiveVisibility, Opacity,
};
pub use image::{BakedImage, Image, ImageCrop};
pub use input::{
    quad_contains, FocusState, HoveredEntity, Interactable, InteractionEvents, PointerInput,
    PressedEntity,
};
pub use interaction::{
    interaction_style_system, InteractionDef, InteractionState, InteractionStateKind, StyleOverride,
};
pub use schedule::{flush_commands_system, CommandQueue, ProteusSet, ProteusWorld};
pub use spawn_order::SpawnOrder;
pub use text::{BakedText, Text};
pub use texture_ref::{CompositeTextureRef, ImageTextureRef, TextTextureRef};
pub use topology::{
    ActiveGroupTransition, ChildConfigs, GroupSource, GroupTarget, MergeLayout, NToOneRequest,
    OneToNRequest, PartOfGroup, SplitStrategy, TransitionAtlasSize,
};
pub use transition::{ActiveTransition, CompletedTransitions, Easing, FrameTime, TransitionConfig};
pub use video::{VideoCrossfade, VideoPlayer};
