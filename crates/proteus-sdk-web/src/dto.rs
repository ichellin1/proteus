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
    ComponentData, ComponentSpec, DropReason, Easing, InteractionStateKind, QuadState,
    StyleOverride, TransitionConfig, TransitionData, TransitionDropped,
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
    pub transition_interaction: Option<TransitionInteractionConfigDto>,
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
pub struct TransitionInteractionConfigDto {
    #[serde(default)]
    pub allow_pointer: bool,
    #[serde(default)]
    pub allow_navigation: bool,
}

impl From<&TransitionInteractionConfigDto> for proteus_ui::TransitionInteractionConfig {
    fn from(d: &TransitionInteractionConfigDto) -> Self {
        Self {
            allow_pointer: d.allow_pointer,
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
        if let Some(interaction) = &self.transition_interaction {
            spec = spec.transition_interaction(interaction.into());
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
    /// Defaults to `easeInOutQuad`, as in Rust.
    #[serde(default)]
    pub easing: EasingDto,
}

impl From<&TransitionConfigDto> for TransitionConfig {
    fn from(d: &TransitionConfigDto) -> Self {
        Self {
            duration: d.duration,
            delay: d.delay,
            easing: d.easing.0,
        }
    }
}

/// The built-in easing names TypeScript accepts, in TypeScript's spelling.
const EASING_NAMES: &[&str] = &[
    "linear",
    "easeInQuad",
    "easeOutQuad",
    "easeInOutQuad",
    "easeOutCubic",
];

/// An easing from JavaScript: a built-in name such as `"easeOutCubic"`, or
/// `{ cubicBezier: [x1, y1, x2, y2] }`. Anything else is an error that names
/// the bad value, so a mistyped name fails the call instead of quietly
/// becoming a different curve.
#[derive(Debug, Clone, Copy, Default)]
pub struct EasingDto(pub Easing);

impl<'de> Deserialize<'de> for EasingDto {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EasingVisitor;

        impl<'de> serde::de::Visitor<'de> for EasingVisitor {
            type Value = EasingDto;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an easing name or { cubicBezier: [x1, y1, x2, y2] }")
            }

            fn visit_str<E: serde::de::Error>(self, name: &str) -> Result<EasingDto, E> {
                let easing = match name {
                    "linear" => Easing::Linear,
                    "easeInQuad" => Easing::EaseInQuad,
                    "easeOutQuad" => Easing::EaseOutQuad,
                    "easeInOutQuad" => Easing::EaseInOutQuad,
                    "easeOutCubic" => Easing::EaseOutCubic,
                    _ => return Err(E::unknown_variant(name, EASING_NAMES)),
                };
                Ok(EasingDto(easing))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<EasingDto, A::Error> {
                let mut points: Option<[f32; 4]> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "cubicBezier" {
                        points = Some(map.next_value()?);
                    } else {
                        return Err(serde::de::Error::unknown_field(&key, &["cubicBezier"]));
                    }
                }
                let [x1, y1, x2, y2] =
                    points.ok_or_else(|| serde::de::Error::missing_field("cubicBezier"))?;
                Ok(EasingDto(Easing::CubicBezier { x1, y1, x2, y2 }))
            }
        }

        deserializer.deserialize_any(EasingVisitor)
    }
}

// ---------------------------------------------------------------------------
// ImageCrop
// ---------------------------------------------------------------------------

/// `{ kind: "none" | "centeredSquare" }`, `{ kind: "aspect", ratio, anchor? }`
/// (the anchor defaults to the center) or `{ kind: "rect", x, y, width, height }`.
/// An unknown `kind`, or a missing field, is an error that names it.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImageCropDto {
    None,
    CenteredSquare,
    Aspect {
        ratio: f32,
        #[serde(default)]
        anchor: Option<Vec2Dto>,
    },
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
}

impl From<&ImageCropDto> for proteus_sdk::ImageCrop {
    fn from(d: &ImageCropDto) -> Self {
        match d {
            ImageCropDto::None => proteus_sdk::ImageCrop::None,
            ImageCropDto::CenteredSquare => proteus_sdk::ImageCrop::CenteredSquare,
            ImageCropDto::Aspect { ratio, anchor } => proteus_sdk::ImageCrop::Aspect {
                ratio: *ratio,
                anchor: anchor
                    .map(|a| glam::Vec2::new(a.x, a.y))
                    .unwrap_or(glam::Vec2::splat(0.5)),
            },
            ImageCropDto::Rect {
                x,
                y,
                width,
                height,
            } => proteus_sdk::ImageCrop::Rect {
                x: *x,
                y: *y,
                width: *width,
                height: *height,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// SplitStrategy / MergeLayout
// ---------------------------------------------------------------------------

/// `{ kind: "perTarget" | "row" | "column" }` or
/// `{ kind: "grid", cols, rows }`. An unknown `kind`, or a grid without `cols`
/// and `rows`, is an error that names the problem.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SplitStrategyDto {
    PerTarget,
    Row,
    Column,
    Grid { cols: usize, rows: usize },
}

impl From<&SplitStrategyDto> for proteus_ui::SplitStrategy {
    fn from(d: &SplitStrategyDto) -> Self {
        match *d {
            SplitStrategyDto::PerTarget => proteus_ui::SplitStrategy::PerTarget,
            SplitStrategyDto::Row => proteus_ui::SplitStrategy::Row,
            SplitStrategyDto::Column => proteus_ui::SplitStrategy::Column,
            SplitStrategyDto::Grid { cols, rows } => proteus_ui::SplitStrategy::Grid { cols, rows },
        }
    }
}

/// `{ kind: "row" | "column" }` or `{ kind: "grid", cols, rows }`. An unknown
/// `kind`, or a grid without `cols` and `rows`, is an error that names the
/// problem.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MergeLayoutDto {
    Row,
    Column,
    Grid { cols: usize, rows: usize },
}

impl From<&MergeLayoutDto> for proteus_ui::MergeLayout {
    fn from(d: &MergeLayoutDto) -> Self {
        match *d {
            MergeLayoutDto::Row => proteus_ui::MergeLayout::Row,
            MergeLayoutDto::Column => proteus_ui::MergeLayout::Column,
            MergeLayoutDto::Grid { cols, rows } => proteus_ui::MergeLayout::Grid { cols, rows },
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
        // A state added to `proteus-ui` must be added here and to TypeScript's
        // `InteractionState`; until then it reads as the default style.
        _ => "default",
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
// TransitionDropped (TransitionChannel::on_dropped payload)
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
        DropReason::ChannelNotFound => "channelNotFound",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(json: &str) -> Result<TransitionConfig, serde_json::Error> {
        serde_json::from_str::<TransitionConfigDto>(json).map(|d| (&d).into())
    }

    #[test]
    fn the_default_easing_matches_rust() {
        let c = config(r#"{"duration": 0.3}"#).unwrap();
        assert!(matches!(c.easing, Easing::EaseInOutQuad));
    }

    #[test]
    fn built_in_names_map_to_their_easing() {
        let c = config(r#"{"duration": 0.3, "easing": "easeOutCubic"}"#).unwrap();
        assert!(matches!(c.easing, Easing::EaseOutCubic));
    }

    #[test]
    fn a_cubic_bezier_is_read_in_order() {
        let c = config(r#"{"duration": 0.3, "easing": {"cubicBezier": [0.1, 0.2, 0.3, 0.4]}}"#)
            .unwrap();
        match c.easing {
            Easing::CubicBezier { x1, y1, x2, y2 } => {
                assert_eq!([x1, y1, x2, y2], [0.1, 0.2, 0.3, 0.4])
            }
            other => panic!("expected a cubic Bézier, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_name_is_an_error_naming_it() {
        let err = config(r#"{"duration": 0.3, "easing": "easeOutQuart"}"#).unwrap_err();
        assert!(err.to_string().contains("easeOutQuart"), "{err}");
    }

    #[test]
    fn layouts_parse_by_kind() {
        let row: SplitStrategyDto = serde_json::from_str(r#"{"kind": "row"}"#).unwrap();
        assert!(matches!((&row).into(), proteus_ui::SplitStrategy::Row));
        let grid: MergeLayoutDto =
            serde_json::from_str(r#"{"kind": "grid", "cols": 3, "rows": 2}"#).unwrap();
        assert!(matches!(
            (&grid).into(),
            proteus_ui::MergeLayout::Grid { cols: 3, rows: 2 }
        ));
    }

    #[test]
    fn an_unknown_layout_kind_or_a_grid_without_dimensions_is_an_error() {
        let err = serde_json::from_str::<SplitStrategyDto>(r#"{"kind": "slice"}"#).unwrap_err();
        assert!(err.to_string().contains("slice"), "{err}");
        assert!(serde_json::from_str::<MergeLayoutDto>(r#"{"kind": "horizontal"}"#).is_err());
        assert!(serde_json::from_str::<MergeLayoutDto>(r#"{"kind": "grid"}"#).is_err());
    }

    #[test]
    fn image_crops_parse_by_kind_and_aspect_defaults_to_centered() {
        let crop: ImageCropDto =
            serde_json::from_str(r#"{"kind": "aspect", "ratio": 2.0}"#).unwrap();
        match (&crop).into() {
            proteus_sdk::ImageCrop::Aspect { ratio, anchor } => {
                assert_eq!(ratio, 2.0);
                assert_eq!(anchor, glam::Vec2::splat(0.5));
            }
            other => panic!("expected an aspect crop, got {other:?}"),
        }
        assert!(serde_json::from_str::<ImageCropDto>(r#"{"kind": "square"}"#).is_err());
        assert!(serde_json::from_str::<ImageCropDto>(r#"{"kind": "rect", "x": 0.1}"#).is_err());
    }

    #[test]
    fn a_malformed_bezier_is_an_error() {
        assert!(config(r#"{"duration": 0.3, "easing": {"cubicBezier": [0.1, 0.2]}}"#).is_err());
        assert!(
            config(r#"{"duration": 0.3, "easing": {"bezier": [0.1, 0.2, 0.3, 0.4]}}"#).is_err()
        );
    }
}
