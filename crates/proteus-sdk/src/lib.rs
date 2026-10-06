//! The app-authoring API for Proteus: create components, connect them, and
//! transition between them.
//!
//! [`Proteus`] holds an app's components, channels and callbacks. The handles it
//! returns ([`Handle`], [`TransitionChannel`], [`TextureHandle`]) are small `Copy` IDs,
//! and their methods take the `Proteus` they came from:
//!
//! ```
//! use glam::Vec2;
//! use proteus_sdk::{ComponentSpec, Proteus, QuadState, TransitionConfig};
//!
//! let mut app = Proteus::new();
//!
//! let button = app.component(ComponentSpec::new(QuadState {
//!     size: Vec2::new(160.0, 48.0),
//!     ..QuadState::default()
//! }));
//! let panel = app.component(
//!     ComponentSpec::new(QuadState {
//!         size: Vec2::new(480.0, 320.0),
//!         ..QuadState::default()
//!     })
//!     .visible(false),
//! );
//!
//! // Clicking the button transitions it into the panel.
//! let open = app.transition_channel(None);
//! button.on_click(&mut app, move |app| {
//!     open.set(app, panel, button, TransitionConfig::default(), false);
//! });
//!
//! // Click the button, then let the transition start.
//! app.pointer_moved(Some(Vec2::ZERO));
//! app.pointer_pressed();
//! app.tick(1.0 / 60.0);
//! app.tick(1.0 / 60.0);
//! assert!(app.get(panel).unwrap().transition.is_some());
//! ```
//!
//! | Call | What it does |
//! |---|---|
//! | [`Proteus::component`] | Creates a component from a [`ComponentSpec`] |
//! | [`Proteus::transition_channel`], [`TransitionChannel::set`] | Transitions one component into another (1→1) |
//! | [`Handle::split_to`], [`Handle::merge_from`] | Transitions one component into many (1→N), or many into one (N→1) |
//! | [`Handle::animate_to`] | Transitions a component to new geometry |
//! | [`Proteus::get`] | Reads a component's current state |
//! | [`Proteus::tick`] | Advances the app and runs callbacks |
//!
//! This crate draws nothing by itself. To put an app on screen, implement
//! `proteus_runtime::App` and run it on a host: `proteus-host-winit` natively, or
//! `proteus-host-web` in the browser. The TypeScript SDK exposes this same API.

#![warn(missing_docs)]

mod app;
mod callback;
mod data;
mod handle;
mod spec;

pub use app::Proteus;
pub use data::{ComponentData, TransitionData};
pub use handle::{Handle, HandleError, TextureHandle, TextureRequest, TransitionChannel};
pub use spec::ComponentSpec;

// The value types this API takes and returns, so an app needs no direct
// `proteus-ui` dependency.
pub use proteus_ui::{
    Border, DropReason, DropShadow, Easing, Glow, Image, ImageCrop, InteractionStateKind,
    MergeLayout, Opacity, QuadState, SplitStrategy, StyleOverride, Text, TransitionConfig,
    TransitionDropped, TransitionInteractionConfig, Visibility,
};
