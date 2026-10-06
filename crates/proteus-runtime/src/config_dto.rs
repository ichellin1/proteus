//! [`ProteusConfigDto`]: a partial [`ProteusConfig`] that can be deserialized,
//! for configuration that comes from outside the program, such as
//! TypeScript's `mount`.
//!
//! Every field is optional, and anything omitted keeps
//! [`ProteusConfig::web`]'s value. Only settings that have an effect are
//! included: a setting that silently did nothing would be worse than none.

use crate::{wgpu, ProteusConfig};
use serde::Deserialize;

/// Overrides for [`ProteusConfig`], deserialized with camelCase field names.
/// Each section is optional; see [`ProteusConfigDto::apply`].
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProteusConfigDto {
    /// Overrides for [`ProteusConfig::render`].
    #[serde(default)]
    pub render: Option<RenderDto>,
    /// Overrides for [`ProteusConfig::memory`].
    #[serde(default)]
    pub memory: Option<MemoryDto>,
    /// Overrides for [`ProteusConfig::frame`].
    #[serde(default)]
    pub frame: Option<FrameDto>,
    /// Overrides for [`ProteusConfig::resources`].
    #[serde(default)]
    pub resources: Option<ResourcesDto>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderDto {
    /// The color behind everything, and what shows through transparency:
    /// RGBA, each `0`–`1`.
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
    /// Scale images down so their longer side is at most this many pixels;
    /// `null` keeps full size.
    #[serde(default, deserialize_with = "double_option")]
    pub image_max_side: Option<Option<u32>>,
    #[serde(default)]
    pub lazy_load: Option<bool>,
}

/// Tells an absent field from an explicit `null`, so that
/// `imageMaxSide: null` means "full size" rather than "keep the default".
fn double_option<'de, D>(d: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u32>::deserialize(d).map(Some)
}

/// An unknown string is rejected rather than replaced with a default: a wrong
/// present mode changes frame pacing in a way that is hard to notice.
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
    /// Applies these overrides to [`ProteusConfig::web`] and returns the
    /// result.
    ///
    /// # Errors
    ///
    /// Returns a message naming the field if a value isn't recognized, such as
    /// an unknown present mode.
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
        // `deny_unknown_fields`: a misspelled field must be an error, not
        // silently ignored.
        assert!(serde_json::from_str::<ProteusConfigDto>(r#"{"renderr":{}}"#).is_err());
        assert!(serde_json::from_str::<ProteusConfigDto>(
            r#"{"render":{"clearColour":[0,0,0,1]}}"#
        )
        .is_err());
    }
}
