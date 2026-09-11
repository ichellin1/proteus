//! [`ProteusConfig`] — runtime configuration threaded through host
//! construction (M13.5).
//!
//! M13.5 fixed the *shape*: every sizing/behaviour knob in the framework
//! lives under one nested config, grouped by concern, rather than scattered
//! as hardcoded constants across `proteus-render` / `proteus-ui` / each
//! host. Not every field is wired to a system yet — each doc comment below
//! says so explicitly. **Wired in this pass:** all of [`MemoryConfig`],
//! [`RenderConfig::clear_color`] / `present_mode` / `power_preference`, and
//! [`FrameConfig::dt_clamp_secs`]. Everything else is a real, documented
//! field with a safe default — the shape is locked now, so plumbing it in
//! later (as the milestone that owns that area comes up) is additive, never
//! a breaking change to callers already holding a `ProteusConfig`.

use std::sync::Arc;

use proteus_render::AtlasConfig;
use proteus_ui::{ease_out_quad, EasingFn, TransitionConfig};

// ---------------------------------------------------------------------------
// ProteusConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ProteusConfig {
    pub memory: MemoryConfig,
    pub render: RenderConfig,
    pub frame: FrameConfig,
    pub input: InputConfig,
    pub transitions: TransitionDefaults,
    pub text: TextConfig,
    pub resources: ResourceConfig,
    pub debug: DebugConfig,
}

impl Default for ProteusConfig {
    fn default() -> Self {
        Self::web()
    }
}

impl ProteusConfig {
    /// The safe floor — works on every host this project ships to, WebGL2
    /// included. Same as `Default::default()`.
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

    /// A native desktop/TV target with real GPU headroom — bigger atlases, a
    /// deeper instance buffer. Needs at least `wgpu::Limits::default()`; do
    /// not use on a WebGL2-limited backend (`validate_render_config` /
    /// `validate_atlas_config` will reject it there).
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

    /// A memory-constrained embedded / kiosk / low-end target. Smaller
    /// atlases and a lower default video size. Tradeoffs (no single baked
    /// region over 1024px/axis, more LRU eviction churn) are documented in
    /// PLANNING.md's M13.5 section.
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

    /// Rough resident GPU bytes at these sizes: `main_atlas` +
    /// `transition_atlas` + the two instance buffers. Excludes the video
    /// atlas (only allocated once a video actually plays — see
    /// [`VideoConfig`]'s doc) and CPU-side memory (the ECS world, the font
    /// glyph cache) — both scale with app content, not this config. A host
    /// can log this against its own memory budget (see
    /// [`DebugConfig::validate_config`]).
    pub fn estimated_gpu_bytes(&self) -> u64 {
        let m = &self.memory;
        let main = m.main_atlas.page_size as u64
            * m.main_atlas.page_size as u64
            * m.main_atlas.page_count as u64
            * 4;
        let transition = m.transition_atlas_size as u64 * m.transition_atlas_size as u64 * 4;
        let instance_size = std::mem::size_of::<proteus_render::QuadInstance>() as u64;
        // ×2: the main instance buffer and the bake-instance buffer are the
        // same size (`QuadPipeline::new`'s own headroom choice).
        let instances = instance_size * m.max_instances as u64 * 2;
        main + transition + instances
    }
}

// ---------------------------------------------------------------------------
// MemoryConfig — wired
// ---------------------------------------------------------------------------

/// GPU memory sizing. Every field here is read by [`crate::Renderer::new`].
#[derive(Debug, Clone, Copy)]
pub struct MemoryConfig {
    /// `main_atlas` sizing (page size in pixels, page count).
    pub main_atlas: AtlasConfig,
    /// `transition_atlas` sizing, pixels (square) — was the hardcoded
    /// `TRANSITION_ATLAS_SIZE` constant before M13.5.
    pub transition_atlas_size: u32,
    /// Instance-buffer capacity — was a bare `QuadPipeline::new` argument
    /// with no upper-bound check before M13.5. Exceeding it silently clamps
    /// (extra instances are dropped from the draw), it doesn't panic.
    pub max_instances: u32,
    pub video: VideoConfig,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            main_atlas: AtlasConfig::default(),
            transition_atlas_size: proteus_render::TRANSITION_ATLAS_SIZE,
            max_instances: 4096,
            video: VideoConfig::default(),
        }
    }
}

/// Video-atlas defaults.
///
/// **Not yet wired.** `QuadPipeline::init_video` still takes its own
/// explicit `width`/`height` from the host (probed via `ffprobe`, or an HLS
/// manifest's metadata) at the moment a video actually starts playing — the
/// video atlas itself is a free 1×1 placeholder until then, so there is no
/// eager allocation for these fields to gate yet. Real wiring (and
/// `enabled: false` skipping video support entirely) is M13.4 territory,
/// once video becomes a host service.
#[derive(Debug, Clone, Copy)]
pub struct VideoConfig {
    pub default_size: (u32, u32),
    pub enabled: bool,
    /// The BYOV frame channel's bound (today a hardcoded `sync_channel(2)`
    /// in `QuadPipeline::init_video`) — how many decoded frames of
    /// lookahead the decoder thread gets before it blocks.
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
// RenderConfig — clear_color / present_mode / power_preference wired
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct RenderConfig {
    /// The surface clear color each frame, straight (non-premultiplied)
    /// RGBA, `0.0`–`1.0`. Shows through component transparency and during
    /// the first frames before any background asset has loaded. **Wired.**
    pub clear_color: [f64; 4],
    /// Swap-chain presentation mode. **Wired** — read by each host when it
    /// configures its surface. `AutoVsync` (default) throttles to the
    /// display's refresh rate; `AutoNoVsync`/`Immediate` suit a benchmark or
    /// a high-refresh display that wants to see every frame land.
    pub present_mode: wgpu::PresentMode,
    /// Adapter selection hint. **Wired** — read by each host at
    /// `request_adapter`. `HighPerformance` (default) prefers a discrete
    /// GPU; `LowPower` suits a battery-sensitive kiosk.
    pub power_preference: wgpu::PowerPreference,
    /// MSAA sample count. **Not yet wired** — the pipeline is always
    /// single-sampled today. The SDF corner-radius/shadow/glow math already
    /// anti-aliases itself, but a rotated quad's straight edges do alias;
    /// wiring this needs a resolve target and a pipeline `multisample`
    /// state change. Post-V1.
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
// FrameConfig — dt_clamp_secs wired
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct FrameConfig {
    /// Per-frame `dt` is clamped to this ceiling before `Proteus::tick` runs.
    /// **Wired** — `Engine::frame` applies it, replacing the identical
    /// `.min(0.05)` the M12 shells (and M13.1's hosts) each hand-duplicated.
    /// Without it, a real stall between frames (most visibly the very first
    /// one, after GPU warm-up) is fed straight into the schedule as one
    /// giant `dt` — easily enough to blow through a short intro animation's
    /// entire delay+fade+hold budget in a single tick.
    pub dt_clamp_secs: f32,
    /// Continue ticking with a real `dt` while the window/tab is hidden,
    /// instead of pausing. **Not yet wired** — no host currently detects
    /// "hidden" uniformly: native has no such concept, and the web host's
    /// `visibilitychange` handling is M13.2.
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
// InputConfig — not yet wired
// ---------------------------------------------------------------------------

/// Input behaviour defaults. **Not yet wired to any system** — `proteus-ui`'s
/// input systems still use their own compiled-in defaults (Phase B:
/// `FocusConfig.input_delay_ms` defaults to 0 per-component,
/// `TransitioningConfig`'s two flags default to `false` per-component). Real
/// wiring is a `RuntimeConfig` ECS resource `Engine::new` would insert
/// alongside `GpuContext`/`QuadPipeline`, read by `interaction_style_system`
/// and the focus/navigation systems the same way `FrameTime` already is.
#[derive(Debug, Clone, Copy)]
pub struct InputConfig {
    /// Global default for Phase B's per-component `FocusConfig.input_delay_ms`
    /// settle window.
    pub focus_input_delay_ms: u32,
    /// Global default for Phase B's per-component
    /// `TransitioningConfig.allow_input`.
    pub transitioning_allow_input: bool,
    /// Global default for Phase B's per-component
    /// `TransitioningConfig.allow_navigation`.
    pub transitioning_allow_navigation: bool,
    /// Whether clicking an `Interactable` also moves keyboard focus to it
    /// (Phase B's click-to-focus). `false` decouples pointer and keyboard
    /// focus entirely.
    pub click_moves_focus: bool,
    /// Minimum pointer movement, in pixels, before a press is reported as a
    /// drag rather than a click. `0.0` (today's behaviour) starts dragging
    /// immediately.
    pub drag_threshold_px: f32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            focus_input_delay_ms: 0,
            transitioning_allow_input: false,
            transitioning_allow_navigation: false,
            click_moves_focus: true,
            drag_threshold_px: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// TransitionDefaults — not yet wired
// ---------------------------------------------------------------------------

/// Animation defaults. **Not yet wired** — `interaction_style_system` still
/// uses its own hardcoded `STYLE_TRANSITION_CONFIG` const, and
/// `TransitionConfig::default()` is a compiled-in default, independent of
/// this struct. Wiring both to read from a `RuntimeConfig` resource (see
/// [`InputConfig`]'s doc) is straightforward once something needs it.
#[derive(Debug, Clone)]
pub struct TransitionDefaults {
    /// The hover/press/focus mini-transition every `InteractionDef` entity
    /// gets (today: `{ duration: 0.15, easing: ease_out_quad }`).
    pub interaction_style: TransitionConfig,
    /// What `signal.set()` uses when the call site doesn't specify a
    /// `TransitionConfig` (today: `TransitionConfig::default()`).
    pub default_config: TransitionConfig,
    /// Named easing functions, for a TS caller to reference a custom
    /// interpolation by string instead of needing a Rust `fn` pointer.
    /// Empty by default — no easings are registered by name today.
    pub custom_easings: Vec<(String, EasingFn)>,
}

impl Default for TransitionDefaults {
    fn default() -> Self {
        Self {
            interaction_style: TransitionConfig {
                duration: 0.15,
                delay: 0.0,
                easing: ease_out_quad,
            },
            default_config: TransitionConfig::default(),
            custom_easings: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// TextConfig — not yet wired
// ---------------------------------------------------------------------------

/// Text defaults. **Not yet wired** — [`crate::Renderer::new`] always builds
/// `FontAtlas::with_embedded_font()`; `default_size_px` has no consumer (every
/// `Text` component already carries its own `size_px`).
#[derive(Debug, Clone)]
pub struct TextConfig {
    /// The font `Renderer` should rasterize with. `FontAtlas::new(&[u8])`
    /// already accepts arbitrary TTF bytes — plumbing this through is a
    /// small change once an app wants a brand font instead of the embedded
    /// Inter-Bold.
    pub default_font: FontSource,
    /// Fallback text size in pixels, for a future `Text` builder default.
    pub default_size_px: f32,
}

#[derive(Debug, Clone)]
pub enum FontSource {
    /// The embedded Inter-Bold (`FontAtlas::with_embedded_font()`).
    Embedded,
    /// Raw TTF/OTF bytes for a custom font.
    Bytes(Arc<[u8]>),
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
// ResourceConfig — image_max_side wired
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ResourceConfig {
    /// Fallback downscale cap (longest side, pixels) for `Image` entities
    /// that don't set their own [`max_side`](proteus_ui::Image::max_side).
    /// `None` = no cap. **Wired** — read by [`crate::Renderer::render`].
    pub image_max_side: Option<u32>,
    /// `main_atlas` eviction policy. **Not yet wired** — `TextureRegistry`
    /// always runs cross-page LRU eviction today; `Never` (for a small app
    /// that would rather fail loudly than silently re-bake) has no effect
    /// yet.
    pub eviction: EvictionPolicy,
    /// Don't decode/upload an `Image`/`Text` until its entity becomes
    /// visible. **Not yet wired** — every entity bakes as soon as
    /// `Renderer::render` sees it, visible or not (Phase A listed this as a
    /// deferred resource concern from the start).
    pub lazy_load: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvictionPolicy {
    Lru,
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
// DebugConfig — validate_config wired
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct DebugConfig {
    /// Log [`ProteusConfig::estimated_gpu_bytes`] at `Renderer::new`.
    /// **Wired.** The hard sizing checks (`validate_atlas_config`,
    /// `validate_render_config`) always run regardless of this flag — they
    /// prevent a real crash, so they aren't optional; this only gates the
    /// informational log line.
    pub validate_config: bool,
    /// Force the `TransitionDropped` messages on in release builds (they're
    /// automatic under `#[cfg(debug_assertions)]` already). **Not yet
    /// wired** — the M12.1 two-tier mechanism has no config input today.
    pub report_dropped_transitions: bool,
    /// An fps / instance-count / atlas-occupancy HUD. **Not yet wired** — no
    /// overlay rendering exists.
    pub overlay: bool,
    /// Surface DevTools hints for subtrees that could be `bake: true`
    /// (Phase B: "no child interaction handlers, no signal bindings on
    /// children"). **Not yet wired.**
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
