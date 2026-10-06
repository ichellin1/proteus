//! The JavaScript shapes of `proteus-sdk`'s types, and conversions between
//! them, for values that cross into wasm through `serde-wasm-bindgen`.
//!
//! They are separate types because `proteus-sdk`'s types aren't serializable,
//! and `TransitionConfig::easing`, a function pointer, can't be. Easing crosses
//! as the name of a built-in curve. Field names must match `ts/src/types.ts`.
//!
//! An entity crosses as its `Entity::to_bits()` value, as an `f64`. That is
//! exact up to 2^53, which only fails after about two million reuses of one
//! entity index: not a concern for a UI app.

use serde::{Deserialize, Serialize};

use proteus_sdk::{
    ComponentData, ComponentSpec, DropReason, InteractionStateKind, QuadState, StyleOverride,
    TransitionConfig, TransitionData, TransitionDropped,
};
use proteus_ui::{Border, DropShadow, Glow, Image, Text};

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
// Text / Image / Border / Glow / DropShadow
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDto {
    pub content: String,
    pub size_px: f32,
    #[serde(default)]
    pub color: Option<ColorDto>,
    #[serde(default)]
    pub letter_spacing_px: f32,
}

impl From<&TextDto> for Text {
    fn from(d: &TextDto) -> Self {
        let mut text =
            Text::new(d.content.clone(), d.size_px).with_letter_spacing(d.letter_spacing_px);
        if let Some(c) = &d.color {
            text = text.with_color(glam::Vec4::new(c.r, c.g, c.b, c.a));
        }
        text
    }
}

/// An image as encoded PNG or JPEG bytes. The format is detected from the
/// data, and decoding happens when the host bakes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDto {
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub max_side: Option<u32>,
}

impl From<&ImageDto> for Image {
    fn from(d: &ImageDto) -> Self {
        let mut image = Image::new(d.bytes.clone());
        if let Some(max_side) = d.max_side {
            image = image.with_max_side(max_side);
        }
        image
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderDto {
    pub width: f32,
    pub color: ColorDto,
    pub offset: f32,
}

impl From<&BorderDto> for Border {
    fn from(d: &BorderDto) -> Self {
        Self {
            width: d.width,
            color: glam::Vec4::new(d.color.r, d.color.g, d.color.b, d.color.a),
            offset: d.offset,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlowDto {
    pub radius: f32,
    pub color: ColorDto,
    pub intensity: f32,
}

impl From<&GlowDto> for Glow {
    fn from(d: &GlowDto) -> Self {
        Self {
            radius: d.radius,
            color: glam::Vec4::new(d.color.r, d.color.g, d.color.b, d.color.a),
            intensity: d.intensity,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropShadowDto {
    pub offset: Vec2Dto,
    pub color: ColorDto,
    pub softness: f32,
    pub spread: f32,
}

impl From<&DropShadowDto> for DropShadow {
    fn from(d: &DropShadowDto) -> Self {
        Self {
            offset: glam::Vec2::new(d.offset.x, d.offset.y),
            color: glam::Vec4::new(d.color.r, d.color.g, d.color.b, d.color.a),
            softness: d.softness,
            spread: d.spread,
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
    /// Children, as `Entity::to_bits()` values.
    #[serde(default)]
    pub children: Vec<f64>,
    #[serde(default)]
    pub bake: bool,
    #[serde(default)]
    pub text: Option<TextDto>,
    #[serde(default)]
    pub image: Option<ImageDto>,
    #[serde(default)]
    pub border: Option<BorderDto>,
    #[serde(default)]
    pub glow: Option<GlowDto>,
    #[serde(default)]
    pub drop_shadow: Option<DropShadowDto>,
    #[serde(default)]
    pub non_interactive: bool,
    /// Defaults to `true`: an omitted `visible` means shown.
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub start_disabled: bool,
    #[serde(default)]
    pub transitioning: Option<TransitioningConfigDto>,
}

/// `{ maxSide?, eternal? }`: how to add a texture to the atlas.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureRequestDto {
    #[serde(default)]
    pub max_side: Option<u32>,
    #[serde(default)]
    pub eternal: bool,
}

impl From<&TextureRequestDto> for proteus_sdk::TextureRequest {
    fn from(d: &TextureRequestDto) -> Self {
        Self {
            max_side: d.max_side,
            eternal: d.eternal,
        }
    }
}

/// Whether a component accepts input while transitioning. Both default to
/// `false`. `allowNavigation` is not read yet; it is reserved for keyboard
/// navigation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitioningConfigDto {
    #[serde(default)]
    pub allow_input: bool,
    #[serde(default)]
    pub allow_navigation: bool,
}

impl From<&TransitioningConfigDto> for proteus_ui::TransitioningConfig {
    fn from(d: &TransitioningConfigDto) -> Self {
        Self {
            allow_input: d.allow_input,
            allow_navigation: d.allow_navigation,
        }
    }
}

fn default_visible() -> bool {
    true
}

impl ComponentSpecDto {
    /// Builds a `ComponentSpec` from everything except `children`, which are
    /// returned separately for the caller to attach.
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
        if let Some(text) = &self.text {
            spec = spec.text(text.into());
        }
        if let Some(image) = &self.image {
            spec = spec.image(image.into());
        }
        if let Some(border) = &self.border {
            spec = spec.border(border.into());
        }
        if let Some(glow) = &self.glow {
            spec = spec.glow(glow.into());
        }
        if let Some(drop_shadow) = &self.drop_shadow {
            spec = spec.drop_shadow(drop_shadow.into());
        }
        if self.non_interactive {
            spec = spec.non_interactive();
        }
        spec = spec.visible(self.visible);
        if let Some(opacity) = self.opacity {
            spec = spec.opacity(opacity);
        }
        if self.start_disabled {
            spec = spec.start_disabled();
        }
        if let Some(transitioning) = &self.transitioning {
            spec = spec.transitioning(transitioning.into());
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
// SplitStrategy / MergeLayout
// ---------------------------------------------------------------------------

/// `{ kind, cols?, rows? }`, where `kind` is `"perTarget"`, `"slice"` or
/// `"gridSlice"`, and `cols` and `rows` apply to `"gridSlice"` only. An
/// unknown `kind` falls back to `"slice"` with a warning, not to the
/// experimental `"perTarget"`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitStrategyDto {
    pub kind: String,
    #[serde(default)]
    pub cols: usize,
    #[serde(default)]
    pub rows: usize,
}

impl From<&SplitStrategyDto> for proteus_ui::SplitStrategy {
    fn from(d: &SplitStrategyDto) -> Self {
        match d.kind.as_str() {
            "slice" => proteus_ui::SplitStrategy::Slice,
            "gridSlice" => proteus_ui::SplitStrategy::GridSlice {
                cols: d.cols.max(1),
                rows: d.rows.max(1),
            },
            "perTarget" => proteus_ui::SplitStrategy::PerTarget,
            other => {
                log::warn!("unknown splitTo strategy {other:?} — falling back to \"slice\"");
                proteus_ui::SplitStrategy::Slice
            }
        }
    }
}

/// `{ kind, cols?, rows? }`, where `kind` is `"horizontal"` or `"grid"`, and
/// `cols` and `rows` apply to `"grid"` only. An unknown `kind` falls back to
/// `"horizontal"`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeLayoutDto {
    pub kind: String,
    #[serde(default)]
    pub cols: usize,
    #[serde(default)]
    pub rows: usize,
}

impl From<&MergeLayoutDto> for proteus_ui::MergeLayout {
    fn from(d: &MergeLayoutDto) -> Self {
        match d.kind.as_str() {
            "grid" => proteus_ui::MergeLayout::Grid {
                cols: d.cols.max(1),
                rows: d.rows.max(1),
            },
            _ => proteus_ui::MergeLayout::Horizontal,
        }
    }
}

/// One target of `splitToWithStates`: its handle ID and the geometry it should
/// end at.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetStateDto {
    pub id: f64,
    pub state: QuadStateDto,
}

// ---------------------------------------------------------------------------
// ComponentData / TransitionData (output only)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentDataDto {
    pub geometry: QuadStateDto,
    pub state: String,
    pub disabled: bool,
    pub visible: bool,
    pub opacity: f32,
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
            disabled: data.disabled,
            visible: data.visible,
            opacity: data.opacity,
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
