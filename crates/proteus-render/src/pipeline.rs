//! `QuadPipeline` — the instanced quad render pipeline.
//!
//! Owns the wgpu render pipeline, the static base-quad geometry buffers, and the
//! per-frame instance buffer. Each frame:
//!
//! 1. Call [`QuadPipeline::set_view_projection`] with the current orthographic matrix.
//! 2. Call [`QuadPipeline::upload_instances`] with all visible [`QuadInstance`]s.
//! 3. Call [`QuadPipeline::draw`] inside an active `wgpu::RenderPass`.
//!
//! One buffer upload and one draw call renders the entire scene.

use std::sync::mpsc::{Receiver, SyncSender};
use wgpu::util::DeviceExt;

use crate::mesh::{quad_vertex_layout, QuadInstance, QUAD_INDICES, QUAD_VERTICES};
use crate::texture_registry::{TextureId, TextureKind, TextureRegistry};

// ---------------------------------------------------------------------------
// Atlas sizes
//
// M1: small fixed sizes — enough for the white-pixel fallback and early dev.
// These will be driven by ProteusConfig (window size) once the config system
// is wired up in M2+.
// ---------------------------------------------------------------------------

/// Default `main_atlas` dimensions. Must fit within `device.limits().max_texture_dimension_2d`.
const DEFAULT_MAIN_ATLAS_SIZE: u32 = 2048;
/// Default `transition_atlas` dimensions (~2× window area for concurrent full-screen bakes).
const DEFAULT_TRANSITION_ATLAS_SIZE: u32 = 2048;
/// Public alias for the main atlas size.
///
/// Use this when creating a [`crate::font_atlas::FontAtlas`] and when
/// converting a [`crate::font_atlas::BakedRegion`] pixel origin into the
/// normalised UV coordinates stored in a [`QuadInstance`].
pub const MAIN_ATLAS_SIZE: u32 = DEFAULT_MAIN_ATLAS_SIZE;

/// Public alias for the transition atlas size — needed to normalize a
/// `transition_atlas` pixel region into UV coordinates (e.g.
/// `BakedTexture::uv_offset`/`uv_scale`), the same way [`MAIN_ATLAS_SIZE`] is
/// used for `main_atlas`.
pub const TRANSITION_ATLAS_SIZE: u32 = DEFAULT_TRANSITION_ATLAS_SIZE;

/// Default video texture dimensions (M9).  1280×720 is enough for a crisp demo;
/// the caller can request a different resolution via [`QuadPipeline::init_video`].
pub const DEFAULT_VIDEO_WIDTH: u32 = 1280;
/// Height of the default video texture (M9).
pub const DEFAULT_VIDEO_HEIGHT: u32 = 720;

// ---------------------------------------------------------------------------
// GpuContext
// ---------------------------------------------------------------------------

/// `wgpu::Device`/`Queue` handles, insertable into the bevy ECS `World` as a
/// resource so transition-setup systems can trigger GPU bakes themselves
/// (see `proteus_ui::topology::one_to_n_setup_system`). The design PLANNING.md
/// originally sketched as an ECS resource named `GpuContext`.
///
/// Deliberately excludes the swapchain `surface` PLANNING.md's original sketch
/// included — that stays shell-owned (native window vs. web canvas differ,
/// and nothing on the bake path touches it).
///
/// `Device`/`Queue` are cheap, `Arc`-backed handles — clone freely. The shell
/// keeps its own copies for swapchain/surface work alongside inserting this.
#[derive(Clone)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl bevy_ecs::prelude::Resource for GpuContext {}

// SAFETY: same reasoning as `QuadPipeline`'s manual Send/Sync impls above —
// sound on the single-threaded wasm32-unknown-unknown target this project
// builds for; a no-op override on native, where `Device`/`Queue` are already
// genuinely `Send + Sync`.
unsafe impl Send for GpuContext {}
unsafe impl Sync for GpuContext {}

// ---------------------------------------------------------------------------
// VideoFrameSender
// ---------------------------------------------------------------------------

/// The sending half of the BYOV (bring-your-own-video-player) channel.
///
/// Obtain one from [`QuadPipeline::init_video`].  Move it into your decoder
/// thread and call [`send`](VideoFrameSender::send) once per decoded frame.
/// The channel is bounded to **2 frames** so the decoder thread blocks
/// naturally when the render loop falls behind — no unbounded memory growth.
///
/// Dropping the sender signals to the pipeline that no more frames will
/// arrive; [`QuadPipeline::consume_video_frame`] becomes a no-op.
pub struct VideoFrameSender {
    tx: SyncSender<Vec<u8>>,
    /// Width of the video texture this sender targets.
    pub width: u32,
    /// Height of the video texture this sender targets.
    pub height: u32,
}

impl VideoFrameSender {
    /// Send one frame of raw RGBA pixels (`width × height × 4` bytes).
    ///
    /// Blocks when the internal 2-frame buffer is full.  This is intentional
    /// backpressure — **do not add an artificial sleep** in your decoder thread.
    /// The pipeline drains the channel once per render frame, so `send` unblocks
    /// at approximately the display refresh rate.  For real video, advance frames
    /// by comparing PTS against playback time; for synthetic content, derive `t`
    /// from `Instant::now()` rather than a fixed increment.
    ///
    /// Returns `false` if the pipeline has been dropped — exit the decoder loop.
    pub fn send(&self, rgba: Vec<u8>) -> bool {
        debug_assert_eq!(
            rgba.len(),
            (self.width * self.height * 4) as usize,
            "VideoFrameSender::send: expected {}×{}×4 bytes, got {}",
            self.width,
            self.height,
            rgba.len(),
        );
        self.tx.send(rgba).is_ok()
    }
}

// ---------------------------------------------------------------------------
// QuadPipeline
// ---------------------------------------------------------------------------

pub struct QuadPipeline {
    // Core render pipeline — targets the swapchain's `surface_format`.
    pipeline: wgpu::RenderPipeline,
    // Same shader/layout, but targets `Rgba8Unorm` — `main_atlas`'s format —
    // for baking into the atlas (see `bake_instances_to_main_atlas`). wgpu
    // pipelines are tied to one color target format, so this can't share
    // `pipeline` above even though everything else about it is identical.
    atlas_pipeline: wgpu::RenderPipeline,

    // Static base-quad geometry — uploaded once at init, never changed
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,

    // Instance buffer — overwritten every frame
    instance_buffer: wgpu::Buffer,
    instance_count: u32,
    max_instances: u32,

    // Separate instance buffer for bake passes — same rationale as
    // `bake_uniform_buffer`: a bake must never clobber state the main
    // per-frame draw depends on.
    bake_instance_buffer: wgpu::Buffer,

    // Frame-level uniforms (view/projection matrix)
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup, // bind group 0

    // Separate uniform for bake passes — see the doc comment where it's
    // created for why this can't share `uniform_buffer` above.
    bake_uniform_buffer: wgpu::Buffer,
    bake_uniform_bind_group: wgpu::BindGroup,

    // Atlas textures — kept alive so the bind group views remain valid.
    // The bind group (group 1) includes all three; it is rebuilt whenever the
    // video_atlas is swapped (suspend/resume).
    _main_atlas: wgpu::Texture,
    _transition_atlas: wgpu::Texture,
    /// Streaming video texture (M9).  Starts as a 1×1 black placeholder; replaced
    /// by [`init_video`] with the requested resolution.
    video_atlas: wgpu::Texture,
    /// Pixel dimensions of the current `video_atlas` allocation.
    video_atlas_size: (u32, u32),
    /// Stored so `rebuild_atlas_bind_group` can recreate group 1 without
    /// accessing the pipeline layout (which is not needed after creation).
    atlas_layout: wgpu::BindGroupLayout,
    /// Shared sampler — kept alive for bind group rebuilds.
    sampler: wgpu::Sampler,
    atlas_bind_group: wgpu::BindGroup, // bind group 1

    /// Metadata store for textures beyond the core atlases.
    pub texture_registry: TextureRegistry,

    /// Receiving end of the BYOV frame channel.  `None` until [`init_video`] is called.
    /// Each frame [`consume_video_frame`] drains this and uploads the latest.
    /// `Receiver<T>` is `Send` but not `Sync` — wrapped so `QuadPipeline` can
    /// be a bevy ECS `Resource` (requires `Sync`). `consume_video_frame` only
    /// ever takes `&self`, so this needs a real lock (not just `Mutex::get_mut`,
    /// which requires `&mut self`).
    video_rx: std::sync::Mutex<Option<Receiver<Vec<u8>>>>,

    /// Sub-region allocator for `transition_atlas`. See `transition_atlas` module docs.
    transition_allocator: crate::transition_atlas::TransitionAtlasAllocator,
}

// `QuadPipeline` is inserted directly into the bevy ECS `World` as a resource
// (see `GpuContext` below) so transition-setup systems can trigger bakes
// themselves — see PLANNING.md's `GpuContext` design.
impl bevy_ecs::prelude::Resource for QuadPipeline {}

// SAFETY: on wasm32-unknown-unknown (no `atomics`/shared-memory target
// feature — the target this project builds for), there is exactly one
// thread of execution: the browser's JS main thread. wgpu's wasm32 backend
// uses non-atomic/non-thread-safe internals for some fields (e.g.
// `Box<dyn DynCommandBuffer>`, `RefCell`-like storage) that are `!Send`/
// `!Sync` there even though the equivalent native backend's
// `Arc<Mutex<..>>`-based internals are genuinely `Send + Sync` — correctly
// so on a real multi-threaded target, but on this single-threaded target
// `QuadPipeline` can never actually move across or be observed from two
// threads at once, so asserting both here is sound in practice. A manual
// `impl` replaces (does not conflict with) whatever auto-derivation would
// otherwise apply, so this is a no-op on native (already genuinely
// `Send + Sync` there) and the necessary override on wasm32.
unsafe impl Send for QuadPipeline {}
unsafe impl Sync for QuadPipeline {}

impl QuadPipeline {
    // ---------------------------------------------------------------------------
    // White-pixel sentinel UV constants
    //
    // The main_atlas has a 1×1 white pixel baked at its origin. Components with
    // no image texture point at this pixel so their `color` field alone
    // determines appearance without any shader branching.
    //
    // Using the texel *center* (0.5/atlas_size) and zero scale avoids the
    // linear-sampler edge bleed that occurs when sampling at the texel boundary.
    // ---------------------------------------------------------------------------

    /// UV offset for the white-pixel sentinel — center of the 1×1 texel at atlas origin.
    ///
    /// Assign to `QuadInstance::uv_offset` when the component has no image texture.
    pub const WHITE_PIXEL_UV_OFFSET: [f32; 2] = [
        0.5 / DEFAULT_MAIN_ATLAS_SIZE as f32,
        0.5 / DEFAULT_MAIN_ATLAS_SIZE as f32,
    ];

    /// UV scale for the white-pixel sentinel — zero means all fragments sample the
    /// same point (the offset), preventing any bilinear bleed into adjacent texels.
    ///
    /// Assign to `QuadInstance::uv_scale` when the component has no image texture.
    pub const WHITE_PIXEL_UV_SCALE: [f32; 2] = [0.0, 0.0];

    /// Create the render pipeline, upload static geometry, and initialize atlas textures.
    ///
    /// `surface_format` must match the swap-chain texture format of the target surface.
    /// `max_instances` sets the capacity of the instance buffer — the pipeline silently
    /// clamps submissions that exceed it. 4096 is a reasonable default for most UIs.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        max_instances: u32,
    ) -> Self {
        // --- Shader ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::QUAD_SHADER_SRC.into()),
        });

        // --- Bind group layouts ---

        // Group 0: view/projection uniform (vertex stage only)
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uniform_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Group 1: main_atlas + transition_atlas textures + sampler (fragment stage only)
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas_bgl"),
            entries: &[
                // binding 0: main_atlas
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 1: transition_atlas
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 2: sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // binding 3: video_atlas (M9) — streaming per-frame video texture
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        // --- Pipeline layout ---
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad_pipeline_layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&atlas_layout)],
            immediate_size: 0,
        });

        // --- Render pipelines ---
        // wgpu render pipelines are baked for one specific color target format —
        // a pipeline created for the swapchain's format cannot render into a
        // texture with a different format. The main per-frame draw always
        // targets the swapchain (`surface_format`), but baking into `main_atlas`
        // (always `Rgba8Unorm`, see `create_atlases`) needs a second pipeline
        // sharing everything else (shader, layout, vertex/instance buffers).
        let pipeline =
            Self::build_render_pipeline(device, &shader, &pipeline_layout, surface_format);
        let atlas_pipeline = Self::build_render_pipeline(
            device,
            &shader,
            &pipeline_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        );

        // --- Static geometry buffers ---
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_vertex_buf"),
            contents: bytemuck::cast_slice(&QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_index_buf"),
            contents: bytemuck::cast_slice(&QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        // --- Instance buffer (written each frame via queue.write_buffer) ---
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad_instance_buf"),
            size: (std::mem::size_of::<QuadInstance>() as u64) * (max_instances as u64),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Bake instance buffer — separate from the one above; see its
        // field doc comment. Bakes only ever draw a handful of instances (one
        // entity's own background + text overlay), but sized the same as the
        // main buffer for headroom (e.g. a future composite bake).
        let bake_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad_bake_instance_buf"),
            size: (std::mem::size_of::<QuadInstance>() as u64) * (max_instances as u64),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Uniform buffer (view/projection matrix, 64 bytes) ---
        let identity: [[f32; 4]; 4] = glam::Mat4::IDENTITY.to_cols_array_2d();
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_uniform_buf"),
            contents: bytemuck::cast_slice(&identity),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bg"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // --- Bake uniform buffer — entirely separate from the main scene's.
        //
        // Bakes (bake_instances_to_atlas) need their own view-projection (framed
        // tightly on whatever entity is being baked), issued mid-frame from
        // inside an ECS system that has no idea what the main scene's screen-size
        // ortho even is. If bakes wrote into `uniform_buffer` above, they'd leave
        // the *main* per-frame draw permanently using that tiny, off-center
        // projection — the shell only calls `set_view_projection` at init/resize,
        // not every frame, so nothing would ever restore it. A dedicated buffer
        // means baking can never clobber the main scene's camera, full stop.
        let bake_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad_bake_uniform_buf"),
            contents: bytemuck::cast_slice(&identity),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bake_uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake_uniform_bg"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bake_uniform_buffer.as_entire_binding(),
            }],
        });

        // --- Atlas textures ---
        let (main_atlas, transition_atlas) = Self::create_atlases(device, queue);

        // Video atlas starts as a 1×1 black placeholder.  Call init_video() to
        // allocate a real resolution before uploading frames.
        let video_atlas = Self::create_video_texture(device, 1, 1);

        let main_atlas_view = main_atlas.create_view(&Default::default());
        let transition_atlas_view = transition_atlas.create_view(&Default::default());
        let video_atlas_view = video_atlas.create_view(&Default::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bg"),
            layout: &atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&main_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&transition_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&video_atlas_view),
                },
            ],
        });

        Self {
            pipeline,
            atlas_pipeline,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            instance_count: 0,
            max_instances,
            bake_instance_buffer,
            uniform_buffer,
            uniform_bind_group,
            bake_uniform_buffer,
            bake_uniform_bind_group,
            _main_atlas: main_atlas,
            _transition_atlas: transition_atlas,
            video_atlas,
            video_atlas_size: (1, 1),
            atlas_layout,
            sampler,
            atlas_bind_group,
            texture_registry: TextureRegistry::new(),
            video_rx: std::sync::Mutex::new(None),
            transition_allocator: crate::transition_atlas::TransitionAtlasAllocator::new(
                TRANSITION_ATLAS_SIZE,
            ),
        }
    }

    // ---------------------------------------------------------------------------
    // Per-frame API
    // ---------------------------------------------------------------------------

    /// Upload a new view/projection matrix for this frame.
    /// Call once per frame before `draw()`.
    pub fn set_view_projection(&self, queue: &wgpu::Queue, matrix: glam::Mat4) {
        let data: [[f32; 4]; 4] = matrix.to_cols_array_2d();
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&data));
    }

    /// Upload the instance list for this frame. Silently clamps to `max_instances`.
    /// Call once per frame before `draw()`.
    pub fn upload_instances(&mut self, queue: &wgpu::Queue, instances: &[QuadInstance]) {
        let count = instances.len().min(self.max_instances as usize);
        if count < instances.len() {
            log::warn!(
                "QuadPipeline: submitted {} instances but capacity is {}; excess dropped",
                instances.len(),
                self.max_instances,
            );
        }
        self.instance_count = count as u32;
        if count > 0 {
            queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&instances[..count]),
            );
        }
    }

    /// Issue the draw call. Must be called inside an active `wgpu::RenderPass`.
    /// Call after `upload_instances()`.
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_bind_group(1, &self.atlas_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..self.instance_count);
    }

    // ---------------------------------------------------------------------------
    // Atlas write API (M4 text pipeline)
    // ---------------------------------------------------------------------------

    /// Write a rectangular region of RGBA pixels into `main_atlas`.
    ///
    /// Use this to upload a pre-baked text bitmap produced by
    /// [`FontAtlas::bake_text`] into the GPU texture so it can be sampled by
    /// the shader.
    ///
    /// `x` / `y`    — top-left pixel of the destination region (atlas coords).
    /// `width` / `height` — dimensions of the region in pixels.
    /// `rgba_data`  — raw RGBA bytes; must have exactly `width * height * 4` bytes.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `rgba_data.len() != width * height * 4`.
    ///
    /// [`FontAtlas::bake_text`]: crate::font_atlas::FontAtlas::bake_text
    pub fn write_to_main_atlas(
        &self,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba_data: &[u8],
    ) {
        debug_assert_eq!(
            rgba_data.len(),
            (width * height * 4) as usize,
            "write_to_main_atlas: rgba_data length {} does not match {}×{}×4={}",
            rgba_data.len(),
            width,
            height,
            width * height * 4,
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self._main_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    // ---------------------------------------------------------------------------
    // Transition atlas allocation
    // ---------------------------------------------------------------------------

    /// Allocate a `width × height` region within `transition_atlas`.
    ///
    /// Returns `None` if the atlas is full — callers (the transition setup
    /// systems) should fall back to flat-color geometry for that slice rather
    /// than failing the whole transition.
    pub fn allocate_transition_region(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(crate::TransitionAllocId, crate::TransitionRegion)> {
        self.transition_allocator.allocate(width, height)
    }

    /// Release a region allocated by [`Self::allocate_transition_region`] back
    /// to the packer. Call once a transition using it (or the source/target
    /// bake it held) is fully complete.
    pub fn free_transition_region(&mut self, id: crate::TransitionAllocId) {
        self.transition_allocator.free(id);
    }

    // ---------------------------------------------------------------------------
    // Transition-bake API
    // ---------------------------------------------------------------------------

    /// Render `instances` into a `width × height` sub-region of `main_atlas`,
    /// preserving the rest of the atlas untouched — a snapshot of one entity's
    /// on-screen appearance (shape, border, baked text) that other entities can
    /// then UV-address via [`crate::QuadInstance::uv_offset`]/`uv_scale`.
    ///
    /// For a *static* texture reference (no crossfade), use this. For a bake
    /// that a virtual slice will crossfade away from as its own transition
    /// progresses, use [`Self::bake_instances_to_transition_atlas`] instead —
    /// the fragment shader's crossfade path only reads `base_uv_offset`/`scale`
    /// from `transition_atlas`, never `main_atlas`.
    ///
    /// See [`Self::bake_instances_to_transition_atlas`] for the full
    /// parameter/behavior docs (scratch-texture rationale, dedicated
    /// uniform/instance buffers, self-contained encoder/submit) — identical
    /// here, just targeting the other atlas.
    pub fn bake_instances_to_main_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[QuadInstance],
        view_projection: glam::Mat4,
        region: (u32, u32, u32, u32),
    ) {
        let dest = self._main_atlas.clone();
        self.bake_instances_to_atlas(device, queue, instances, view_projection, region, &dest);
    }

    /// Render `instances` into a `width × height` sub-region of
    /// `transition_atlas` — the atlas the fragment shader's crossfade path
    /// reads `base_uv_offset`/`base_uv_scale` from (see `quad.wgsl`:
    /// `if in.crossfade_t > 0.0 { ... transition_atlas ... }`).
    ///
    /// This is how a 1→N Slice virtual crossfades from an actual crop of the
    /// source entity's rendered appearance toward its own target state as it
    /// morphs, instead of either (a) a flat-color geometry-only slice with no
    /// texture at all, or (b) a static baked texture that never fades: bake
    /// the source once into `transition_atlas`, point each slice's
    /// `base_uv_offset`/`scale` at its third of the region, and drive
    /// `crossfade_t` from the slice's own `ActiveTransition` progress each
    /// frame (see `proteus_ui::BakedTexture` / `collect.rs`).
    ///
    /// `view_projection` should be [`Self::ortho_centered`] on the source
    /// entity's own position/size, so its geometry fills the target region
    /// exactly. `region` is `(x, y, width, height)` in atlas pixel
    /// coordinates — the caller is responsible for choosing a region that
    /// doesn't collide with other content in the same atlas.
    ///
    /// ## Why this renders to a scratch texture first, not the atlas directly
    ///
    /// `instances` typically sample `main_atlas` themselves — the white-pixel
    /// fill sentinel, baked text glyphs — and a texture cannot be both a
    /// render attachment and a sampled texture in the same render pass (this
    /// would only be a hazard for `main_atlas` itself, but the scratch
    /// indirection is shared code for both atlases). It renders into a
    /// throwaway `Rgba8Unorm` texture sized exactly to `region` (fully
    /// cleared to transparent — it's fresh each call, so no risk of a
    /// previous bake's pixels bleeding through), then
    /// `copy_texture_to_texture`s the result into the reserved atlas region,
    /// all within one encoder/submit.
    ///
    /// This method is fully self-contained: its own command encoder, its own
    /// dedicated uniform/instance buffers (entirely separate from the ones
    /// the main per-frame draw uses — see their field doc comments), and its
    /// own submit before returning. Safe to call from anywhere, including
    /// mid-frame from inside an ECS system, without disturbing the main
    /// scene's camera or pending instance data in any way.
    pub fn bake_instances_to_transition_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[QuadInstance],
        view_projection: glam::Mat4,
        region: (u32, u32, u32, u32),
    ) {
        let dest = self._transition_atlas.clone();
        self.bake_instances_to_atlas(device, queue, instances, view_projection, region, &dest);
    }

    fn bake_instances_to_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[QuadInstance],
        view_projection: glam::Mat4,
        region: (u32, u32, u32, u32),
        dest_texture: &wgpu::Texture,
    ) {
        let (x, y, w, h) = region;

        let scratch = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bake_scratch"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let scratch_view = scratch.create_view(&Default::default());

        // Dedicated bake uniform + instance buffers — never touches
        // `uniform_buffer`/`instance_buffer`, which the main per-frame draw
        // relies on staying correct between bakes (see their field doc
        // comments for why sharing them was the wrong call).
        let matrix_data: [[f32; 4]; 4] = view_projection.to_cols_array_2d();
        queue.write_buffer(
            &self.bake_uniform_buffer,
            0,
            bytemuck::cast_slice(&matrix_data),
        );
        let bake_instance_count = instances.len().min(self.max_instances as usize);
        if bake_instance_count < instances.len() {
            log::warn!(
                "QuadPipeline: bake submitted {} instances but capacity is {}; excess dropped",
                instances.len(),
                self.max_instances,
            );
        }
        if bake_instance_count > 0 {
            queue.write_buffer(
                &self.bake_instance_buffer,
                0,
                bytemuck::cast_slice(&instances[..bake_instance_count]),
            );
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bake_to_atlas_encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bake_to_atlas_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &scratch_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Not `self.draw()` — that uses `pipeline`, which is tied to the
            // swapchain's format. The scratch texture is `Rgba8Unorm`, so this
            // pass needs `atlas_pipeline` instead (see its field doc comment).
            if bake_instance_count > 0 {
                pass.set_pipeline(&self.atlas_pipeline);
                pass.set_bind_group(0, &self.bake_uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.bake_instance_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..6, 0, 0..bake_instance_count as u32);
            }
        }
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &scratch,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: dest_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
    }

    // ---------------------------------------------------------------------------
    // Video texture API (M9)
    // ---------------------------------------------------------------------------

    /// Initialize the video texture slot and return the BYOV frame channel.
    ///
    /// Returns `(TextureId, VideoFrameSender)`.  Move the [`VideoFrameSender`]
    /// into your decoder thread and call [`VideoFrameSender::send`] once per
    /// decoded frame.  The pipeline drains the channel each render frame via
    /// [`consume_video_frame`].
    ///
    /// The channel is bounded to **2 frames** so the decoder thread blocks
    /// naturally when the render loop falls behind — no unbounded memory growth.
    ///
    /// Calling `init_video` a second time replaces both the GPU texture and the
    /// channel, implicitly dropping any previous [`VideoFrameSender`].
    ///
    /// [`consume_video_frame`]: QuadPipeline::consume_video_frame
    pub fn init_video(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> (TextureId, VideoFrameSender) {
        self.video_atlas = Self::create_video_texture(device, width, height);
        self.video_atlas_size = (width, height);
        self.rebuild_atlas_bind_group(device);
        let id = self
            .texture_registry
            .register(TextureKind::Video, width, height);

        // Bounded to 2 frames: one frame of lookahead; sender blocks when the
        // render loop is behind, providing natural backpressure.
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        // `&mut self` here — `get_mut` skips locking since exclusive access is
        // already guaranteed.
        *self.video_rx.get_mut().unwrap() = Some(rx);

        (id, VideoFrameSender { tx, width, height })
    }

    /// Drain the BYOV frame channel and upload the latest received frame.
    ///
    /// Call this once per render frame, before [`draw`].  If multiple frames
    /// arrived since the last call (e.g., after a pause) only the freshest is
    /// uploaded — stale intermediate frames are discarded.  No-op if the
    /// channel is empty or if [`init_video`] has not been called.
    ///
    /// [`draw`]: QuadPipeline::draw
    /// [`init_video`]: QuadPipeline::init_video
    pub fn consume_video_frame(&self, queue: &wgpu::Queue) {
        let guard = self.video_rx.lock().unwrap();
        let Some(rx) = guard.as_ref() else { return };
        // After suspend_video() the texture is a 1×1 placeholder.  Uploading a
        // full-resolution frame into it would hit the debug_assert in
        // upload_video_frame and corrupt GPU memory.  Skip until resume_video()
        // restores the full-resolution allocation.
        if self.video_atlas_size == (1, 1) {
            return;
        }
        let mut latest: Option<Vec<u8>> = None;
        let mut drained = 0u32;
        while let Ok(frame) = rx.try_recv() {
            latest = Some(frame);
            drained += 1;
        }
        // More than one frame queued up since the last call means the render
        // loop fell behind the decoder for at least one tick — everything but
        // the freshest gets discarded here. Logged so playback smoothness
        // issues can be attributed to this coalescing vs. decoder-side jitter.
        if drained > 1 {
            log::debug!(
                "consume_video_frame: {} stale frame(s) discarded (render loop behind decoder)",
                drained - 1
            );
        }
        if let Some(frame) = latest {
            self.upload_video_frame(queue, &frame);
        }
    }

    /// Upload one frame of RGBA pixels to the video texture.
    ///
    /// `rgba` must be exactly `width × height × 4` bytes, where `width` and
    /// `height` are the dimensions passed to the most recent [`init_video`] call.
    /// Call this once per render frame while video is playing, before
    /// [`draw`].
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `rgba.len() != width * height * 4`.
    ///
    /// [`init_video`]: QuadPipeline::init_video
    /// [`draw`]: QuadPipeline::draw
    pub fn upload_video_frame(&self, queue: &wgpu::Queue, rgba: &[u8]) {
        let (w, h) = self.video_atlas_size;
        debug_assert_eq!(
            rgba.len(),
            (w * h * 4) as usize,
            "upload_video_frame: expected {}×{}×4={} bytes, got {}",
            w,
            h,
            w * h * 4,
            rgba.len(),
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.video_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Release the video texture GPU memory (e.g., when the app is backgrounded).
    ///
    /// Replaces the full-resolution `video_atlas` with a 1×1 black placeholder,
    /// freeing the bulk of GPU memory used by the video.  The [`TextureId`]
    /// remains valid but [`TextureRegistry::is_active`] returns `false`.
    ///
    /// Call [`resume_video`] when the app returns to the foreground.
    ///
    /// [`resume_video`]: QuadPipeline::resume_video
    pub fn suspend_video(&mut self, device: &wgpu::Device, id: TextureId) {
        self.video_atlas = Self::create_video_texture(device, 1, 1);
        self.video_atlas_size = (1, 1);
        self.rebuild_atlas_bind_group(device);
        self.texture_registry.mark_suspended(id);
    }

    /// Re-allocate the video texture after [`suspend_video`].
    ///
    /// Creates a fresh texture at the given resolution and rebuilds the bind
    /// group.  [`TextureRegistry::is_active`] returns `true` again after this
    /// call.  Upload frames immediately afterward.
    ///
    /// [`suspend_video`]: QuadPipeline::suspend_video
    pub fn resume_video(&mut self, device: &wgpu::Device, id: TextureId, width: u32, height: u32) {
        self.video_atlas = Self::create_video_texture(device, width, height);
        self.video_atlas_size = (width, height);
        self.rebuild_atlas_bind_group(device);
        self.texture_registry.mark_active(id);
    }

    // ---------------------------------------------------------------------------
    // Projection helpers
    // ---------------------------------------------------------------------------

    /// Orthographic projection for a viewport of `width` × `height` pixels.
    ///
    /// - Origin at viewport center, Y-up, 1 unit = 1 pixel.
    /// - Depth range: Z 0 → 1000 maps to NDC 0 → 1 (wgpu convention).
    /// - DPI scaling should be applied by the caller: pass physical pixels, not logical ones.
    pub fn ortho(width: f32, height: f32) -> glam::Mat4 {
        Self::ortho_centered(0.0, 0.0, width, height)
    }

    /// Orthographic projection for a `width` × `height` viewport centered at
    /// `(center_x, center_y)` in world space, instead of the world origin.
    ///
    /// Used to "frame" an arbitrary entity — e.g. baking one entity's own
    /// appearance into an offscreen atlas region wants a projection centered
    /// on *that entity*, not the screen.
    pub fn ortho_centered(center_x: f32, center_y: f32, width: f32, height: f32) -> glam::Mat4 {
        // wgpu NDC: X [-1,1] left→right, Y [-1,1] bottom→top, Z [0,1] near→far.
        // glam's orthographic_rh maps depth to [-1,1] (OpenGL), so we construct
        // the matrix directly for the [0,1] depth convention wgpu expects.
        let sx = 2.0 / width;
        let sy = 2.0 / height;
        let sz = 1.0 / 1000.0; // depth range 0..1000

        glam::Mat4::from_cols(
            glam::Vec4::new(sx, 0.0, 0.0, 0.0),
            glam::Vec4::new(0.0, sy, 0.0, 0.0),
            glam::Vec4::new(0.0, 0.0, sz, 0.0),
            glam::Vec4::new(-center_x * sx, -center_y * sy, 0.0, 1.0),
        )
    }

    // ---------------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------------

    /// Build a quad render pipeline targeting `color_format`. Used to create
    /// both `pipeline` (swapchain format) and `atlas_pipeline` (`main_atlas`'s
    /// `Rgba8Unorm` format) from identical shader/layout/vertex state — see
    /// the field doc comments on `QuadPipeline::atlas_pipeline` for why two
    /// pipelines are needed at all.
    fn build_render_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        pipeline_layout: &wgpu::PipelineLayout,
        color_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad_pipeline"),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[quad_vertex_layout(), QuadInstance::buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    // Standard alpha blending: src_alpha * src + (1 - src_alpha) * dst
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // No back-face culling — quads are flat
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None, // Z ordering via instance sort order for now
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Create a blank RGBA video texture of the given dimensions.
    fn create_video_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video_atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Linear (not sRGB) to match main_atlas and transition_atlas, avoiding
            // brightness discontinuity when morphing between them at crossfade endpoints.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Rebuild the atlas bind group (group 1) after `video_atlas` has changed.
    ///
    /// This is called by [`init_video`], [`suspend_video`], and [`resume_video`]
    /// whenever the video texture is swapped for a different allocation.
    fn rebuild_atlas_bind_group(&mut self, device: &wgpu::Device) {
        let main_view = self._main_atlas.create_view(&Default::default());
        let transition_view = self._transition_atlas.create_view(&Default::default());
        let video_view = self.video_atlas.create_view(&Default::default());

        self.atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bg"),
            layout: &self.atlas_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&main_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&transition_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&video_view),
                },
            ],
        });
    }

    /// Create `main_atlas` and `transition_atlas` and bake a 1×1 white pixel at
    /// the origin of `main_atlas`. Components with no texture point at this pixel
    /// so their `color` field alone determines their appearance with no shader branching.
    fn create_atlases(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (wgpu::Texture, wgpu::Texture) {
        let main_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("main_atlas"),
            size: wgpu::Extent3d {
                width: DEFAULT_MAIN_ATLAS_SIZE,
                height: DEFAULT_MAIN_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        // Write a 1×1 white pixel at (0, 0) in main_atlas.
        // Components with no texture use uv_offset=[0,0], uv_scale=[1/atlas_size].
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &main_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255], // RGBA white
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );

        let transition_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transition_atlas"),
            size: wgpu::Extent3d {
                width: DEFAULT_TRANSITION_ATLAS_SIZE,
                height: DEFAULT_TRANSITION_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        (main_atlas, transition_atlas)
    }
}
