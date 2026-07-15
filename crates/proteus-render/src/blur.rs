//! `BlurPipeline` — separable Gaussian blur for M8.5.
//!
//! ## Pipeline overview
//!
//! A `BlurPipeline` is created once (alongside `QuadPipeline`). Each frame, for
//! every entity that has a [`proteus_ui::Blur`] component the shell does three
//! things:
//!
//! 1. **CPU bake**: call [`BlurPipeline::bake_solid_color`] to upload the entity's
//!    flat RGBA color to `bake_src` (a 512×512 `Rgba8Unorm` texture).
//! 2. **Write uniforms**: call [`BlurPipeline::write_uniforms`] with the entity's
//!    blur radius before recording encoder commands.
//! 3. **Record blur passes**: call [`BlurPipeline::apply_passes`] inside the frame
//!    encoder — this records an H-blur render pass (bake_src → bake_h) followed
//!    by a V-blur render pass (bake_h → blur_atlas).
//!
//! The blur_atlas texture is owned by `QuadPipeline` (bind group 1, binding 3).
//! `QuadPipeline::blur_atlas_view()` exposes the render-attachment view the
//! V-blur pass writes into. Reading it from the bind group in the subsequent main
//! render pass is safe because wgpu's implicit barriers order pass outputs before
//! later passes in the same command encoder.
//!
//! ## Limitations (M8.5)
//!
//! - One blurred entity at a time — the blur atlas is a fixed 512×512 texture
//!   with no sub-region packing. A second Blur entity overwrites the atlas.
//! - Solid-color quads only — text (`BakedText`) is NOT included in the blur bake.
//! - The entity's QuadState corner_radius is not applied in the bake; the blurred
//!   result is a soft rectangle (not a soft rounded rect). Full shape support will
//!   land in M9.

use bytemuck::{Pod, Zeroable};

use crate::pipeline::BLUR_ATLAS_SIZE;

// ---------------------------------------------------------------------------
// Uniform struct — identical layout to the WGSL `Uniforms` in blur.wgsl
// ---------------------------------------------------------------------------

/// Uniform data for one blur pass. `direction` selects horizontal or vertical.
///
/// std140 rule: `vec2<f32>` is 8 bytes, `f32` is 4 bytes. The struct must be
/// padded to a multiple of 16 bytes for WGSL — hence the explicit `_pad` field.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct BlurUniforms {
    /// [1.0, 0.0] for the horizontal pass; [0.0, 1.0] for the vertical pass.
    direction: [f32; 2],
    /// Gaussian blur radius in pixels (sigma = radius / 3.0).
    radius: f32,
    /// Padding to reach 16 bytes (std140 alignment).
    _pad: f32,
}

// ---------------------------------------------------------------------------
// BlurPipeline
// ---------------------------------------------------------------------------

/// Separable Gaussian blur pipeline (M8.5).
///
/// See the module docs for the three-step per-frame API:
/// [`bake_solid_color`][BlurPipeline::bake_solid_color] →
/// [`write_uniforms`][BlurPipeline::write_uniforms] →
/// [`apply_passes`][BlurPipeline::apply_passes].
pub struct BlurPipeline {
    /// The compiled render pipeline (shared by both passes; direction and radius
    /// are selected per-pass via the uniform buffer).
    pipeline: wgpu::RenderPipeline,

    // ── Intermediate textures ─────────────────────────────────────────────────

    /// Source texture: the shell uploads solid-color pixels here each frame.
    /// Usage: COPY_DST | TEXTURE_BINDING (CPU write → H-blur samples from it).
    bake_src: wgpu::Texture,

    /// Intermediate texture: H-blur writes here; V-blur reads from it.
    /// Usage: RENDER_ATTACHMENT | TEXTURE_BINDING.
    _bake_h: wgpu::Texture,

    /// Render-attachment view of `bake_h` — used as the color attachment for
    /// the H-blur render pass.
    bake_h_view: wgpu::TextureView,

    // ── Per-direction uniform buffers ─────────────────────────────────────────

    /// H-pass uniform: direction = [1, 0]. `radius` is written each frame via
    /// [`write_uniforms`][BlurPipeline::write_uniforms].
    h_uniform: wgpu::Buffer,
    /// V-pass uniform: direction = [0, 1]. Same as above.
    v_uniform: wgpu::Buffer,

    // ── Pre-created bind groups ───────────────────────────────────────────────

    /// H-pass bind group: {h_uniform, bake_src_view, sampler}.
    h_bind_group: wgpu::BindGroup,
    /// V-pass bind group: {v_uniform, bake_h_sampling_view, sampler}.
    v_bind_group: wgpu::BindGroup,
}

impl BlurPipeline {
    /// Create the blur pipeline and all associated GPU resources.
    pub fn new(device: &wgpu::Device) -> Self {
        // --- Shader ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::BLUR_SHADER_SRC.into()),
        });

        // --- Bind group layout ---
        // binding 0: Uniforms (direction + radius)
        // binding 1: src texture
        // binding 2: sampler
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
            label: Some("blur_pipeline_layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        // --- Render pipeline ---
        // Uses the full-screen-triangle trick (no VBO, 3 vertices from vertex_index).
        // Output format must match the target texture format (Rgba8Unorm).
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[], // no vertex buffers — hard-coded triangle in shader
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None, // simple write — no blending needed for blur passes
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Sampler (linear, clamp-to-edge — same settings as atlas_sampler) ---
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // --- Intermediate textures ---
        let bake_src = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur_bake_src"),
            size: wgpu::Extent3d {
                width: BLUR_ATLAS_SIZE,
                height: BLUR_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // COPY_DST so the CPU can write pixels via queue.write_texture.
            // TEXTURE_BINDING so the H-blur pass can sample from it.
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let bake_h = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur_bake_h"),
            size: wgpu::Extent3d {
                width: BLUR_ATLAS_SIZE,
                height: BLUR_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            // RENDER_ATTACHMENT so the H-blur pass can write to it.
            // TEXTURE_BINDING so the V-blur pass can sample from it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        // Two views of bake_h: one for the H-pass color attachment, one for V-pass sampling.
        let bake_h_view = bake_h.create_view(&Default::default());
        let bake_h_sampling_view = bake_h.create_view(&Default::default());

        let bake_src_view = bake_src.create_view(&Default::default());

        // --- Uniform buffers (direction baked in at init; radius updated per-frame) ---
        let h_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_h_uniform"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let v_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("blur_v_uniform"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Pre-created bind groups ---
        let h_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur_h_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: h_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&bake_src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let v_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur_v_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: v_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&bake_h_sampling_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            pipeline,
            bake_src,
            _bake_h: bake_h,
            bake_h_view,
            h_uniform,
            v_uniform,
            h_bind_group,
            v_bind_group,
        }
    }

    // ---------------------------------------------------------------------------
    // Per-frame API
    // ---------------------------------------------------------------------------

    /// Upload a solid RGBA color to the top-left `w × h` region of `bake_src`.
    ///
    /// Call this before [`apply_passes`][Self::apply_passes]. `color` is a
    /// premultiplied RGBA byte tuple; for a component with `QuadState::color`
    /// `[r, g, b, a]` (0.0–1.0), convert with `(c * 255.0) as u8`.
    ///
    /// The rest of the 512×512 texture retains its previous contents, but since
    /// the blur passes only sample within `[0, w/512] × [0, h/512]` UV space and
    /// the Gaussian kernel is clamped at texture edges, this is correct.
    pub fn bake_solid_color(
        &self,
        queue: &wgpu::Queue,
        color: [u8; 4],
        w: u32,
        h: u32,
    ) {
        let w = w.max(1).min(BLUR_ATLAS_SIZE);
        let h = h.max(1).min(BLUR_ATLAS_SIZE);
        // Allocate a flat pixel buffer filled with the uniform color.
        let pixel_count = (w * h) as usize;
        let mut pixels = vec![0u8; pixel_count * 4];
        for i in 0..pixel_count {
            pixels[i * 4]     = color[0];
            pixels[i * 4 + 1] = color[1];
            pixels[i * 4 + 2] = color[2];
            pixels[i * 4 + 3] = color[3];
        }
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.bake_src,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::ImageDataLayout {
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

    /// Write the blur radius to both H and V uniform buffers.
    ///
    /// The direction is fixed per-buffer (H = [1,0], V = [0,1]) and is set here
    /// alongside `radius`. Call this before [`apply_passes`][Self::apply_passes].
    pub fn write_uniforms(&self, queue: &wgpu::Queue, radius: f32) {
        let r = radius.max(0.5); // avoid degenerate Gaussian
        queue.write_buffer(
            &self.h_uniform,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                direction: [1.0, 0.0],
                radius: r,
                _pad: 0.0,
            }),
        );
        queue.write_buffer(
            &self.v_uniform,
            0,
            bytemuck::bytes_of(&BlurUniforms {
                direction: [0.0, 1.0],
                radius: r,
                _pad: 0.0,
            }),
        );
    }

    /// Record two blur render passes into `encoder`:
    ///
    /// 1. **H pass**: samples `bake_src`, writes to `bake_h`.
    /// 2. **V pass**: samples `bake_h`, writes to `dest_view` (the blur_atlas).
    ///
    /// `dest_view` should be `QuadPipeline::blur_atlas_view()` — the write is
    /// visible to the main render pass because later passes in the same encoder
    /// see stores from earlier passes after an implicit barrier.
    pub fn apply_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        dest_view: &wgpu::TextureView,
    ) {
        // H pass ── bake_src → bake_h
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur_h_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.bake_h_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.h_bind_group, &[]);
            pass.draw(0..3, 0..1); // full-screen triangle
        }

        // V pass ── bake_h → blur_atlas (dest_view)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur_v_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.v_bind_group, &[]);
            pass.draw(0..3, 0..1); // full-screen triangle
        }
    }

    // ---------------------------------------------------------------------------
    // UV helpers
    // ---------------------------------------------------------------------------

    /// Compute the `(uv_offset, uv_scale)` pair to store in `BakedBlur` for an
    /// entity whose pixel size is `(w, h)`.
    ///
    /// The blur occupies the top-left `w × h` region of the 512×512 blur atlas.
    /// UV origin is `[0, 0]` and scale maps exactly to that region.
    pub fn uv_for_size(w: u32, h: u32) -> ([f32; 2], [f32; 2]) {
        let s = BLUR_ATLAS_SIZE as f32;
        let w = w.max(1).min(BLUR_ATLAS_SIZE) as f32;
        let h = h.max(1).min(BLUR_ATLAS_SIZE) as f32;
        ([0.0, 0.0], [w / s, h / s])
    }
}
