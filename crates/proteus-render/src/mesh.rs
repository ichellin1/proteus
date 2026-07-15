//! Base quad geometry and per-instance data for the instanced render pipeline.
//!
//! Every visible component in Proteus is rendered as a `QuadInstance` — a 124-byte
//! struct packed into the instance buffer. One buffer upload + one draw call per frame
//! renders the entire scene regardless of component count.

use bytemuck::{Pod, Zeroable};

// ---------------------------------------------------------------------------
// Base quad geometry — static, uploaded once at startup
// ---------------------------------------------------------------------------

/// One vertex of the base unit quad.
/// The vertex shader scales and positions each instance from here.
///
/// ```text
///  (-0.5, 0.5) ---- (0.5, 0.5)
///       |                |
///  (-0.5,-0.5) ---- (0.5,-0.5)
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct QuadVertex {
    /// Object-space position (x, y). Z is always 0 — depth comes from the instance.
    pub position: [f32; 2],
    /// UV coordinates for this corner of the unit quad.
    pub uv: [f32; 2],
}

/// The four vertices of the unit quad.
pub const QUAD_VERTICES: [QuadVertex; 4] = [
    QuadVertex {
        position: [-0.5, -0.5],
        uv: [0.0, 1.0],
    }, // bottom-left
    QuadVertex {
        position: [0.5, -0.5],
        uv: [1.0, 1.0],
    }, // bottom-right
    QuadVertex {
        position: [0.5, 0.5],
        uv: [1.0, 0.0],
    }, // top-right
    QuadVertex {
        position: [-0.5, 0.5],
        uv: [0.0, 0.0],
    }, // top-left
];

/// Two triangles forming the quad (counter-clockwise winding).
pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// Returns the wgpu vertex buffer layout for `QuadVertex` (the base quad, step per vertex).
pub fn quad_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    use std::mem;
    wgpu::VertexBufferLayout {
        array_stride: mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Per-instance data — one entry per visible component, packed each frame
// ---------------------------------------------------------------------------

/// Per-instance GPU data for one component quad. Packed into the instance buffer
/// each frame by the render system; uploaded in a single transfer.
///
/// Size: 156 bytes. 1000 components ≈ 156 KB — well within GPU limits.
///
/// Field byte offsets must match `buffer_layout()` and `@location` attributes in `quad.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct QuadInstance {
    // --- Transform ---
    /// World-space position (x, y, z). Z is reserved for future depth sorting;
    /// draw order currently determines stacking (last in instance buffer = on top).
    pub position: [f32; 3], // offset   0, size 12
    /// Component size in pixels (width, height).
    pub size: [f32; 2], // offset  12, size  8
    /// Rotation in radians. Converted from degrees at the WASM boundary.
    pub rotation: f32, // offset  20, size  4
    /// Uniform scale multiplier.
    pub scale: f32, // offset  24, size  4
    /// Transform anchor point, normalized 0.0–1.0, Y-down screen convention.
    /// [0, 0] = top-left, [0.5, 0.5] = center (default), [1, 1] = bottom-right.
    pub anchor: [f32; 2], // offset  28, size  8

    // --- Visual ---
    /// RGBA color tint (0.0–1.0). Alpha affects color tint independently of `opacity`.
    pub color: [f32; 4], // offset  36, size 16
    /// Whole-component opacity multiplier (0.0–1.0). Applied on top of `color.a`.
    pub opacity: f32, // offset  52, size  4
    /// Corner radius in pixels, evaluated as an SDF. 0.0 = sharp corners.
    pub corner_radius: f32, // offset  56, size  4

    // --- Atlas UV (current/target state) ---
    /// Sub-region origin within `main_atlas` or `transition_atlas`.
    pub uv_offset: [f32; 2], // offset  60, size  8
    /// Sub-region size within the active atlas.
    pub uv_scale: [f32; 2], // offset  68, size  8
    /// Which atlas this instance samples. 0 = main_atlas, 1 = transition_atlas.
    pub atlas_page: u32, // offset  76, size  4

    // --- Crossfade (from-state during a baked transition) ---
    /// From-state sub-region origin within `transition_atlas`.
    pub base_uv_offset: [f32; 2], // offset  80, size  8
    /// From-state sub-region size within `transition_atlas`.
    pub base_uv_scale: [f32; 2], // offset  88, size  8
    /// Blend factor: 0.0 = fully from-state, 1.0 = fully to-state. 0.0 disables crossfade.
    pub crossfade_t: f32, // offset  96, size  4

    // --- Border ---
    /// Border width in pixels. 0.0 = no border (zero-cost in shader).
    pub border_width: f32, // offset 100, size  4
    /// Border color RGBA.
    pub border_color: [f32; 4], // offset 104, size 16
    /// Border placement: -1.0 = inner, 0.0 = center, 1.0 = outer.
    pub border_offset: f32, // offset 120, size  4

    // --- Drop shadow (M8) ---
    /// Shadow params: [offset_x, offset_y, softness, spread] in world-space pixels.
    /// `shadow_color.a == 0` disables the shadow (zero-cost in shader).
    pub shadow_params: [f32; 4], // offset 124, size 16
    /// Shadow color RGBA.  Alpha = 0 means no shadow.
    pub shadow_color: [f32; 4], // offset 140, size 16
} // total       156 bytes

impl QuadInstance {
    /// Returns the wgpu vertex buffer layout for the instance buffer (step per instance).
    ///
    /// Shader locations start at 2 (0 and 1 are used by `QuadVertex`).
    ///
    /// Metal and most wgpu backends cap total vertex attributes at 16. We pack
    /// adjacent scalar fields into wider formats to stay under that limit:
    ///
    /// | Loc | Format     | Byte range | Fields packed                              |
    /// |-----|------------|------------|--------------------------------------------|
    /// |   2 | Float32x3  |   0 –  12  | position                                   |
    /// |   3 | Float32x4  |  12 –  28  | size.xy, rotation, scale                   |
    /// |   4 | Float32x2  |  28 –  36  | anchor                                     |
    /// |   5 | Float32x4  |  36 –  52  | color                                      |
    /// |   6 | Float32x2  |  52 –  60  | opacity, corner_radius                     |
    /// |   7 | Float32x4  |  60 –  76  | uv_offset.xy, uv_scale.xy                  |
    /// |   8 | Uint32     |  76 –  80  | atlas_page                                 |
    /// |   9 | Float32x4  |  80 –  96  | base_uv_offset.xy, base_uv_scale.xy        |
    /// |  10 | Float32x2  |  96 – 104  | crossfade_t, border_width                  |
    /// |  11 | Float32x4  | 104 – 120  | border_color                               |
    /// |  12 | Float32    | 120 – 124  | border_offset                              |
    /// |  13 | Float32x4  | 124 – 140  | shadow_params (offset_x, offset_y, softness, spread) |
    /// |  14 | Float32x4  | 140 – 156  | shadow_color                               |
    ///
    /// Total: 13 instance attributes + 2 vertex attributes = 15. Limit is 16.
    ///
    /// **Vertex attribute budget:** 15 of 16 locations are in use (1 slot remaining).
    /// Adding any new per-instance field would exceed the Metal limit and require
    /// packing two existing fields into a single attribute location.  Consider
    /// this before adding to `QuadInstance` or `QuadVertex`.
    pub fn buffer_layout() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<QuadInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // loc 2: position (xyz)
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // loc 3: size.xy + rotation + scale  (packed — contiguous bytes 12–28)
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 4: anchor (xy)
                wgpu::VertexAttribute {
                    offset: 28,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // loc 5: color (rgba)
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 6: opacity + corner_radius  (packed — contiguous bytes 52–60)
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // loc 7: uv_offset.xy + uv_scale.xy  (packed — contiguous bytes 60–76)
                wgpu::VertexAttribute {
                    offset: 60,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 8: atlas_page (u32)
                wgpu::VertexAttribute {
                    offset: 76,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Uint32,
                },
                // loc 9: base_uv_offset.xy + base_uv_scale.xy  (packed — bytes 80–96)
                wgpu::VertexAttribute {
                    offset: 80,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 10: crossfade_t + border_width  (packed — contiguous bytes 96–104)
                wgpu::VertexAttribute {
                    offset: 96,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // loc 11: border_color (rgba)
                wgpu::VertexAttribute {
                    offset: 104,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 12: border_offset
                wgpu::VertexAttribute {
                    offset: 120,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32,
                },
                // loc 13: shadow_params (offset_x, offset_y, softness, spread)
                wgpu::VertexAttribute {
                    offset: 124,
                    shader_location: 13,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 14: shadow_color (rgba)
                wgpu::VertexAttribute {
                    offset: 140,
                    shader_location: 14,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

// Compile-time size guard. If QuadInstance changes, this fails immediately
// and forces the developer to audit buffer_layout() offsets.
const _QUAD_INSTANCE_SIZE: () = assert!(std::mem::size_of::<QuadInstance>() == 156);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    // -----------------------------------------------------------------------
    // Layout tests — every field offset must match buffer_layout() exactly.
    // If a field is reordered the GPU reads garbage; these catch it immediately.
    // -----------------------------------------------------------------------

    #[test]
    fn quad_instance_field_offsets() {
        assert_eq!(offset_of!(QuadInstance, position), 0);
        assert_eq!(offset_of!(QuadInstance, size), 12);
        assert_eq!(offset_of!(QuadInstance, rotation), 20);
        assert_eq!(offset_of!(QuadInstance, scale), 24);
        assert_eq!(offset_of!(QuadInstance, anchor), 28);
        assert_eq!(offset_of!(QuadInstance, color), 36);
        assert_eq!(offset_of!(QuadInstance, opacity), 52);
        assert_eq!(offset_of!(QuadInstance, corner_radius), 56);
        assert_eq!(offset_of!(QuadInstance, uv_offset), 60);
        assert_eq!(offset_of!(QuadInstance, uv_scale), 68);
        assert_eq!(offset_of!(QuadInstance, atlas_page), 76);
        assert_eq!(offset_of!(QuadInstance, base_uv_offset), 80);
        assert_eq!(offset_of!(QuadInstance, base_uv_scale), 88);
        assert_eq!(offset_of!(QuadInstance, crossfade_t), 96);
        assert_eq!(offset_of!(QuadInstance, border_width), 100);
        assert_eq!(offset_of!(QuadInstance, border_color), 104);
        assert_eq!(offset_of!(QuadInstance, border_offset), 120);
        assert_eq!(offset_of!(QuadInstance, shadow_params), 124);
        assert_eq!(offset_of!(QuadInstance, shadow_color), 140);
    }

    #[test]
    fn quad_instance_total_size() {
        assert_eq!(std::mem::size_of::<QuadInstance>(), 156);
    }

    // -----------------------------------------------------------------------
    // Transform math tests — mirrors the WGSL vertex shader logic in Rust so
    // we can verify correctness without spinning up a GPU.
    //
    // Shader reference (quad.wgsl vs_main):
    //   scaled_size  = inst_size * inst_scale
    //   centered     = vertex_pos * scaled_size        ← unit quad vertex
    //   anchor_shift = (anchor - 0.5) * scaled_size * [1,-1]  ← Y-down→Y-up flip
    //   pivoted      = centered - anchor_shift
    //   rotated      = rotate(pivoted, rotation)
    //   world        = rotated + inst_position.xy
    //   clip         = view_projection * vec4(world, inst_position.z, 1)
    // -----------------------------------------------------------------------

    /// Replicates the vertex shader transform for one vertex.
    fn transform_vertex(
        vertex_pos: [f32; 2],    // base quad corner e.g. [-0.5, -0.5]
        inst_position: [f32; 3], // world position (x, y, z)
        inst_size: [f32; 2],     // component size in pixels
        rotation: f32,           // radians
        scale: f32,
        anchor: [f32; 2], // 0.0–1.0
        view_projection: glam::Mat4,
    ) -> glam::Vec4 {
        let vp = glam::Vec2::from(vertex_pos);
        let sz = glam::Vec2::from(inst_size);
        let an = glam::Vec2::from(anchor);

        let scaled_size = sz * scale;
        let centered = vp * scaled_size;
        // Mirror the Y-down→Y-up flip from the shader: negate Y so [0,0]=top-left.
        let anchor_shift = (an - glam::Vec2::splat(0.5)) * scaled_size * glam::Vec2::new(1.0, -1.0);
        let pivoted = centered - anchor_shift;

        let (sin_r, cos_r) = rotation.sin_cos();
        let rotated = glam::Vec2::new(
            pivoted.x * cos_r - pivoted.y * sin_r,
            pivoted.x * sin_r + pivoted.y * cos_r,
        );

        let world = glam::Vec4::new(
            rotated.x + inst_position[0],
            rotated.y + inst_position[1],
            inst_position[2],
            1.0,
        );
        view_projection * world
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    /// Center anchor, no rotation, scale=1 — simplest case.
    /// vertex [-0.5,-0.5] of a 200×100 quad at origin should land at world (-100,-50).
    #[test]
    fn transform_center_anchor_no_rotation() {
        let clip = transform_vertex(
            [-0.5, -0.5],
            [0.0, 0.0, 0.5],
            [200.0, 100.0],
            0.0,
            1.0,
            [0.5, 0.5],
            glam::Mat4::IDENTITY,
        );
        assert!(approx_eq(clip.x, -100.0), "x: {}", clip.x);
        assert!(approx_eq(clip.y, -50.0), "y: {}", clip.y);
        assert!(approx_eq(clip.z, 0.5), "z: {}", clip.z);
        assert!(approx_eq(clip.w, 1.0), "w: {}", clip.w);
    }

    /// Top-left anchor [0,0] (Y-down screen convention): the component's top-left
    /// corner becomes the pivot. In Y-up world space the top-left vertex is [-0.5,+0.5].
    ///
    /// With the Y-flip:
    ///   anchor_shift = ([0,0] - 0.5) * [200,100] * [1,-1] = [-100, +50]
    ///   centered for [-0.5,+0.5] = (-100, +50)
    ///   pivoted = (-100,+50) - (-100,+50) = (0, 0)  ✓
    #[test]
    fn transform_top_left_anchor() {
        let clip = transform_vertex(
            [-0.5, 0.5], // top-left vertex in Y-up world = visually top-left on screen
            [0.0, 0.0, 0.0],
            [200.0, 100.0],
            0.0,
            1.0,
            [0.0, 0.0], // anchor [0,0] = top-left
            glam::Mat4::IDENTITY,
        );
        assert!(approx_eq(clip.x, 0.0), "x: {}", clip.x);
        assert!(approx_eq(clip.y, 0.0), "y: {}", clip.y);
    }

    /// Scale=2 doubles the effective size. A 100×50 quad at scale=2 behaves
    /// identically to a 200×100 quad at scale=1.
    #[test]
    fn transform_scale() {
        let scale1 = transform_vertex(
            [-0.5, -0.5],
            [0.0, 0.0, 0.0],
            [200.0, 100.0],
            0.0,
            1.0,
            [0.5, 0.5],
            glam::Mat4::IDENTITY,
        );
        let scale2 = transform_vertex(
            [-0.5, -0.5],
            [0.0, 0.0, 0.0],
            [100.0, 50.0],
            0.0,
            2.0,
            [0.5, 0.5],
            glam::Mat4::IDENTITY,
        );
        assert!(
            approx_eq(scale1.x, scale2.x),
            "x: {} vs {}",
            scale1.x,
            scale2.x
        );
        assert!(
            approx_eq(scale1.y, scale2.y),
            "y: {} vs {}",
            scale1.y,
            scale2.y
        );
    }

    /// 90° CCW rotation: a point at (1, 0) should become (0, 1).
    /// Use a 2×2 quad at center anchor so vertex (0.5, -0.5) → world (1, -1) pre-rotation,
    /// then after 90° CCW it should be (1, 1).
    #[test]
    fn transform_rotation_90_deg() {
        let clip = transform_vertex(
            [0.5, -0.5],
            [0.0, 0.0, 0.0],
            [2.0, 2.0],
            std::f32::consts::FRAC_PI_2, // 90° CCW
            1.0,
            [0.5, 0.5],
            glam::Mat4::IDENTITY,
        );
        // centered = [0.5,-0.5]*[2,2] = [1,-1]; anchor_shift=0 (center)
        // rotated: x = 1*cos90 - (-1)*sin90 = 0 + 1 = 1
        //          y = 1*sin90 + (-1)*cos90 = 1 - 0 = 1
        assert!(approx_eq(clip.x, 1.0), "x: {}", clip.x);
        assert!(approx_eq(clip.y, 1.0), "y: {}", clip.y);
    }

    /// Orthographic projection: a world point at pixel (640, 400) on a 1280×800
    /// viewport should map to NDC (1, 1) — the top-right corner.
    #[test]
    fn transform_ortho_projection() {
        use crate::pipeline::QuadPipeline;
        let vp = QuadPipeline::ortho(1280.0, 800.0);
        let clip = transform_vertex(
            [0.5, 0.5],
            [0.0, 0.0, 0.0],
            [1280.0, 800.0],
            0.0,
            1.0,
            [0.5, 0.5],
            vp,
        );
        // vertex [0.5,0.5] * [1280,800] = [640,400]; anchor_shift=0 (center)
        // world = [640,400]; ortho maps [640,400] → NDC [1,1]
        assert!(approx_eq(clip.x, 1.0), "x: {}", clip.x);
        assert!(approx_eq(clip.y, 1.0), "y: {}", clip.y);
    }
}
