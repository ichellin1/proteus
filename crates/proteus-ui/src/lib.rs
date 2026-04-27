//! `proteus-ui` — Layer 2: the semantic component model.
//!
//! Components in this layer declare a *semantic role* rather than a fixed
//! visual form. The framework resolves the active visual expression at runtime
//! via the context bus supplied by [`proteus_context`].
//!
//! ## Key concepts
//!
//! - [`Component`] — a node with a semantic role and a set of possible visual forms
//! - [`Form`] — one concrete visual rendering of a component
//! - [`TransitionGraph`] — rules for how a component moves between forms
//! - [`ContextBus`] — the signal channel that drives form selection

pub mod component;
pub mod form;
pub mod transition;
pub mod context_bus;
