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
// Text / Image / Border / Glow / DropShadow (M13.8 — TS/JS parity audit;
// previously only reachable from Rust)
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

/// `bytes` is raw PNG/JPEG file bytes (format sniffed from the data, not a
/// file extension — see `proteus_ui::Image`'s own doc), e.g. straight from a
/// `fetch()` response's `Uint8Array`, not decoded pixels — decoding happens
/// during baking, same as the Rust-only path.
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
    /// Child entity handles, as `Entity::to_bits()` values — see this
    /// module's top doc.
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
    /// Defaults to `true` — an omitted `visible` must mean "shown", not
    /// `bool::default()`.
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub start_disabled: bool,
    #[serde(default)]
    pub transitioning: Option<TransitioningConfigDto>,
}

/// Per-entity opt-in to input while mid-transition. Both flags default to
/// `false`; `allowNavigation` is accepted but inert until navigation exists.
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
// SplitStrategy / MergeLayout (M13.8 — group transitions, previously
// Rust-only)
// ---------------------------------------------------------------------------

/// Same flat `{kind, ...}` shape convention `easing` already uses above
/// (a string tag instead of a nested JSON tagged-union) — `kind` is one of
/// `"perTarget"` / `"slice"` / `"gridSlice"`; `cols`/`rows` only matter for
/// `"gridSlice"`. An unrecognized `kind` falls back to `Slice` and logs,
/// keeping `TransitionConfigDto::easing`'s "unknown string → sane default"
/// leniency rather than erroring. It used to fall back to `PerTarget`, which
/// meant a typo silently selected the experimental strategy.
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
            _ => proteus_ui::SplitStrategy::PerTarget,
        }
    }
}

/// `kind` is `"horizontal"` / `"grid"`; `cols`/`rows` only matter for
/// `"grid"`. Unrecognized `kind` falls back to `Horizontal` — same
/// leniency convention as [`SplitStrategyDto`].
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

/// One entry of `splitToWithStates`'s target list — `id` is a `Handle.id()`
/// value (see this module's top doc), `state` the explicit rest geometry to
/// use instead of resolving it from the target's own declared/live
/// `QuadState` (mirrors `proteus-sdk`'s `Handle::split_to_with_states`).
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
