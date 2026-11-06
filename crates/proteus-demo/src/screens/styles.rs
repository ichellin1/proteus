//! The demo's one interaction style: a glow that spreads, and a little
//! growth, while a surface is hovered with the pointer or focused with the
//! keys. The engine shows the hover style only while the pointer is in use,
//! and the focused style only while the keys are, so there's one highlight
//! at a time.

use proteus_sdk::{ComponentSpec, Easing, StyleOverride, TransitionConfig};

/// The glow's radius while lit.
pub const GLOW_PX: f32 = 15.0;
/// How much a lit surface grows.
pub const SCALE: f32 = 1.07;
/// How long a surface takes to light up or go back.
pub const SECS: f32 = 0.25;

/// `spec` with the demo's hover and focused style: a glow and a little
/// growth. The component needs a glow at rest, of radius 0, for the style to
/// grow.
pub fn lit(spec: ComponentSpec) -> ComponentSpec {
    with_style(
        spec,
        StyleOverride {
            scale: Some(SCALE),
            glow_radius: Some(GLOW_PX),
            ..Default::default()
        },
    )
}

/// `spec` with a glow alone while hovered or focused, for a surface already
/// as big as its space allows, such as the enlarged photo.
pub fn glow_only(spec: ComponentSpec) -> ComponentSpec {
    with_style(
        spec,
        StyleOverride {
            glow_radius: Some(GLOW_PX),
            ..Default::default()
        },
    )
}

fn with_style(spec: ComponentSpec, style: StyleOverride) -> ComponentSpec {
    spec.hover(style.clone())
        .focused(style)
        .style_transition(TransitionConfig {
            duration: SECS,
            delay: 0.0,
            easing: Easing::Linear,
        })
}
