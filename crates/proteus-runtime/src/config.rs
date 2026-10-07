//! [`ProteusConfig`]: the engine's settings, passed to a host when it starts.
//!
//! Settings are grouped by area. Some are declared but not read yet; each of
//! those says "Not read yet" and has a safe default, so a later version can
//! start reading it without breaking code that sets it.

use std::sync::Arc;

use proteus_render::{AtlasConfig, FontAtlas, FontError, QuadInstance};
use proteus_ui::{Easing, TransitionConfig};

// ---------------------------------------------------------------------------
// ProteusConfig
// ---------------------------------------------------------------------------

/// The engine's settings. Start from a preset, [`ProteusConfig::web`],
/// [`ProteusConfig::desktop`] or [`ProteusConfig::constrained`], and change
/// what you need.
///
/// The memory settings must fit the GPU limits the host requests, which
/// [`ProteusConfig::check`] tests without a GPU. `web()` and `constrained()`
/// fit both hosts; `desktop()` fits the native host only.
#[derive(Debug, Clone)]
pub struct ProteusConfig {
    /// GPU memory: atlas and instance-buffer sizes.
    pub memory: MemoryConfig,
    /// Clear color and presentation.
    pub render: RenderConfig,
    /// Frame timing.
    pub frame: FrameConfig,
    /// Input defaults. Not read yet.
    pub input: InputConfig,
    /// Transition defaults. Not read yet.
    pub transitions: TransitionDefaults,
    /// Text rendering.
    pub text: TextConfig,
    /// Image and texture loading.
    pub resources: ResourceConfig,
    /// Diagnostics.
    pub debug: DebugConfig,
}

impl Default for ProteusConfig {
    fn default() -> Self {
        Self::web()
    }
}

impl ProteusConfig {
    /// Settings that work on every host, including WebGL2 in the browser.
    /// Also the default.
    pub fn web() -> Self {
        Self {
            memory: MemoryConfig::default(),
            render: RenderConfig::default(),
            frame: FrameConfig::default(),
            input: InputConfig::default(),
            transitions: TransitionDefaults::default(),
            text: TextConfig::default(),
            resources: ResourceConfig::default(),
            debug: DebugConfig::default(),
        }
    }

    /// Larger atlases and instance buffer, for native desktop or TV with a
    /// capable GPU. Needs at least `wgpu::Limits::default()`, which the native
    /// host requests, so not the web host, which [`ProteusConfig::check`]
    /// holds to WebGL2's limits.
    pub fn desktop() -> Self {
        Self {
            memory: MemoryConfig {
                main_atlas: AtlasConfig {
                    page_size: 4096,
                    page_count: 4,
                },
                transition_atlas_size: 4096,
                max_instances: 16384,
                ..MemoryConfig::default()
            },
            ..Self::web()
        }
    }

    /// Smaller atlases, for devices with little memory, such as embedded
    /// systems and kiosks. Fits both hosts. No single texture can be larger than 1024 pixels on
    /// a side, and textures are evicted more often.
    pub fn constrained() -> Self {
        Self {
            memory: MemoryConfig {
                main_atlas: AtlasConfig {
                    page_size: 1024,
                    page_count: 3,
                },
                transition_atlas_size: 1024,
                max_instances: 4096,
                video: VideoConfig {
                    default_size: (854, 480),
                    ..VideoConfig::default()
                },
            },
            ..Self::web()
        }
    }

    /// Checks the memory settings against a host's GPU `limits`, without a
    /// GPU, so a setting that doesn't fit is found before the app runs. An
    /// app can call it in its own tests with its host's limits:
    /// `proteus_host_winit::limits()` or `proteus_host_web::limits()`.
    ///
    /// Both hosts call it before they create a GPU device, and
    /// [`Renderer::new`](crate::Renderer::new) checks the device it is given.
    ///
    /// # Errors
    ///
    /// The first setting that doesn't fit, as a [`ConfigError`].
    pub fn check(&self, limits: &wgpu::Limits) -> Result<(), ConfigError> {
        let mem = &self.memory;
        let max_side = u64::from(limits.max_texture_dimension_2d);
        let too_big = |setting, value: u64, limit: u64, fix| {
            (value > limit).then_some(ConfigError {
                setting,
                value,
                allowed: Allowed::AtMost(limit),
                fix,
            })
        };
        let error = too_big(
            "memory.main_atlas.page_size",
            mem.main_atlas.page_size.into(),
            max_side,
            "Use smaller pages, and more of them for the same room.",
        )
        .or_else(|| {
            (mem.main_atlas.page_count == 0).then_some(ConfigError {
                setting: "memory.main_atlas.page_count",
                value: 0,
                allowed: Allowed::AtLeast(1),
                fix: "Use at least one page.",
            })
        })
        .or_else(|| {
            too_big(
                "memory.main_atlas.page_count",
                mem.main_atlas.page_count.into(),
                limits.max_texture_array_layers.into(),
                "Use fewer pages.",
            )
        })
        .or_else(|| {
            too_big(
                "memory.transition_atlas_size",
                mem.transition_atlas_size.into(),
                max_side,
                "Use a smaller transition atlas.",
            )
        })
        .or_else(|| {
            let instance_bytes = std::mem::size_of::<QuadInstance>() as u64;
            too_big(
                "memory.max_instances",
                mem.max_instances.into(),
                limits.max_buffer_size / instance_bytes,
                "Use fewer instances.",
            )
        });
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Roughly how much GPU memory these settings use, in bytes: both
    /// atlases and the instance buffers. It leaves out video, which is only
    /// allocated when a video plays, and CPU memory, which depends on the
    /// app's content.
    pub fn estimated_gpu_bytes(&self) -> u64 {
        let m = &self.memory;
        let main = m.main_atlas.page_size as u64
            * m.main_atlas.page_size as u64
            * m.main_atlas.page_count as u64
            * 4;
        let transition = m.transition_atlas_size as u64 * m.transition_atlas_size as u64 * 4;
        let instance_size = std::mem::size_of::<proteus_render::QuadInstance>() as u64;
        // ×2: the bake-instance buffer is the same size as the main one.
        let instances = instance_size * m.max_instances as u64 * 2;
        main + transition + instances
    }
}

// ---------------------------------------------------------------------------
// ConfigError
// ---------------------------------------------------------------------------

/// A setting that doesn't fit a host's GPU limits, from
/// [`ProteusConfig::check`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "ProteusConfig.{setting} is {value}, but this host allows {allowed}. {fix} \
     ProteusConfig::web() fits every host."
)]
pub struct ConfigError {
    /// The setting, such as `"memory.main_atlas.page_size"`.
    pub setting: &'static str,
    /// Its value.
    pub value: u64,
    /// What the host allows.
    pub allowed: Allowed,
    /// How to fix it.
    pub fix: &'static str,
}

/// The values a host allows for a setting, in a [`ConfigError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowed {
    /// At most this value.
    AtMost(u64),
    /// At least this value.
    AtLeast(u64),
}

impl std::fmt::Display for Allowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AtMost(limit) => write!(f, "at most {limit}"),
            Self::AtLeast(limit) => write!(f, "at least {limit}"),
        }
    }
}

// ---------------------------------------------------------------------------
// MemoryConfig
// ---------------------------------------------------------------------------

/// GPU memory sizes, read by [`crate::Renderer::new`].
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    /// The main texture atlas, which holds text, images and baked components:
    /// its page size in pixels and number of pages.
    pub main_atlas: AtlasConfig,
    /// The size in pixels of the square atlas that holds images of components
    /// during splits and merges.
    pub transition_atlas_size: u32,
    /// The most components drawn in one frame. Any beyond it are not drawn.
    pub max_instances: u32,
    /// Video texture defaults. Not read yet.
    pub video: VideoConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            main_atlas: AtlasConfig::default(),
            transition_atlas_size: proteus_render::DEFAULT_TRANSITION_ATLAS_SIZE,
            max_instances: 4096,
            video: VideoConfig::default(),
        }
    }
}

/// Video texture defaults. Not read yet: a video's texture is sized from its
/// first frame when it starts playing.
#[derive(Debug, Clone, Copy)]
pub struct VideoConfig {
    /// Default video size in pixels. Not read yet.
    pub default_size: (u32, u32),
    /// Whether video is supported at all. Not read yet.
    pub enabled: bool,
    /// How many decoded frames a host's decoder should buffer ahead. Not read
    /// yet; a guide for whoever writes a decoder.
    pub channel_depth: usize,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            default_size: (
                proteus_render::DEFAULT_VIDEO_WIDTH,
                proteus_render::DEFAULT_VIDEO_HEIGHT,
            ),
            enabled: true,
            channel_depth: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// RenderConfig
// ---------------------------------------------------------------------------

/// Clear color and presentation settings.
#[derive(Debug, Clone, Copy)]
pub struct RenderConfig {
    /// The color behind everything: RGBA, not premultiplied, each `0`–`1`.
    /// It shows through transparent components, and before the first frame's
    /// content has loaded.
    pub clear_color: [f64; 4],
    /// How frames are synchronized with the display. `AutoVsync`, the
    /// default, matches the display's refresh rate; `AutoNoVsync` and
    /// `Immediate` draw as fast as possible, for benchmarks.
    pub present_mode: wgpu::PresentMode,
    /// Which GPU to prefer on a machine with more than one.
    /// `HighPerformance`, the default, prefers a discrete GPU; `LowPower`
    /// saves battery.
    pub power_preference: wgpu::PowerPreference,
    /// Multisample anti-aliasing sample count. Not read yet: drawing is
    /// always single-sampled. Rounded corners, shadows and glows are smooth
    /// regardless, but the straight edges of a rotated component are not.
    pub msaa_samples: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            clear_color: [0.0, 0.0, 0.0, 1.0],
            present_mode: wgpu::PresentMode::AutoVsync,
            power_preference: wgpu::PowerPreference::HighPerformance,
            msaa_samples: 1,
        }
    }
}

impl RenderConfig {
    /// [`clear_color`](Self::clear_color) as a [`wgpu::Color`].
    pub fn wgpu_clear_color(&self) -> wgpu::Color {
        let [r, g, b, a] = self.clear_color;
        wgpu::Color { r, g, b, a }
    }
}

// ---------------------------------------------------------------------------
// FrameConfig
// ---------------------------------------------------------------------------

/// Frame timing settings.
#[derive(Debug, Clone, Copy)]
pub struct FrameConfig {
    /// The longest time step one frame can take, in seconds. A pause between
    /// frames, such as the first frame after startup, would otherwise skip a
    /// short animation entirely.
    pub dt_clamp_secs: f32,
    /// Keep ticking while the window or tab is hidden, instead of pausing.
    /// Not read yet.
    pub tick_while_hidden: bool,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            dt_clamp_secs: 0.05,
            tick_while_hidden: false,
        }
    }
}

// ---------------------------------------------------------------------------
// InputConfig
// ---------------------------------------------------------------------------

/// Input defaults. Not read yet: input uses fixed defaults, which these
/// fields match.
#[derive(Debug, Clone, Copy)]
pub struct InputConfig {
    /// How long, in milliseconds, a newly focused component waits before it
    /// accepts input.
    pub focus_input_delay_ms: u32,
    /// Default for `TransitionInteractionConfig::allow_pointer`.
    pub transition_interaction_allow_pointer: bool,
    /// Default for `TransitionInteractionConfig::allow_navigation`.
    pub transition_interaction_allow_navigation: bool,
    /// Whether clicking a component also gives it focus. `false` keeps
    /// pointer and keyboard focus separate.
    pub click_moves_focus: bool,
    /// How far the pointer must move, in pixels, before a press counts as a
    /// drag. `0.0` starts dragging immediately, as now.
    pub drag_threshold_px: f32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            focus_input_delay_ms: 0,
            transition_interaction_allow_pointer: false,
            transition_interaction_allow_navigation: false,
            click_moves_focus: true,
            drag_threshold_px: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// TransitionDefaults
// ---------------------------------------------------------------------------

/// Transition defaults. Not read yet: transitions use fixed defaults, which
/// these fields match.
#[derive(Debug, Clone)]
pub struct TransitionDefaults {
    /// The transition used when a component changes interaction style, such
    /// as on hover.
    pub interaction_style: TransitionConfig,
    /// The default transition config.
    pub default_config: TransitionConfig,
}

impl Default for TransitionDefaults {
    fn default() -> Self {
        Self {
            interaction_style: TransitionConfig {
                duration: 0.15,
                delay: 0.0,
                easing: Easing::EaseOutQuad,
            },
            default_config: TransitionConfig::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// TextConfig
// ---------------------------------------------------------------------------

/// Text settings.
#[derive(Debug, Clone)]
pub struct TextConfig {
    /// The font all text is drawn in: the embedded Inter Bold, or your own,
    /// from [`FontSource::from_bytes`].
    pub default_font: FontSource,
    /// A default text size in pixels. Not read yet: every text sets its own
    /// size.
    pub default_size_px: f32,
}

/// Where a font comes from.
#[derive(Debug, Clone)]
pub enum FontSource {
    /// Inter Bold, embedded in Proteus.
    Embedded,
    /// A TTF or OTF font file's bytes. Prefer [`FontSource::from_bytes`],
    /// which checks them. Bytes that turn out not to be a font are logged as
    /// an error when the renderer is created, and the embedded font is used
    /// instead.
    Bytes(Arc<[u8]>),
}

impl FontSource {
    /// A font from a TTF or OTF file's bytes, checked now, so a bad font,
    /// such as a failed download, is an error the app handles rather than a
    /// problem found when the renderer starts.
    ///
    /// # Errors
    ///
    /// [`FontError`] if the bytes aren't a TTF or OTF font that can be read.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, FontError> {
        let bytes = bytes.into();
        FontAtlas::new(&bytes)?;
        Ok(Self::Bytes(bytes))
    }
}

impl Default for TextConfig {
    fn default() -> Self {
        Self {
            default_font: FontSource::Embedded,
            default_size_px: 16.0,
        }
    }
}

// ---------------------------------------------------------------------------
// ResourceConfig
// ---------------------------------------------------------------------------

/// Image and texture loading settings.
#[derive(Debug, Clone, Copy)]
pub struct ResourceConfig {
    /// Scale images down so their longer side is at most this many pixels,
    /// unless the image sets its own
    /// [`max_side`](proteus_ui::Image::max_side). `None` keeps full size, up
    /// to an atlas page: a larger image is scaled down to fit the page.
    pub image_max_side: Option<u32>,
    /// Whether the atlas may evict textures to make room. Not read yet:
    /// least-recently-used textures are always evicted.
    pub eviction: EvictionPolicy,
    /// Wait to bake a component's text or image until the component is
    /// visible.
    pub lazy_load: bool,
}

/// Whether the atlas may evict textures to make room for new ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    /// Evict the least recently used textures first.
    Lru,
    /// Never evict; a texture that doesn't fit fails to load.
    Never,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            image_max_side: None,
            eviction: EvictionPolicy::Lru,
            lazy_load: false,
        }
    }
}

// ---------------------------------------------------------------------------
// DebugConfig
// ---------------------------------------------------------------------------

/// Diagnostic settings.
#[derive(Debug, Clone, Copy)]
pub struct DebugConfig {
    /// Log [`ProteusConfig::estimated_gpu_bytes`] when the renderer starts.
    /// The checks that the settings fit the device always run.
    pub validate_config: bool,
    /// Log dropped channel requests in release builds too; debug builds always
    /// do. Not read yet.
    pub report_dropped_transitions: bool,
    /// Show frame rate, component count and atlas use on screen. Not read yet.
    pub overlay: bool,
    /// Suggest components that could be baked. Not read yet.
    pub bake_hints: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            validate_config: true,
            report_dropped_transitions: cfg!(debug_assertions),
            overlay: false,
            bake_hints: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The limits each host requests: `proteus_host_web::limits()` and
    // `proteus_host_winit::limits()`.
    fn web_limits() -> wgpu::Limits {
        wgpu::Limits::downlevel_webgl2_defaults()
    }
    fn native_limits() -> wgpu::Limits {
        wgpu::Limits::default()
    }

    #[test]
    fn web_and_constrained_fit_both_hosts() {
        for config in [ProteusConfig::web(), ProteusConfig::constrained()] {
            assert_eq!(config.check(&web_limits()), Ok(()));
            assert_eq!(config.check(&native_limits()), Ok(()));
        }
    }

    #[test]
    fn desktop_fits_the_native_host_only() {
        assert_eq!(ProteusConfig::desktop().check(&native_limits()), Ok(()));
        let err = ProteusConfig::desktop().check(&web_limits()).unwrap_err();
        assert_eq!(err.setting, "memory.main_atlas.page_size");
        assert_eq!(err.value, 4096);
        assert_eq!(err.allowed, Allowed::AtMost(2048));
    }

    #[test]
    fn each_setting_that_doesnt_fit_is_named() {
        let limits = web_limits();
        let with = |change: fn(&mut MemoryConfig)| {
            let mut config = ProteusConfig::web();
            change(&mut config.memory);
            config.check(&limits).unwrap_err()
        };

        let e = with(|m| m.main_atlas.page_count = 0);
        assert_eq!(
            (e.setting, e.allowed),
            ("memory.main_atlas.page_count", Allowed::AtLeast(1))
        );
        let e = with(|m| m.main_atlas.page_count = 10_000);
        assert_eq!(e.setting, "memory.main_atlas.page_count");
        let e = with(|m| m.transition_atlas_size = 4096);
        assert_eq!(e.setting, "memory.transition_atlas_size");

        let mut small_buffer = limits.clone();
        small_buffer.max_buffer_size = 1024;
        let mut config = ProteusConfig::web();
        config.memory.max_instances = 1_000;
        assert_eq!(
            config.check(&small_buffer).unwrap_err().setting,
            "memory.max_instances"
        );
    }

    #[test]
    fn the_message_says_what_to_change() {
        let err = ProteusConfig::desktop().check(&web_limits()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "ProteusConfig.memory.main_atlas.page_size is 4096, but this host allows at most \
             2048. Use smaller pages, and more of them for the same room. ProteusConfig::web() \
             fits every host."
        );
    }
}
