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
/// Size: 124 bytes. 1000 components ≈ 124 KB — well within GPU limits.
///
/// Field byte offsets must match `buffer_layout()` and `@location` attributes in `quad.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct QuadInstance {
    // --- Transform ---
    /// World-space position (x, y, z). Z controls depth ordering; higher = on top.
    pub position: [f32; 3], // offset   0, size 12
    /// Component size in pixels (width, height).
    pub size: [f32; 2], // offset  12, size  8
    /// Rotation in radians. Converted from degrees at the WASM boundary.
    pub rotation: f32, // offset  20, size  4
    /// Uniform scale multiplier.
    pub scale: f32, // offset  24, size  4
    /// Transform anchor point, normalized 0.0–1.0. Default: [0.5, 0.5] (center).
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
} // total       124 bytes

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
    ///
    /// Total: 11 instance attributes + 2 vertex attributes = 13. Limit is 16.
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
            ],
        }
    }
}

// Compile-time size guard. If QuadInstance changes, this fails immediately
// and forces the developer to audit buffer_layout() offsets.
const _QUAD_INSTANCE_SIZE: () = assert!(std::mem::size_of::<QuadInstance>() == 124);
