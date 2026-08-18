//! JS-facing DTOs and their conversions to/from `proteus-sdk`/`proteus-ui`
//! types, crossing the wasm boundary via `serde-wasm-bindgen`.
//!
//! Defined here rather than adding `Serialize`/`Deserialize` to `QuadState`/
//! `StyleOverride`/`ComponentSpec`/`ComponentData` directly: those types
//! don't derive it today, and `TransitionConfig::easing` (a raw
//! `fn(f32) -> f32`) can't derive it at all. Keeping the DTOs local to this
//! crate means M12.1–3's shipped code needs no changes. `easing` becomes a
//! string naming one of `proteus_ui`'s five existing, already-tested easing
//! functions (`linear`/`easeInQuad`/`easeOutQuad`/`easeInOutQuad`/
//! `easeOutCubic`) — wiring through what already works, not new
//! interpolation logic. A genuinely different thing — letting a caller
//! register an arbitrary *custom* easing function — is M13's job
//! ("pluggable interpolation interface"), not this.
//!
//! An entity handle crosses as `Entity::to_bits(): u64` cast to `f64` (both
//! `to_bits`/`from_bits` are `proteus-ui`'s own public round-trip pair, not
//! feature-gated). `f64` can represent every integer up to 2^53 exactly;
//! `to_bits` packs a small generation counter into the high bits, so this
//! only loses precision past roughly two million generations reused on a
//! single entity index — not realistic for a UI app. Documented rather than
//! solved with a custom index/generation struct.

use serde::{Deserialize, Serialize};

use proteus_sdk::{
    ComponentData, ComponentSpec, DropReason, InteractionStateKind, QuadState, StyleOverride,
    TransitionConfig, TransitionData, TransitionDropped,
};

// ---------------------------------------------------------------------------
// Shared value types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vec2Dto {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vec3Dto {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorDto {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

// ---------------------------------------------------------------------------
// QuadState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuadStateDto {
    pub position: Vec3Dto,
    pub size: Vec2Dto,
    pub rotation: f32,
    pub scale: f32,
    pub anchor: Vec2Dto,
    pub color: ColorDto,
    pub corner_radius: f32,
}

impl From<&QuadState> for QuadStateDto {
    fn from(q: &QuadState) -> Self {
        Self {
            position: Vec3Dto {
                x: q.position.x,
                y: q.position.y,
                z: q.position.z,
            },
            size: Vec2Dto {
                x: q.size.x,
                y: q.size.y,
            },
            rotation: q.rotation,
            scale: q.scale,
            anchor: Vec2Dto {
                x: q.anchor.x,
                y: q.anchor.y,
            },
            color: ColorDto {
                r: q.color.x,
                g: q.color.y,
                b: q.color.z,
                a: q.color.w,
            },
            corner_radius: q.corner_radius,
        }
    }
}

impl From<&QuadStateDto> for QuadState {
    fn from(d: &QuadStateDto) -> Self {
        Self {
            position: glam::Vec3::new(d.position.x, d.position.y, d.position.z),
            size: glam::Vec2::new(d.size.x, d.size.y),
            rotation: d.rotation,
            scale: d.scale,
            anchor: glam::Vec2::new(d.anchor.x, d.anchor.y),
            color: glam::Vec4::new(d.color.r, d.color.g, d.color.b, d.color.a),
            corner_radius: d.corner_radius,
        }
    }
}

// ---------------------------------------------------------------------------
// StyleOverride
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleOverrideDto {
    #[serde(default)]
    pub position: Option<Vec3Dto>,
    #[serde(default)]
    pub size: Option<Vec2Dto>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub scale: Option<f32>,
    #[serde(default)]
    pub anchor: Option<Vec2Dto>,
    #[serde(default)]
    pub color: Option<ColorDto>,
    #[serde(default)]
    pub corner_radius: Option<f32>,
}

impl From<&StyleOverrideDto> for StyleOverride {
    fn from(d: &StyleOverrideDto) -> Self {
        Self {
            position: d.position.map(|p| glam::Vec3::new(p.x, p.y, p.z)),
            size: d.size.map(|s| glam::Vec2::new(s.x, s.y)),
            rotation: d.rotation,
            scale: d.scale,
            anchor: d.anchor.map(|a| glam::Vec2::new(a.x, a.y)),
            color: d.color.map(|c| glam::Vec4::new(c.r, c.g, c.b, c.a)),
            corner_radius: d.corner_radius,
        }
    }
}

// ---------------------------------------------------------------------------
// ComponentSpec
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentSpecDto {
    pub geometry: QuadStateDto,
    #[serde(default)]
    pub hover: Option<StyleOverrideDto>,
    #[serde(default)]
    pub pressed: Option<StyleOverrideDto>,
    #[serde(default)]
    pub focused: Option<StyleOverrideDto>,
    #[serde(default)]
    pub disabled: Option<StyleOverrideDto>,
    /// Child entity handles, as `Entity::to_bits()` values — see this
    /// module's top doc.
    #[serde(default)]
    pub children: Vec<f64>,
    #[serde(default)]
    pub bake: bool,
}

impl ComponentSpecDto {
    /// Consumes `self`, building a real `ComponentSpec`. `children` still
    /// needs the caller to resolve each bits-value into a `proteus_sdk::Handle`
    /// (this DTO alone can't do that — it has no access to the live world).
    pub fn into_spec_without_children(self) -> (ComponentSpec, Vec<f64>) {
        let mut spec = ComponentSpec::new((&self.geometry).into());
        if let Some(hover) = &self.hover {
            spec = spec.hover(hover.into());
        }
        if let Some(pressed) = &self.pressed {
            spec = spec.pressed(pressed.into());
        }
        if let Some(focused) = &self.focused {
            spec = spec.focused(focused.into());
        }
        if let Some(disabled) = &self.disabled {
            spec = spec.disabled(disabled.into());
        }
        if self.bake {
            spec = spec.bake();
        }
        (spec, self.children)
    }
}

// ---------------------------------------------------------------------------
// TransitionConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionConfigDto {
    pub duration: f32,
    #[serde(default)]
    pub delay: f32,
    #[serde(default = "default_easing")]
    pub easing: String,
}

fn default_easing() -> String {
    "linear".to_string()
}

impl From<&TransitionConfigDto> for TransitionConfig {
    fn from(d: &TransitionConfigDto) -> Self {
        let easing = match d.easing.as_str() {
            "easeInQuad" => proteus_sdk::ease_in_quad,
            "easeOutQuad" => proteus_sdk::ease_out_quad,
            "easeInOutQuad" => proteus_sdk::ease_in_out_quad,
            "easeOutCubic" => proteus_sdk::ease_out_cubic,
            _ => proteus_sdk::linear,
        };
        Self {
            duration: d.duration,
            delay: d.delay,
            easing,
        }
    }
}

// ---------------------------------------------------------------------------
// ComponentData / TransitionData (output only)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDataDto {
    pub geometry: QuadStateDto,
    pub state: String,
    pub visible: bool,
    /// Child entity handles, as `Entity::to_bits()` values.
    pub children: Vec<f64>,
    pub transition: Option<TransitionDataDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionDataDto {
    pub base: QuadStateDto,
    pub target: QuadStateDto,
    pub current: QuadStateDto,
    pub progress: f32,
}

fn interaction_state_str(state: InteractionStateKind) -> &'static str {
    match state {
        InteractionStateKind::Default => "default",
        InteractionStateKind::Hover => "hover",
        InteractionStateKind::Pressed => "pressed",
        InteractionStateKind::Focused => "focused",
        InteractionStateKind::Disabled => "disabled",
    }
}

impl ComponentDataDto {
    pub fn from_data(data: &ComponentData, children_bits: Vec<f64>) -> Self {
        Self {
            geometry: (&data.geometry).into(),
            state: interaction_state_str(data.state).to_string(),
            visible: data.visible,
            children: children_bits,
            transition: data.transition.as_ref().map(TransitionDataDto::from),
        }
    }
}

impl From<&TransitionData> for TransitionDataDto {
    fn from(t: &TransitionData) -> Self {
        Self {
            base: (&t.base).into(),
            target: (&t.target).into(),
            current: (&t.current).into(),
            progress: t.progress,
        }
    }
}

// ---------------------------------------------------------------------------
// TransitionDropped (SignalHandle::on_dropped payload)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionDroppedDto {
    pub to: f64,
    pub from: f64,
    pub reason: String,
}

fn drop_reason_str(reason: DropReason) -> &'static str {
    match reason {
        DropReason::SignalNotFound => "signalNotFound",
        DropReason::EntityNotFound => "entityNotFound",
        DropReason::AlreadyTransitioning => "alreadyTransitioning",
        DropReason::EntityNotVisible => "entityNotVisible",
    }
}

impl From<&TransitionDropped> for TransitionDroppedDto {
    fn from(d: &TransitionDropped) -> Self {
        Self {
            to: bevy_ecs::prelude::Entity::to_bits(d.to) as f64,
            from: bevy_ecs::prelude::Entity::to_bits(d.from) as f64,
            reason: drop_reason_str(d.reason).to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// TextureHandle::state()
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureStateDto {
    pub kind: String,
    pub width: u32,
    pub height: u32,
}

impl TextureStateDto {
    pub fn from_state(kind: proteus_render::TextureKind, width: u32, height: u32) -> Self {
        let kind = match kind {
            proteus_render::TextureKind::Static => "static",
            proteus_render::TextureKind::Video => "video",
            proteus_render::TextureKind::Animated => "animated",
        };
        Self {
            kind: kind.to_string(),
            width,
            height,
        }
    }
}
