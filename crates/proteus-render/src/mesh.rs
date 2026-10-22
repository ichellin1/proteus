//! The base quad's geometry and the per-instance data for the instanced render
//! pipeline.
//!
//! Every visible component is drawn as a [`QuadInstance`], a 160-byte struct in
//! the instance buffer. One buffer upload and one draw call per frame draw
//! everything, however many components there are.

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
    /// Object-space position (x, y).
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

/// The GPU data for one component's quad, packed into the instance buffer
/// each frame and uploaded in one transfer.
///
/// Size: 164 bytes, so 1000 components take about 164 KB.
///
/// The field offsets must match `buffer_layout()` and the `@location`
/// attributes in the shader's `vertex.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct QuadInstance {
    // --- Transform ---
    /// Position in world units. Instances are drawn in buffer order, so the
    /// last is on top; `collect_instances` orders them, using `z`. The shader
    /// doesn't read `z`.
    pub position: [f32; 3], // offset   0, size 12
    /// Component size in pixels (width, height).
    pub size: [f32; 2], // offset  12, size  8
    /// Rotation in radians.
    pub rotation: f32, // offset  20, size  4
    /// Uniform scale multiplier.
    pub scale: f32, // offset  24, size  4
    /// Transform anchor point, normalized 0.0–1.0, Y-down screen convention.
    /// [0, 0] = top-left, [0.5, 0.5] = center (default), [1, 1] = bottom-right.
    pub anchor: [f32; 2], // offset  28, size  8

    // --- Visual ---
    /// RGBA color tint (0.0–1.0). Alpha affects color tint independently of `opacity`.
    pub color: [f32; 4], // offset  36, size 16
    /// Each corner's radius in pixels, top-left, top-right, bottom-right,
    /// bottom-left, as 16-bit floats. Set it with [`Self::set_corner_radii`].
    /// The shader draws the corners with a signed distance function, so they
    /// stay smooth at any size. 0 is a sharp corner.
    ///
    /// Half floats, so that four fit where one 32-bit radius did: every vertex
    /// attribute location is in use (see [`QuadInstance::buffer_layout`]).
    /// They hold whole pixels exactly up to 2048, and to within a pixel up to
    /// 4096, far beyond any corner.
    pub corner_radii: [u16; 4], // offset  52, size  8

    // --- Atlas UV (current/target state) ---
    /// Sub-region origin within `main_atlas` or `transition_atlas`.
    pub uv_offset: [f32; 2], // offset  60, size  8
    /// Sub-region size within the active atlas.
    pub uv_scale: [f32; 2], // offset  68, size  8
    /// Which atlas to sample, and for the main atlas, which page. Bits 0–7
    /// select the atlas (`ATLAS_SELECTOR_MAIN` = 0, `ATLAS_SELECTOR_TRANSITION`
    /// = 1, `ATLAS_SELECTOR_VIDEO` = 2); bits 8–31 are the main-atlas page,
    /// since the other atlases have one page. Build it with [`pack_atlas_page`]
    /// and read it with [`unpack_atlas_page`]; the shader's `bindings.wgsl` uses the same
    /// layout.
    ///
    /// Packed into one field because every vertex attribute location is in use
    /// (see [`QuadInstance::buffer_layout`]). Page 0 packs to the selector
    /// alone: `pack_atlas_page(selector, 0) == selector`.
    pub atlas_page: u32, // offset  76, size  4

    // --- Crossfade (from-state during a baked transition) ---
    /// Texture coordinates of the start image's top-left corner, in the atlas
    /// [`Self::base_atlas_page`] selects: usually `transition_atlas`.
    pub base_uv_offset: [f32; 2], // offset  80, size  8
    /// Size of the start image, in texture coordinates of that atlas.
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
    /// Whole-component opacity multiplier (0.0–1.0). Applied on top of `color.a`.
    /// Next to `border_offset` so the two share an attribute location.
    pub opacity: f32, // offset 124, size  4

    // --- Drop shadow ---
    /// Shadow params: [offset_x, offset_y, softness, spread] in world-space pixels.
    /// `shadow_color.a == 0` disables the shadow (zero-cost in shader).
    pub shadow_params: [f32; 4], // offset 128, size 16
    /// Shadow color RGBA.  Alpha = 0 means no shadow.
    pub shadow_color: [f32; 4], // offset 144, size 16

    // --- Crossfade start atlas ---
    /// Which atlas `base_uv_offset` and `base_uv_scale` sample during a
    /// crossfade, packed like [`Self::atlas_page`]. `ATLAS_SELECTOR_TRANSITION`
    /// by default. It lets the start of a crossfade be in a different atlas from
    /// the end: for example, an image in the main atlas fading into playing
    /// video, which keeps playing rather than being frozen into a bake.
    pub base_atlas_page: u32, // offset 160, size 4
} // total       164 bytes

impl QuadInstance {
    /// Sets every corner's radius, in pixels: top-left, top-right,
    /// bottom-right, bottom-left.
    pub fn set_corner_radii(&mut self, radii: [f32; 4]) {
        self.corner_radii = Self::pack_corner_radii(radii);
    }

    /// Corner radii in pixels, top-left, top-right, bottom-right, bottom-left,
    /// packed for [`Self::corner_radii`](field@Self::corner_radii).
    pub fn pack_corner_radii(radii: [f32; 4]) -> [u16; 4] {
        radii.map(|r| half::f16::from_f32(r).to_bits())
    }

    /// Sets all four corners to the same radius, in pixels.
    pub fn set_corner_radius(&mut self, radius: f32) {
        self.set_corner_radii([radius; 4]);
    }

    /// Each corner's radius in pixels, as [`Self::set_corner_radii`] takes
    /// them, after rounding to 16 bits.
    pub fn corner_radii(&self) -> [f32; 4] {
        self.corner_radii
            .map(|bits| half::f16::from_bits(bits).to_f32())
    }

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
    /// |   6 | Float16x4  |  52 –  60  | corner_radii (tl, tr, br, bl)              |
    /// |   7 | Float32x4  |  60 –  76  | uv_offset.xy, uv_scale.xy                  |
    /// |   8 | Uint32     |  76 –  80  | atlas_page                                 |
    /// |   9 | Float32x4  |  80 –  96  | base_uv_offset.xy, base_uv_scale.xy        |
    /// |  10 | Float32x2  |  96 – 104  | crossfade_t, border_width                  |
    /// |  11 | Float32x4  | 104 – 120  | border_color                               |
    /// |  12 | Float32x2  | 120 – 128  | border_offset, opacity                     |
    /// |  13 | Float32x4  | 128 – 144  | shadow_params (offset_x, offset_y, softness, spread) |
    /// |  14 | Float32x4  | 144 – 160  | shadow_color                               |
    /// |  15 | Uint32     | 160 – 164  | base_atlas_page                            |
    ///
    /// Total: 14 instance attributes + 2 vertex attributes = 16. Limit is 16.
    ///
    /// **Vertex attribute budget: exhausted — 16 of 16 locations in use.** Any
    /// further field requires packing two existing fields into a single
    /// attribute location (as already done throughout this layout) rather
    /// than adding a new location outright.
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
                // loc 6: corner_radii, four half floats (bytes 52–60)
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float16x4,
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
                // loc 12: border_offset + opacity  (packed — contiguous bytes 120–128)
                wgpu::VertexAttribute {
                    offset: 120,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // loc 13: shadow_params (offset_x, offset_y, softness, spread)
                wgpu::VertexAttribute {
                    offset: 128,
                    shader_location: 13,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 14: shadow_color (rgba)
                wgpu::VertexAttribute {
                    offset: 144,
                    shader_location: 14,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // loc 15: base_atlas_page (u32)
                wgpu::VertexAttribute {
                    offset: 160,
                    shader_location: 15,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

// Fails to compile if `QuadInstance`'s size changes, so that
// `buffer_layout()`'s offsets are checked.
const _QUAD_INSTANCE_SIZE: () = assert!(std::mem::size_of::<QuadInstance>() == 164);

// ---------------------------------------------------------------------------
// atlas_page bit packing
//
// The selector mask and page shift are mirrored in the shader's `bindings.wgsl`.
// `wgsl_atlas_page_bit_layout_matches_rust` (below) parses the shader and fails the tests if
// their values ever drift.
// ---------------------------------------------------------------------------

/// [`QuadInstance::atlas_page`]/[`QuadInstance::base_atlas_page`] selector: sample `main_atlas`
/// (the multi-page `D2Array` pool).
pub const ATLAS_SELECTOR_MAIN: u32 = 0;
/// Selector: sample `transition_atlas` (single layer — page bits ignored).
pub const ATLAS_SELECTOR_TRANSITION: u32 = 1;
/// Selector: sample `video_atlas` (single layer — page bits ignored).
pub const ATLAS_SELECTOR_VIDEO: u32 = 2;

/// Mask isolating the atlas selector in a packed `atlas_page`/`base_atlas_page`.
pub const ATLAS_SELECTOR_MASK: u32 = 0xFF;
/// Bit position of the `main_atlas` page index in a packed `atlas_page`/`base_atlas_page`. A
/// full byte of selector space leaves 24 bits of page index — far beyond the hardware ceiling of
/// 256 array layers.
pub const ATLAS_PAGE_SHIFT: u32 = 8;

/// Packs an atlas selector and a `main_atlas` page index into one
/// [`QuadInstance::atlas_page`]/[`QuadInstance::base_atlas_page`] value.
///
/// `page` is ignored by the shader for the `ATLAS_SELECTOR_TRANSITION`/`ATLAS_SELECTOR_VIDEO`
/// selectors; pass 0 there.
#[inline]
pub const fn pack_atlas_page(selector: u32, page: u32) -> u32 {
    (selector & ATLAS_SELECTOR_MASK) | (page << ATLAS_PAGE_SHIFT)
}

/// Inverse of [`pack_atlas_page`] — returns `(selector, page)`.
#[inline]
pub const fn unpack_atlas_page(packed: u32) -> (u32, u32) {
    (packed & ATLAS_SELECTOR_MASK, packed >> ATLAS_PAGE_SHIFT)
}

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
        assert_eq!(offset_of!(QuadInstance, corner_radii), 52);
        assert_eq!(offset_of!(QuadInstance, uv_offset), 60);
        assert_eq!(offset_of!(QuadInstance, uv_scale), 68);
        assert_eq!(offset_of!(QuadInstance, atlas_page), 76);
        assert_eq!(offset_of!(QuadInstance, base_uv_offset), 80);
        assert_eq!(offset_of!(QuadInstance, base_uv_scale), 88);
        assert_eq!(offset_of!(QuadInstance, crossfade_t), 96);
        assert_eq!(offset_of!(QuadInstance, border_width), 100);
        assert_eq!(offset_of!(QuadInstance, border_color), 104);
        assert_eq!(offset_of!(QuadInstance, border_offset), 120);
        assert_eq!(offset_of!(QuadInstance, opacity), 124);
        assert_eq!(offset_of!(QuadInstance, shadow_params), 128);
        assert_eq!(offset_of!(QuadInstance, shadow_color), 144);
        assert_eq!(offset_of!(QuadInstance, base_atlas_page), 160);
    }

    #[test]
    fn quad_instance_total_size() {
        assert_eq!(std::mem::size_of::<QuadInstance>(), 164);
    }

    #[test]
    fn corner_radii_round_trip_through_half_floats() {
        let mut instance = QuadInstance::zeroed();
        instance.set_corner_radii([0.0, 10.0, 2048.0, 12.5]);
        assert_eq!(instance.corner_radii(), [0.0, 10.0, 2048.0, 12.5]);
        // Above 2048, half floats step by 2 pixels.
        instance.set_corner_radius(4097.0);
        assert!((instance.corner_radii()[0] - 4097.0).abs() <= 1.0);
    }

    // -----------------------------------------------------------------------
    // Transform math tests — mirrors the WGSL vertex shader logic in Rust so
    // we can verify correctness without spinning up a GPU.
    //
    // Shader reference (vertex.wgsl vs_main):
    //   scaled_size  = inst_size * inst_scale
    //   centered     = vertex_pos * scaled_size        ← unit quad vertex
    //   anchor_shift = (anchor - 0.5) * scaled_size * [1,-1]  ← Y-down→Y-up flip
    //   pivoted      = centered - anchor_shift
    //   rotated      = rotate(pivoted, rotation)
    //   world        = rotated + inst_position.xy
    //   clip         = view_projection * vec4(world, inst_position.z, 1)
    // -----------------------------------------------------------------------

    // Replicates the vertex shader transform for one vertex.
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

    // Center anchor, no rotation, scale=1 — simplest case.
    // vertex [-0.5,-0.5] of a 200×100 quad at origin should land at world (-100,-50).
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

    // Top-left anchor [0,0] (Y-down screen convention): the component's top-left
    // corner becomes the pivot. In Y-up world space the top-left vertex is [-0.5,+0.5].
    //
    // With the Y-flip:
    //   anchor_shift = ([0,0] - 0.5) * [200,100] * [1,-1] = [-100, +50]
    //   centered for [-0.5,+0.5] = (-100, +50)
    //   pivoted = (-100,+50) - (-100,+50) = (0, 0)  ✓
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

    // Scale=2 doubles the effective size. A 100×50 quad at scale=2 behaves
    // identically to a 200×100 quad at scale=1.
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

    // -----------------------------------------------------------------------
    // UV-computation tests — mirrors the WGSL fragment shader logic in Rust.
    //
    // The fragment shader computes `norm_uv` from `local_pos` and `half_size`:
    //
    //   let safe_half   = max(half_size, 0.5);
    //   let norm_uv_raw = (local_pos + safe_half) / (safe_half * 2.0);
    //   let norm_uv     = clamp(vec2(norm_uv_raw.x, 1.0 - norm_uv_raw.y), 0, 1);
    //
    // The Y-flip (1.0 - raw.y) is required because:
    //   • World Y increases UPWARD: top of entity = +half_y, bottom = -half_y
    //   • UV Y convention is DOWN:  top of texture = 0,      bottom = 1
    //
    // Without the flip, textures (BakedText, VideoPlayer) appear upside-down.
    // -----------------------------------------------------------------------

    // Mirror of the fragment shader's `texture_position`, in fill.wgsl.
    fn compute_norm_uv(local_pos: glam::Vec2, half_size: glam::Vec2) -> glam::Vec2 {
        let safe_half = half_size.max(glam::Vec2::splat(0.5));
        let raw = (local_pos + safe_half) / (safe_half * 2.0);
        // Y-flip: world Y goes up; UV Y goes down.
        glam::Vec2::new(raw.x, 1.0 - raw.y).clamp(glam::Vec2::ZERO, glam::Vec2::ONE)
    }

    // Top-center of the entity (local_pos.y = +half_y) must sample the TOP of
    // the texture (UV.y = 0.0).  Without the Y-flip this returns 1.0 instead.
    #[test]
    fn uv_top_of_entity_samples_texture_top() {
        let half = glam::Vec2::new(100.0, 50.0);
        let top_center = glam::Vec2::new(0.0, half.y);
        let uv = compute_norm_uv(top_center, half);
        assert!(
            (uv.x - 0.5).abs() < 1e-5,
            "UV X at center should be 0.5, got {}",
            uv.x
        );
        assert!(
            (uv.y - 0.0).abs() < 1e-5,
            "UV Y at entity top must be 0.0 (texture top), got {} — Y-flip missing?",
            uv.y
        );
    }

    // Bottom-center of the entity (local_pos.y = -half_y) must sample the BOTTOM
    // of the texture (UV.y = 1.0).  Without the Y-flip this returns 0.0 instead.
    #[test]
    fn uv_bottom_of_entity_samples_texture_bottom() {
        let half = glam::Vec2::new(100.0, 50.0);
        let bottom_center = glam::Vec2::new(0.0, -half.y);
        let uv = compute_norm_uv(bottom_center, half);
        assert!(
            (uv.x - 0.5).abs() < 1e-5,
            "UV X at center should be 0.5, got {}",
            uv.x
        );
        assert!(
            (uv.y - 1.0).abs() < 1e-5,
            "UV Y at entity bottom must be 1.0 (texture bottom), got {} — Y-flip missing?",
            uv.y
        );
    }

    // Top-left corner of the entity in world-Y-up space is (−half_x, +half_y).
    // It must map to UV (0, 0) — the top-left of the texture.
    #[test]
    fn uv_top_left_entity_corner_maps_to_uv_origin() {
        let half = glam::Vec2::new(100.0, 50.0);
        let top_left = glam::Vec2::new(-half.x, half.y);
        let uv = compute_norm_uv(top_left, half);
        assert!((uv.x - 0.0).abs() < 1e-5, "UV X at left edge should be 0.0");
        assert!((uv.y - 0.0).abs() < 1e-5, "UV Y at top should be 0.0");
    }

    // Bottom-right corner of the entity in world-Y-up space is (+half_x, −half_y).
    // It must map to UV (1, 1) — the bottom-right of the texture.
    #[test]
    fn uv_bottom_right_entity_corner_maps_to_uv_one_one() {
        let half = glam::Vec2::new(100.0, 50.0);
        let bottom_right = glam::Vec2::new(half.x, -half.y);
        let uv = compute_norm_uv(bottom_right, half);
        assert!(
            (uv.x - 1.0).abs() < 1e-5,
            "UV X at right edge should be 1.0"
        );
        assert!((uv.y - 1.0).abs() < 1e-5, "UV Y at bottom should be 1.0");
    }

    // Center of the entity must always map to UV (0.5, 0.5).
    #[test]
    fn uv_center_of_entity_maps_to_uv_center() {
        let half = glam::Vec2::new(200.0, 80.0);
        let center = glam::Vec2::ZERO;
        let uv = compute_norm_uv(center, half);
        assert!((uv.x - 0.5).abs() < 1e-5, "UV X at center should be 0.5");
        assert!((uv.y - 0.5).abs() < 1e-5, "UV Y at center should be 0.5");
    }

    // 90° CCW rotation: a point at (1, 0) should become (0, 1).
    // Use a 2×2 quad at center anchor so vertex (0.5, -0.5) → world (1, -1) pre-rotation,
    // then after 90° CCW it should be (1, 1).
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

    // Orthographic projection: a world point at pixel (640, 400) on a 1280×800
    // viewport should map to NDC (1, 1) — the top-right corner.
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

    // -----------------------------------------------------------------------
    // atlas_page bit packing
    // -----------------------------------------------------------------------

    #[test]
    fn pack_atlas_page_round_trips_selector_and_page() {
        for selector in [
            ATLAS_SELECTOR_MAIN,
            ATLAS_SELECTOR_TRANSITION,
            ATLAS_SELECTOR_VIDEO,
        ] {
            for page in [0, 1, 3, 255] {
                assert_eq!(
                    unpack_atlas_page(pack_atlas_page(selector, page)),
                    (selector, page)
                );
            }
        }
    }

    #[test]
    fn pack_atlas_page_is_bit_identical_to_the_legacy_encoding_for_page_zero() {
        // Page 0 must pack to the selector alone, which code that writes
        // 0, 1 or 2 directly relies on.
        assert_eq!(pack_atlas_page(ATLAS_SELECTOR_MAIN, 0), 0);
        assert_eq!(pack_atlas_page(ATLAS_SELECTOR_TRANSITION, 0), 1);
        assert_eq!(pack_atlas_page(ATLAS_SELECTOR_VIDEO, 0), 2);
    }

    #[test]
    fn wgsl_atlas_page_bit_layout_matches_rust() {
        // The shader copies these constants, since WGSL can't include Rust. If
        // one side changes without the other, the main atlas samples the wrong
        // page; this catches it in tests instead. The shader is parsed, so its
        // formatting doesn't matter, only the values.
        use wgpu::naga;

        let module =
            naga::front::wgsl::parse_str(crate::QUAD_SHADER_SRC).expect("the shader parses");
        let value = |name: &str| -> u32 {
            let (_, constant) = module
                .constants
                .iter()
                .find(|(_, c)| c.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("the shader declares {name}"));
            match module.global_expressions[constant.init] {
                naga::Expression::Literal(naga::Literal::U32(v)) => v,
                ref other => panic!("{name} is a u32 literal in the shader, not {other:?}"),
            }
        };
        assert_eq!(
            value("ATLAS_SELECTOR_MASK"),
            ATLAS_SELECTOR_MASK,
            "the shader's ATLAS_SELECTOR_MASK drifted from mesh.rs's"
        );
        assert_eq!(
            value("ATLAS_PAGE_SHIFT"),
            ATLAS_PAGE_SHIFT,
            "the shader's ATLAS_PAGE_SHIFT drifted from mesh.rs's"
        );
    }
}
