//! [`ProteusConfigDto`] — a partial, deserializable [`ProteusConfig`].
//!
//! Built for `proteus-host-web`'s `mount`, which takes config from a JS
//! caller, but it lives here rather than in that host: this is a property of
//! the config, not of the web. A native host reading settings from a file
//! wants the same thing — and here the tests actually run, since
//! `proteus-host-web` only compiles for wasm32, where nothing runs
//! `cargo test`.
//!
//! **Overrides, not a mirror.** Every field is optional and anything omitted
//! keeps [`ProteusConfig::web`]'s value, so a TS app states only what it
//! actually wants to change. That also keeps this additive: a new knob is a
//! new optional field, and M13.5's "the config shape only grows" rule holds
//! on this side too.
//!
//! **Only knobs that do something.** `ProteusConfig` carries fields that are
//! declared but not yet consumed — `memory.video.*`, `render.msaa_samples`,
//! all of `input.*`, `transitions.custom_easings` (audit A-07), most of
//! `debug.*`. Those are deliberately absent here: an inert field is a
//! documented placeholder in Rust, but in a TypeScript API it is a knob that
//! silently does nothing, which is worse. They can be added when they work.

use crate::{wgpu, ProteusConfig};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProteusConfigDto {
    #[serde(default)]
    pub render: Option<RenderDto>,
    #[serde(default)]
    pub memory: Option<MemoryDto>,
    #[serde(default)]
    pub frame: Option<FrameDto>,
    #[serde(default)]
    pub resources: Option<ResourcesDto>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderDto {
    /// Linear RGBA, 0–1. The colour behind everything, shown before the
    /// first frame paints and through any transparency.
    #[serde(default)]
    pub clear_color: Option<[f64; 4]>,
    #[serde(default)]
    pub present_mode: Option<String>,
    #[serde(default)]
    pub power_preference: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryDto {
    #[serde(default)]
    pub main_atlas: Option<AtlasDto>,
    #[serde(default)]
    pub transition_atlas_size: Option<u32>,
    #[serde(default)]
    pub max_instances: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AtlasDto {
    #[serde(default)]
    pub page_size: Option<u32>,
    #[serde(default)]
    pub page_count: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameDto {
    #[serde(default)]
    pub dt_clamp_secs: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourcesDto {
    /// `null` packs at native resolution; a number caps the longest side.
    #[serde(default, deserialize_with = "double_option")]
    pub image_max_side: Option<Option<u32>>,
    #[serde(default)]
    pub lazy_load: Option<bool>,
}

/// Distinguishes "absent" from an explicit `null`, so `imageMaxSide: null`
/// can mean "pack at native resolution" rather than "leave the default".
fn double_option<'de, D>(d: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u32>::deserialize(d).map(Some)
}

/// Unknown strings are rejected rather than silently defaulted: unlike a
/// split strategy, getting `presentMode` wrong changes frame pacing in a way
/// that is hard to notice and harder to attribute.
fn present_mode(s: &str) -> Result<wgpu::PresentMode, String> {
    Ok(match s {
        "autoVsync" => wgpu::PresentMode::AutoVsync,
        "autoNoVsync" => wgpu::PresentMode::AutoNoVsync,
        "fifo" => wgpu::PresentMode::Fifo,
        "fifoRelaxed" => wgpu::PresentMode::FifoRelaxed,
        "immediate" => wgpu::PresentMode::Immediate,
        "mailbox" => wgpu::PresentMode::Mailbox,
        other => return Err(format!("unknown presentMode {other:?}")),
    })
}

fn power_preference(s: &str) -> Result<wgpu::PowerPreference, String> {
    Ok(match s {
        "none" => wgpu::PowerPreference::None,
        "lowPower" => wgpu::PowerPreference::LowPower,
        "highPerformance" => wgpu::PowerPreference::HighPerformance,
        other => return Err(format!("unknown powerPreference {other:?}")),
    })
}

impl ProteusConfigDto {
    /// Apply these overrides on top of [`ProteusConfig::web`].
    pub fn apply(&self) -> Result<ProteusConfig, String> {
        let mut config = ProteusConfig::web();

        if let Some(r) = &self.render {
            if let Some(c) = r.clear_color {
                config.render.clear_color = c;
            }
            if let Some(m) = &r.present_mode {
                config.render.present_mode = present_mode(m)?;
            }
            if let Some(p) = &r.power_preference {
                config.render.power_preference = power_preference(p)?;
            }
        }
        if let Some(m) = &self.memory {
            if let Some(a) = &m.main_atlas {
                if let Some(v) = a.page_size {
                    config.memory.main_atlas.page_size = v;
                }
                if let Some(v) = a.page_count {
                    config.memory.main_atlas.page_count = v;
                }
            }
            if let Some(v) = m.transition_atlas_size {
                config.memory.transition_atlas_size = v;
            }
            if let Some(v) = m.max_instances {
                config.memory.max_instances = v;
            }
        }
        if let Some(f) = &self.frame {
            if let Some(v) = f.dt_clamp_secs {
                config.frame.dt_clamp_secs = v;
            }
        }
        if let Some(r) = &self.resources {
            if let Some(v) = r.image_max_side {
                config.resources.image_max_side = v;
            }
            if let Some(v) = r.lazy_load {
                config.resources.lazy_load = v;
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(json: &str) -> ProteusConfigDto {
        serde_json::from_str(json).expect("valid DTO")
    }

    #[test]
    fn an_empty_override_is_the_web_preset() {
        let applied = dto("{}").apply().unwrap();
        let base = ProteusConfig::web();
        assert_eq!(applied.render.clear_color, base.render.clear_color);
        assert_eq!(
            applied.memory.main_atlas.page_size,
            base.memory.main_atlas.page_size
        );
        assert_eq!(applied.resources.lazy_load, base.resources.lazy_load);
    }

    #[test]
    fn only_named_fields_change() {
        let applied = dto(r#"{"render":{"clearColor":[1.0,0.0,0.0,1.0]}}"#)
            .apply()
            .unwrap();
        let base = ProteusConfig::web();
        assert_eq!(applied.render.clear_color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(applied.render.present_mode, base.render.present_mode);
        assert_eq!(applied.memory.max_instances, base.memory.max_instances);
    }

    #[test]
    fn nested_partials_leave_their_siblings_alone() {
        let applied = dto(r#"{"memory":{"mainAtlas":{"pageCount":2}}}"#)
            .apply()
            .unwrap();
        let base = ProteusConfig::web();
        assert_eq!(applied.memory.main_atlas.page_count, 2);
        assert_eq!(
            applied.memory.main_atlas.page_size, base.memory.main_atlas.page_size,
            "page_size must survive an override that only names page_count"
        );
    }

    #[test]
    fn an_explicit_null_image_max_side_means_native_resolution() {
        let applied = dto(r#"{"resources":{"imageMaxSide":null}}"#)
            .apply()
            .unwrap();
        assert_eq!(
            applied.resources.image_max_side, None,
            "null is an instruction, not an omission"
        );
    }

    #[test]
    fn an_unknown_present_mode_is_an_error_not_a_default() {
        let err = dto(r#"{"render":{"presentMode":"turbo"}}"#)
            .apply()
            .unwrap_err();
        assert!(
            err.contains("turbo"),
            "the error names what was wrong: {err}"
        );
    }

    #[test]
    fn a_misspelled_field_is_rejected() {
        // `deny_unknown_fields`: a typo silently doing nothing is the exact
        // failure mode this API exists to avoid.
        assert!(serde_json::from_str::<ProteusConfigDto>(r#"{"renderr":{}}"#).is_err());
        assert!(serde_json::from_str::<ProteusConfigDto>(
            r#"{"render":{"clearColour":[0,0,0,1]}}"#
        )
        .is_err());
    }
}
