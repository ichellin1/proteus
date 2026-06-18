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

// ---------------------------------------------------------------------------
// QuadPipeline
// ---------------------------------------------------------------------------

pub struct QuadPipeline {
    // Core render pipeline
    pipeline: wgpu::RenderPipeline,

    // Static base-quad geometry — uploaded once at init, never changed
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,

    // Instance buffer — overwritten every frame
    instance_buffer: wgpu::Buffer,
    instance_count: u32,
    max_instances: u32,

    // Frame-level uniforms (view/projection matrix)
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup, // bind group 0

    // Atlas textures — kept alive so the bind group views remain valid
    _main_atlas: wgpu::Texture,
    _transition_atlas: wgpu::Texture,
    atlas_bind_group: wgpu::BindGroup, // bind group 1
}

impl QuadPipeline {
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
            ],
        });

        // --- Pipeline layout ---
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad_pipeline_layout"),
            bind_group_layouts: &[&uniform_layout, &atlas_layout],
            push_constant_ranges: &[],
        });

        // --- Render pipeline ---
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[quad_vertex_layout(), QuadInstance::buffer_layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
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
            multiview: None,
            cache: None,
        });

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

        // --- Atlas textures ---
        let (main_atlas, transition_atlas) = Self::create_atlases(device, queue);

        let main_atlas_view = main_atlas.create_view(&Default::default());
        let transition_atlas_view = transition_atlas.create_view(&Default::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
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
            ],
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            instance_buffer,
            instance_count: 0,
            max_instances,
            uniform_buffer,
            uniform_bind_group,
            _main_atlas: main_atlas,
            _transition_atlas: transition_atlas,
            atlas_bind_group,
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
    // Projection helpers
    // ---------------------------------------------------------------------------

    /// Orthographic projection for a viewport of `width` × `height` pixels.
    ///
    /// - Origin at viewport center, Y-up, 1 unit = 1 pixel.
    /// - Depth range: Z 0 → 1000 maps to NDC 0 → 1 (wgpu convention).
    /// - DPI scaling should be applied by the caller: pass physical pixels, not logical ones.
    pub fn ortho(width: f32, height: f32) -> glam::Mat4 {
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
            glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    // ---------------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------------

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
            wgpu::ImageCopyTexture {
                texture: &main_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255], // RGBA white
            wgpu::ImageDataLayout {
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
