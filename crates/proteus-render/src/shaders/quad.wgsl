// =============================================================================
// quad.wgsl — Proteus instanced quad renderer
//
// Renders every visible component in one instanced draw call.
//
// Bind group 0:  uniform buffer   — view/projection matrix (one upload per frame)
// Bind group 1:  main_atlas       — long-lived textures (images, static bakes)
//                transition_atlas — ephemeral bakes for in-flight transitions
//                atlas_sampler    — shared linear sampler
//
// Vertex buffer 0:  QuadVertex   — base unit quad, 4 vertices, step per vertex
// Vertex buffer 1:  QuadInstance — per-component data, step per instance
// =============================================================================


// ---------------------------------------------------------------------------
// Bind groups
// ---------------------------------------------------------------------------

struct Uniforms {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(1) @binding(0) var main_atlas:       texture_2d<f32>;
@group(1) @binding(1) var transition_atlas: texture_2d<f32>;
@group(1) @binding(2) var atlas_sampler:    sampler;


// ---------------------------------------------------------------------------
// Vertex stage
// ---------------------------------------------------------------------------

// QuadInstance fields are packed into fewer attribute locations to stay under
// the Metal/wgpu limit of 16 total vertex attributes (2 vertex + 11 instance = 13).
//
// Packed fields are unpacked at the top of vs_main with named locals.
struct VertexIn {
    // QuadVertex — locations 0–1, step per vertex
    @location(0) position:             vec2<f32>,
    @location(1) uv:                   vec2<f32>,

    // QuadInstance — locations 2–12, step per instance
    @location(2)  inst_position:       vec3<f32>,
    // .xy = size, .z = rotation (radians), .w = scale
    @location(3)  inst_size_rot_scale: vec4<f32>,
    @location(4)  inst_anchor:         vec2<f32>,  // 0.0–1.0; [0.5, 0.5] = center
    @location(5)  inst_color:          vec4<f32>,
    // .x = opacity, .y = corner_radius (pixels)
    @location(6)  inst_opacity_radius: vec2<f32>,
    // .xy = uv_offset, .zw = uv_scale
    @location(7)  inst_uv:             vec4<f32>,
    @location(8)  inst_atlas_page:     u32,        // 0 = main_atlas, 1 = transition_atlas
    // .xy = base_uv_offset, .zw = base_uv_scale
    @location(9)  inst_base_uv:        vec4<f32>,
    // .x = crossfade_t, .y = border_width (0.0 = no border)
    @location(10) inst_crossfade_bw:   vec2<f32>,
    @location(11) inst_border_color:   vec4<f32>,
    @location(12) inst_border_offset:  f32,        // -1.0 inner / 0.0 center / 1.0 outer
}

struct VertexOut {
    @builtin(position)               clip_position: vec4<f32>,

    // Position relative to the component center in pixels — used for SDF.
    // Always centered regardless of anchor setting.
    @location(0)                     local_pos:     vec2<f32>,
    // Half-extents of the component in pixels — used for SDF.
    @location(1)                     half_size:     vec2<f32>,

    // Atlas UVs
    @location(2)                     atlas_uv:      vec2<f32>,  // primary (to-state)
    @location(3)                     base_atlas_uv: vec2<f32>,  // from-state crossfade

    // Fragment-stage per-instance data
    @location(4)                     color:         vec4<f32>,
    @location(5)                     opacity:       f32,
    @location(6)                     corner_radius: f32,
    @location(7)                     crossfade_t:   f32,
    @location(8)                     border_width:  f32,
    @location(9)                     border_color:  vec4<f32>,
    @location(10)                    border_offset: f32,
    @location(11) @interpolate(flat) atlas_page:    u32,  // flat — no interpolation for integers
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;

    // Unpack packed fields.
    let inst_size         = in.inst_size_rot_scale.xy;
    let inst_rotation     = in.inst_size_rot_scale.z;
    let inst_scale        = in.inst_size_rot_scale.w;
    let inst_opacity      = in.inst_opacity_radius.x;
    let inst_corner_radius = in.inst_opacity_radius.y;
    let inst_uv_offset    = in.inst_uv.xy;
    let inst_uv_scale     = in.inst_uv.zw;
    let inst_base_uv_offset = in.inst_base_uv.xy;
    let inst_base_uv_scale  = in.inst_base_uv.zw;
    let inst_crossfade_t  = in.inst_crossfade_bw.x;
    let inst_border_width = in.inst_crossfade_bw.y;

    let scaled_size = inst_size * inst_scale;
    let half        = scaled_size * 0.5;

    // Scale the unit quad vertex into component-local pixel space.
    // At this point the component is centered at the origin.
    let centered = in.position * scaled_size;

    // Shift so that the declared anchor becomes the pivot for rotation and scale.
    // anchor [0.5, 0.5] = center → no shift.
    // anchor [0.0, 0.0] = top-left → shift right and down by half the size.
    let anchor_shift = (in.inst_anchor - vec2(0.5, 0.5)) * scaled_size;
    let pivoted = centered - anchor_shift;

    // Rotate in pivot space.
    let cos_r   = cos(inst_rotation);
    let sin_r   = sin(inst_rotation);
    let rotated = vec2(
        pivoted.x * cos_r - pivoted.y * sin_r,
        pivoted.x * sin_r + pivoted.y * cos_r,
    );

    // Translate to world space and project.
    let world = vec4(rotated + in.inst_position.xy, in.inst_position.z, 1.0);
    out.clip_position = uniforms.view_projection * world;

    // SDF uses the centered (pre-pivot-shift) position so corner rounding is
    // correct regardless of anchor setting.
    out.local_pos = centered;
    out.half_size = half;

    // Map the base quad UVs into the atlas sub-regions.
    out.atlas_uv      = inst_uv_offset      + in.uv * inst_uv_scale;
    out.base_atlas_uv = inst_base_uv_offset + in.uv * inst_base_uv_scale;

    out.color         = in.inst_color;
    out.opacity       = inst_opacity;
    out.corner_radius = inst_corner_radius;
    out.crossfade_t   = inst_crossfade_t;
    out.border_width  = inst_border_width;
    out.border_color  = in.inst_border_color;
    out.border_offset = in.inst_border_offset;
    out.atlas_page    = in.inst_atlas_page;

    return out;
}


// ---------------------------------------------------------------------------
// Fragment stage
// ---------------------------------------------------------------------------

// Signed distance field for a rounded rectangle, centered at the origin.
// Returns: negative inside the shape, positive outside, zero at the edge.
//   p         — fragment position relative to rect center (pixels)
//   half_size — half-extents of the rect (pixels)
//   r         — corner radius (pixels); 0.0 = sharp corners
fn sdf_rounded_rect(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2(r, r);
    return length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {

    // --- Shape SDF ---
    // dist < 0 inside the rounded rect, > 0 outside.
    let dist       = sdf_rounded_rect(in.local_pos, in.half_size, in.corner_radius);
    // 1-pixel antialiased edge. edge_alpha → 0 as dist → +1 (outside the shape).
    let edge_alpha = 1.0 - smoothstep(-1.0, 1.0, dist);
    if edge_alpha <= 0.0 {
        discard;
    }

    // --- Texture sampling ---
    // Primary texture: atlas_page selects which atlas to sample.
    // Components without a texture point at a 1×1 white pixel baked into main_atlas
    // at init, so the color tint alone determines their appearance with no branching.
    var tex_color: vec4<f32>;
    if in.atlas_page == 0u {
        tex_color = textureSample(main_atlas,       atlas_sampler, in.atlas_uv);
    } else {
        tex_color = textureSample(transition_atlas, atlas_sampler, in.atlas_uv);
    }

    // Crossfade: blend from-state (always in transition_atlas) into to-state.
    // When crossfade_t == 0.0 this branch is skipped entirely.
    if in.crossfade_t > 0.0 {
        let base_color = textureSample(transition_atlas, atlas_sampler, in.base_atlas_uv);
        tex_color = mix(base_color, tex_color, in.crossfade_t);
    }

    // --- Color tint + opacity ---
    // color.a tints the texture alpha independently; opacity is the whole-component multiplier.
    var out_color    = tex_color * in.color;
    out_color.a     *= in.opacity * edge_alpha;

    // --- Border (SDF-based) ---
    // Zero cost when border_width == 0.0 — the branch is never entered.
    if in.border_width > 0.0 {
        let half_w = in.border_width * 0.5;
        // border_offset shifts the border band relative to the shape edge:
        //   -1.0 = fully inside, 0.0 = centered on edge, 1.0 = fully outside
        let border_center = in.border_offset * half_w;
        let border_dist   = abs(dist - border_center) - half_w;
        let border_alpha  = (1.0 - smoothstep(-1.0, 1.0, border_dist)) * edge_alpha;

        // Composite border over fill using standard alpha blending.
        let b      = in.border_color;
        let b_a    = b.a * border_alpha;
        out_color  = vec4(
            mix(out_color.rgb, b.rgb, b_a),
            max(out_color.a, b_a),
        );
    }

    return out_color;
}
