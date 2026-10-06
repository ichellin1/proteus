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

use wgpu::util::DeviceExt;

use crate::mesh::{quad_vertex_layout, QuadInstance, QUAD_INDICES, QUAD_VERTICES};
use crate::texture_registry::{AtlasConfig, TextureId, TextureRegistry};

/// Checks that `config` fits `device.limits()`. Call it before creating a
/// [`QuadPipeline`], which otherwise fails with an unclear wgpu validation
/// panic.
///
/// Natively and on WebGL2, `device.limits()` are exactly the limits the host
/// requested. With WebGPU in a browser, a requested limit lower than
/// WebGPU's default is raised to the default: the web host requests WebGL2's
/// 2048-pixel textures but gets 8192. So on the web, a config can pass with
/// WebGPU and fail with WebGL2. To be sure a config works in every browser,
/// keep it within WebGL2's limits.
pub fn validate_atlas_config(device: &wgpu::Device, config: &AtlasConfig) -> Result<(), String> {
    let limits = device.limits();
    if config.page_size > limits.max_texture_dimension_2d {
        return Err(format!(
            "AtlasConfig.page_size={} exceeds this device's max_texture_dimension_2d={}",
            config.page_size, limits.max_texture_dimension_2d
        ));
    }
    if config.page_count == 0 {
        return Err("AtlasConfig.page_count must be at least 1".to_string());
    }
    if config.page_count > limits.max_texture_array_layers {
        return Err(format!(
            "AtlasConfig.page_count={} exceeds this device's max_texture_array_layers={}",
            config.page_count, limits.max_texture_array_layers
        ));
    }
    Ok(())
}

/// Checks `transition_atlas_size` and `max_instances` against this device's
/// limits, as [`validate_atlas_config`] does for the main atlas.
pub fn validate_render_config(
    device: &wgpu::Device,
    transition_atlas_size: u32,
    max_instances: u32,
) -> Result<(), String> {
    let limits = device.limits();
    if transition_atlas_size > limits.max_texture_dimension_2d {
        return Err(format!(
            "transition_atlas_size={transition_atlas_size} exceeds this device's max_texture_dimension_2d={}",
            limits.max_texture_dimension_2d
        ));
    }
    let instance_buf_bytes = std::mem::size_of::<QuadInstance>() as u64 * max_instances as u64;
    if instance_buf_bytes > limits.max_buffer_size {
        return Err(format!(
            "max_instances={max_instances} needs a {instance_buf_bytes}-byte instance buffer, \
             exceeding this device's max_buffer_size={}",
            limits.max_buffer_size
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Default sizes
// ---------------------------------------------------------------------------

/// The default size of each main-atlas page, in pixels.
///
/// It must fit `max_texture_dimension_2d`, which WebGL2 limits to 2048, so it
/// can't be larger on the web. For more room, add pages instead: see
/// [`DEFAULT_MAIN_ATLAS_PAGE_COUNT`] and [`crate::texture_registry::AtlasConfig`].
///
/// A main-atlas region is converted to texture coordinates by dividing by the
/// page size: this value for a default pipeline, or the configured
/// `AtlasConfig::page_size` otherwise.
pub const DEFAULT_MAIN_ATLAS_SIZE: u32 = 2048;

/// The default number of main-atlas pages.
///
/// At the defaults the main atlas holds 4 pages of 2048 × 2048 × 4 bytes:
/// 64 MiB of GPU memory, allocated up front. The count is fixed when the atlas
/// is created, since an array texture can't grow without being recreated; the
/// [`crate::texture_registry::TextureRegistry`] keeps usage within it by
/// evicting the least recently used textures.
///
/// Every target allows 256 array layers, so the page count, unlike the page
/// size, isn't limited by WebGL2.
pub const DEFAULT_MAIN_ATLAS_PAGE_COUNT: u32 = 4;

/// The default size of the transition atlas, about twice the window's area,
/// so full-screen bakes can overlap.
///
/// Transition-atlas regions are converted to texture coordinates by dividing by
/// this value for a default pipeline, or by the configured
/// `transition_atlas_size` otherwise.
pub const DEFAULT_TRANSITION_ATLAS_SIZE: u32 = 2048;

/// The default video texture width. [`QuadPipeline::init_video`] sets the real
/// size.
pub const DEFAULT_VIDEO_WIDTH: u32 = 1280;
/// The default video texture height.
pub const DEFAULT_VIDEO_HEIGHT: u32 = 720;

// ---------------------------------------------------------------------------
// GpuContext
// ---------------------------------------------------------------------------

/// The GPU device and queue, as an ECS resource, so systems such as split setup
/// can bake textures.
///
/// It doesn't include the surface, which the host owns. The handles are cheap
/// to clone.
#[derive(Clone)]
pub struct GpuContext {
    /// The GPU device.
    pub device: wgpu::Device,
    /// The device's command queue.
    pub queue: wgpu::Queue,
}

impl bevy_ecs::prelude::Resource for GpuContext {}

// SAFETY: as for `QuadPipeline`'s `Send`/`Sync` impls below. On native these
// types are already `Send + Sync`.
unsafe impl Send for GpuContext {}
unsafe impl Sync for GpuContext {}

// ---------------------------------------------------------------------------
// QuadPipeline
// ---------------------------------------------------------------------------

/// The render pipeline that draws every component as an instanced quad, with
/// its buffers and texture atlases.
///
/// Each frame: set the projection with [`QuadPipeline::set_view_projection`],
/// upload the instances with [`QuadPipeline::upload_instances`], then call
/// [`QuadPipeline::draw`] in a render pass. One upload and one draw call render
/// everything.
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

    // The atlas textures. Uploads and bakes write to them, and the atlas bind
    // group (group 1) samples them along with `video_atlas`; it is rebuilt
    // whenever `video_atlas` is replaced.
    main_atlas: wgpu::Texture,
    transition_atlas: wgpu::Texture,
    /// The video texture. A 1×1 black placeholder until
    /// [`init_video`](QuadPipeline::init_video) gives it a size.
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

    /// Sub-region allocator for `transition_atlas`. See `transition_atlas` module docs.
    transition_allocator: crate::transition_atlas::TransitionAtlasAllocator,
}

// `QuadPipeline` is an ECS resource, so that systems can bake textures.
impl bevy_ecs::prelude::Resource for QuadPipeline {}

// SAFETY: on wasm32-unknown-unknown without atomics, which is the target built
// for, there is only one thread, the browser's main thread. Some of wgpu's
// wasm types aren't `Send`/`Sync`, but `QuadPipeline` can never be used from
// two threads there. On native, wgpu's types are already `Send + Sync`, so
// these impls change nothing.
unsafe impl Send for QuadPipeline {}
unsafe impl Sync for QuadPipeline {}

impl QuadPipeline {
    // ---------------------------------------------------------------------------
    // White-pixel texture coordinates
    //
    // The main atlas has a white block at the origin of page 0 (see
    // `create_atlases`). A component with no texture samples it, so its color
    // alone decides how it looks, with no branch in the shader.
    //
    // The offset is exactly (0, 0), the atlas corner, not the center of the
    // first texel. The sampler clamps to the edge, so all four bilinear samples
    // at the corner read texel (0, 0), whatever the page size. A texel center
    // would be a normalized coordinate, so its position would shift with the
    // page size and, on a 4096 page, land between texels and blend in unwritten
    // ones. The whole guard block is white too, so nearby samples are also
    // white.
    // ---------------------------------------------------------------------------

    /// UV offset for the white-pixel sentinel — the atlas origin corner.
    ///
    /// Assign to `QuadInstance::uv_offset` when the component has no image texture.
    pub const WHITE_PIXEL_UV_OFFSET: [f32; 2] = [0.0, 0.0];

    /// UV scale for the white-pixel sentinel — zero means all fragments sample the
    /// same point (the offset), preventing any bilinear bleed into adjacent texels.
    ///
    /// Assign to `QuadInstance::uv_scale` when the component has no image texture.
    pub const WHITE_PIXEL_UV_SCALE: [f32; 2] = [0.0, 0.0];

    /// Creates the render pipeline, its buffers and its atlases.
    ///
    /// `surface_format` must match the surface the pipeline draws to.
    /// `max_instances` is how many quads one frame can draw; more are dropped.
    /// `atlas_config` sizes the main atlas and `transition_atlas_size` the
    /// transition atlas ([`DEFAULT_TRANSITION_ATLAS_SIZE`] by default).
    ///
    /// # Panics
    ///
    /// If the atlas sizes don't fit the device. Check first with
    /// [`crate::validate_atlas_config`] and [`crate::validate_render_config`].
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        max_instances: u32,
        atlas_config: AtlasConfig,
        transition_atlas_size: u32,
    ) -> Self {
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
                // binding 0: main_atlas, an array texture with one layer per page
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
                // binding 3: video_atlas, updated with each video frame
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

        // --- Bake uniform buffer, separate from the main one ---
        //
        // A bake frames the entity being baked with its own projection, and
        // can happen mid-frame, from inside a system. If it wrote the main
        // uniform buffer, the main draw would keep that projection, since
        // `set_view_projection` is only called on resize.
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
        let (main_atlas, transition_atlas) =
            Self::create_atlases(device, queue, &atlas_config, transition_atlas_size);

        // Video atlas starts as a 1×1 black placeholder.  Call init_video() to
        // allocate a real resolution before uploading frames.
        let video_atlas = Self::create_video_texture(device, 1, 1);

        let main_atlas_view = Self::create_main_atlas_view(&main_atlas);
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
            main_atlas,
            transition_atlas,
            video_atlas,
            video_atlas_size: (1, 1),
            atlas_layout,
            sampler,
            atlas_bind_group,
            texture_registry: TextureRegistry::new(atlas_config),
            transition_allocator: crate::transition_atlas::TransitionAtlasAllocator::new(
                transition_atlas_size,
            ),
        }
    }

    // ---------------------------------------------------------------------------
    // Per-frame API
    // ---------------------------------------------------------------------------

    /// Uploads the view/projection matrix. Call it before [`Self::draw`]
    /// whenever the viewport changes.
    pub fn set_view_projection(&self, queue: &wgpu::Queue, matrix: glam::Mat4) {
        let data: [[f32; 4]; 4] = matrix.to_cols_array_2d();
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&data));
    }

    /// Uploads this frame's instances. Instances beyond `max_instances` are
    /// dropped. Call it once per frame, before [`Self::draw`].
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

    /// Draws the uploaded instances in `pass`. Call it after
    /// [`Self::upload_instances`].
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
    // Writing to the main atlas
    // ---------------------------------------------------------------------------

    /// Writes a rectangle of RGBA pixels into one page of `main_atlas`.
    ///
    /// Use this to upload pixels produced by [`FontAtlas::rasterize_text`] or
    /// [`crate::decode_image`], after registering their region via
    /// [`crate::TextureRegistry::register_static`] — pass the [`crate::MainAtlasPlacement`]
    /// [`crate::TextureRegistry::main_atlas_region`] returns straight through, so the page and
    /// rect can never drift apart.
    ///
    /// `rgba_data` — raw, straight-alpha RGBA bytes; must have exactly
    /// `width * height * 4` bytes. Premultiplied in place (a local copy) before
    /// upload — see [`crate::static_texture::premultiply_alpha`]'s doc comment
    /// for why `main_atlas` is stored premultiplied. Callers hand this straight
    /// alpha exactly as `rasterize_text`/`decode_image` produce it.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if `rgba_data.len() != width * height * 4`.
    ///
    /// [`FontAtlas::rasterize_text`]: crate::font_atlas::FontAtlas::rasterize_text
    pub fn write_to_main_atlas(
        &self,
        queue: &wgpu::Queue,
        placement: crate::MainAtlasPlacement,
        rgba_data: &[u8],
    ) {
        let crate::MainAtlasPlacement {
            page,
            x,
            y,
            width,
            height,
        } = placement;
        debug_assert_eq!(
            rgba_data.len(),
            (width * height * 4) as usize,
            "write_to_main_atlas: rgba_data length {} does not match {}×{}×4={}",
            rgba_data.len(),
            width,
            height,
            width * height * 4,
        );

        let mut premultiplied = rgba_data.to_vec();
        crate::static_texture::premultiply_alpha(&mut premultiplied);

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.main_atlas,
                mip_level: 0,
                // `origin.z` is the page (array layer), not a depth.
                origin: wgpu::Origin3d { x, y, z: page },
                aspect: wgpu::TextureAspect::All,
            },
            &premultiplied,
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

    /// Allocates a `width × height` region in `transition_atlas`.
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

    /// Releases a region allocated by [`Self::allocate_transition_region`].
    /// Call it once the transition using it has completed.
    pub fn free_transition_region(&mut self, id: crate::TransitionAllocId) {
        self.transition_allocator.free(id);
    }

    // ---------------------------------------------------------------------------
    // Transition-bake API
    // ---------------------------------------------------------------------------

    /// Renders `instances` into a `width × height` region of `main_atlas`,
    /// leaving the rest of the atlas untouched — a snapshot of one entity's
    /// on-screen appearance (shape, border, baked text) that other entities can
    /// then UV-address via [`crate::QuadInstance::uv_offset`]/`uv_scale`.
    ///
    /// Use it for a lasting texture, such as a component baked with
    /// `.bake()`. For the short-lived images a split or merge's pieces fade
    /// between, use [`Self::bake_instances_to_transition_atlas`], whose
    /// regions are freed when the transition ends.
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
        placement: crate::MainAtlasPlacement,
    ) {
        let dest = self.main_atlas.clone();
        // No gutter here (`pad: 0`) — unlike `transition_allocator`,
        // `MainAtlasAllocator` packs regions with zero reserved margin, so
        // painting a transparent ring beyond the requested region would
        // overwrite a few edge pixels of whatever neighboring allocation
        // happens to be packed adjacent to it.
        self.bake_instances_to_atlas(
            device,
            queue,
            instances,
            view_projection,
            (placement.x, placement.y, placement.width, placement.height),
            &dest,
            placement.page,
            0,
        );
    }

    /// Renders `instances` into a `width × height` region of
    /// `transition_atlas` — the atlas the fragment shader's crossfade path
    /// reads `base_uv_offset`/`base_uv_scale` from (see `quad.wgsl`:
    /// `if in.crossfade_t > 0.0 { ... transition_atlas ... }`).
    ///
    /// Splits and merges use it so that each piece fades from a slice of the
    /// source's appearance: the source is baked once, each piece's
    /// `base_uv_offset` and `base_uv_scale` select its slice, and its
    /// `crossfade_t` follows its transition (see `proteus_ui::BakedTexture`).
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
        let dest = self.transition_atlas.clone();
        // `pad`: `TransitionAtlasAllocator` reserved a `TRANSITION_BAKE_PAD`
        // gutter around `region`, which this bake paints transparent (see
        // `bake_instances_to_atlas`). `dest_layer: 0`: the transition atlas has
        // one layer.
        self.bake_instances_to_atlas(
            device,
            queue,
            instances,
            view_projection,
            region,
            &dest,
            0,
            crate::transition_atlas::TRANSITION_BAKE_PAD,
        );
    }

    /// `dest_layer` is the page of `dest_texture` to write: the main atlas's page
    /// index, or 0 for the single-layer transition atlas.
    ///
    /// `pad` is a transparent gutter, in atlas pixels, painted around `region`.
    /// The allocator must already have reserved it, as
    /// `TransitionAtlasAllocator` does; otherwise it would overwrite the edge of
    /// a neighboring region. `MainAtlasAllocator` reserves none, so its callers
    /// pass 0.
    #[allow(clippy::too_many_arguments)]
    fn bake_instances_to_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[QuadInstance],
        view_projection: glam::Mat4,
        region: (u32, u32, u32, u32),
        dest_texture: &wgpu::Texture,
        dest_layer: u32,
        pad: u32,
    ) {
        let (x, y, w, h) = region;

        // Without a gutter, `region`'s edge pixels in the atlas would be
        // whatever the allocator's packing left there — for `pad > 0`
        // callers, a rounded corner's near-edge bilinear sample could then
        // blend with an adjacent allocation's opaque content instead of
        // reliably fading to transparent. The scratch texture is sized to
        // cover content + gutter, cleared transparent, and the draw is
        // confined to the centered `w×h` viewport so the gutter ring is
        // never touched by rasterization — then the *whole* padded scratch
        // (content + guaranteed-transparent ring) is copied into the atlas.
        let padded_w = w + 2 * pad;
        let padded_h = h + 2 * pad;

        let scratch = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bake_scratch"),
            size: wgpu::Extent3d {
                width: padded_w,
                height: padded_h,
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
        // relies on staying correct between bakes (see their field
        // comments).
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
                // `view_projection` maps the entity's geometry to fill NDC
                // ±1 for a `w×h` viewport — confining the viewport to the
                // centered `w×h` sub-rect (rather than the full padded
                // scratch) reuses that same projection unchanged while
                // leaving the `pad`-pixel ring around it untouched by
                // rasterization, so it stays at the clear color set above.
                pass.set_viewport(pad as f32, pad as f32, w as f32, h as f32, 0.0, 1.0);
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
                origin: wgpu::Origin3d {
                    x: x.saturating_sub(pad),
                    y: y.saturating_sub(pad),
                    z: dest_layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: padded_w,
                height: padded_h,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
    }

    // ---------------------------------------------------------------------------
    // Video
    // ---------------------------------------------------------------------------

    /// Creates the video texture at `width` × `height` and registers it.
    ///
    /// Upload frames to it with [`upload_video_frame`]. The host decodes the
    /// video (see `VideoStream`); this crate has no decoder.
    ///
    /// Calling it again replaces the texture. The new texture is cleared to
    /// black, since a new GPU texture's contents are undefined and would show
    /// until the first frame arrives.
    ///
    /// [`upload_video_frame`]: QuadPipeline::upload_video_frame
    pub fn init_video(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> TextureId {
        self.video_atlas = Self::create_video_texture(device, width, height);
        self.video_atlas_size = (width, height);
        let black_frame: Vec<u8> = std::iter::repeat_n([0u8, 0, 0, 255], (width * height) as usize)
            .flatten()
            .collect();
        self.upload_video_frame(queue, &black_frame);
        self.rebuild_atlas_bind_group(device);
        self.texture_registry.register_video(width, height)
    }

    /// Uploads one frame of RGBA pixels to the video texture. Call it once per
    /// frame while video plays, before [`draw`].
    ///
    /// `rgba` must be `width × height × 4` bytes, for the size given to the last
    /// [`init_video`]. A frame of the wrong size is dropped and logged, not
    /// passed to wgpu, which could reject it harshly: in a browser, by losing
    /// the GPU device. This is checked in release builds too, since the web
    /// package is built in release mode.
    ///
    /// [`init_video`]: QuadPipeline::init_video
    /// [`draw`]: QuadPipeline::draw
    pub fn upload_video_frame(&self, queue: &wgpu::Queue, rgba: &[u8]) {
        let (w, h) = self.video_atlas_size;
        let expected_len = (w * h * 4) as usize;
        if rgba.len() != expected_len {
            log::warn!(
                "upload_video_frame: expected {w}×{h}×4={expected_len} bytes, got {} — dropping frame",
                rgba.len(),
            );
            return;
        }
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

    /// Releases the video texture's GPU memory, for example when the app is
    /// in the background.
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

    /// Creates the video texture again after [`suspend_video`], and rebuilds
    /// the bind group. [`TextureRegistry::is_active`] is `true` again after
    /// this; upload frames straight away.
    ///
    /// Nothing calls it yet: it is for resuming when an app returns from the
    /// background.
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
    /// - Pass the viewport's size in logical pixels: one unit is one logical
    ///   pixel, and the surface's scale factor doesn't affect it.
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

    /// Builds a quad render pipeline targeting `color_format`. Used to create
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
                    // Premultiplied alpha: src + (1 - src_alpha) * dst. `fs_main`
                    // outputs premultiplied color, so this must match, or
                    // partly transparent fragments would blend wrongly.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            depth_stencil: None, // Z order comes from instance order
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// The `main_atlas` view used by every `atlas_bind_group` build. Explicit `D2Array` rather
    /// than `Default::default()`: wgpu only infers `D2Array` while the texture has more than one
    /// layer, so an inferred view would silently become plain `D2` — mismatching this bind
    /// group's `D2Array` layout entry — if `AtlasConfig::page_count` were ever configured to 1
    /// (e.g. for debugging). Being explicit keeps a one-page pool valid too.
    fn create_main_atlas_view(main_atlas: &wgpu::Texture) -> wgpu::TextureView {
        main_atlas.create_view(&wgpu::TextureViewDescriptor {
            label: Some("main_atlas_view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    }

    /// Creates a blank RGBA video texture of the given size.
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
            // Not sRGB, like the other atlases, so brightness doesn't jump when
            // fading between them.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Rebuilds the atlas bind group (group 1) after `video_atlas` has changed.
    ///
    /// This is called by [`init_video`], [`suspend_video`], and [`resume_video`]
    /// whenever the video texture is swapped for a different allocation.
    fn rebuild_atlas_bind_group(&mut self, device: &wgpu::Device) {
        let main_view = Self::create_main_atlas_view(&self.main_atlas);
        let transition_view = self.transition_atlas.create_view(&Default::default());
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

    /// Creates the main atlas, with `config.page_count` pages, and the
    /// transition atlas, and fills the white guard block at the origin of the
    /// main atlas's page 0.
    ///
    /// [`QuadPipeline::WHITE_PIXEL_UV_OFFSET`] and the default atlas page both
    /// point there, so it must never move. Components with no texture sample it,
    /// so their color alone decides how they look. The whole
    /// [`crate::main_atlas_allocator::WHITE_PIXEL_GUARD_SIZE`] block is filled,
    /// not one texel, since nothing else can be placed there anyway.
    fn create_atlases(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &AtlasConfig,
        transition_atlas_size: u32,
    ) -> (wgpu::Texture, wgpu::Texture) {
        let main_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("main_atlas"),
            size: wgpu::Extent3d {
                width: config.page_size,
                height: config.page_size,
                depth_or_array_layers: config.page_count.max(1),
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

        // Fill the reserved guard block at (0, 0) of layer 0 only — origin.z = 0
        // and depth_or_array_layers: 1 below already mean exactly that.
        // Components with no texture sample this via `WHITE_PIXEL_UV_OFFSET`.
        let guard = crate::main_atlas_allocator::WHITE_PIXEL_GUARD_SIZE;
        let guard_pixels = vec![255u8; (guard * guard * 4) as usize]; // RGBA white
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &main_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &guard_pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(guard * 4),
                rows_per_image: Some(guard),
            },
            wgpu::Extent3d {
                width: guard,
                height: guard,
                depth_or_array_layers: 1,
            },
        );

        let transition_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transition_atlas"),
            size: wgpu::Extent3d {
                width: transition_atlas_size,
                height: transition_atlas_size,
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
