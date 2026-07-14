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
// the Metal/wgpu limit of 16 total vertex attributes (2 vertex + 13 instance = 15).
//
// Packed fields are unpacked at the top of vs_main with named locals.
struct VertexIn {
    // QuadVertex — locations 0–1, step per vertex
    @location(0) position:             vec2<f32>,
    @location(1) uv:                   vec2<f32>,

    // QuadInstance — locations 2–14, step per instance
    @location(2)  inst_position:       vec3<f32>,
    // .xy = size, .z = rotation (radians), .w = scale
    @location(3)  inst_size_rot_scale: vec4<f32>,
    @location(4)  inst_anchor:         vec2<f32>,  // Y-down screen: [0,0]=top-left [0.5,0.5]=center [1,1]=bottom-right
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
    // .x = shadow offset X, .y = shadow offset Y, .z = softness (px), .w = spread (px)
    @location(13) inst_shadow_params:  vec4<f32>,
    @location(14) inst_shadow_color:   vec4<f32>,  // RGBA; alpha == 0.0 disables shadow
}

struct VertexOut {
    @builtin(position)               clip_position: vec4<f32>,

    // Position relative to the component center in pixels — used for SDF.
    // Always centered regardless of anchor setting.
    // When shadow is active, fragments outside the main shape (in the shadow area)
    // have |local_pos| > half_size; the shadow SDF handles them correctly.
    @location(0)                     local_pos:     vec2<f32>,
    // Half-extents of the original (un-inflated) component in pixels.
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

    // Drop shadow (M8)
    @location(12)                    shadow_params: vec4<f32>, // (offset_x, offset_y, softness, spread)
    @location(13)                    shadow_color:  vec4<f32>, // RGBA; alpha == 0.0 = no shadow
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;

    // Unpack packed fields.
    let inst_size          = in.inst_size_rot_scale.xy;
    let inst_rotation      = in.inst_size_rot_scale.z;
    let inst_scale         = in.inst_size_rot_scale.w;
    let inst_opacity       = in.inst_opacity_radius.x;
    let inst_corner_radius = in.inst_opacity_radius.y;
    let inst_uv_offset     = in.inst_uv.xy;
    let inst_uv_scale      = in.inst_uv.zw;
    let inst_base_uv_offset = in.inst_base_uv.xy;
    let inst_base_uv_scale  = in.inst_base_uv.zw;
    let inst_crossfade_t   = in.inst_crossfade_bw.x;
    let inst_border_width  = in.inst_crossfade_bw.y;

    let shadow_offset_x = in.inst_shadow_params.x;
    let shadow_offset_y = in.inst_shadow_params.y;
    let shadow_softness = in.inst_shadow_params.z;
    let shadow_spread   = in.inst_shadow_params.w;
    let shadow_active   = in.inst_shadow_color.a > 0.0;

    let scaled_size = inst_size * inst_scale;
    let half        = scaled_size * 0.5;

    // Inflate the quad geometry when shadow is active so that shadow fragments
    // that fall outside the main shape boundary are still rasterized.
    //
    // For each edge we extend it by max(0, shadow_offset_in_that_direction) + pad,
    // where pad = softness + spread.  The sign of in.position tells us which edge.
    var inflate = vec2(0.0, 0.0);
    if shadow_active {
        let pad = shadow_softness + shadow_spread;
        if in.position.x >= 0.0 {
            inflate.x = max(0.0, shadow_offset_x) + pad;
        } else {
            inflate.x = -(max(0.0, -shadow_offset_x) + pad);
        }
        if in.position.y >= 0.0 {
            inflate.y = max(0.0, shadow_offset_y) + pad;
        } else {
            inflate.y = -(max(0.0, -shadow_offset_y) + pad);
        }
    }

    // Scale the unit quad vertex into component-local pixel space, then apply
    // the shadow inflation so the rasterized area covers the full shadow footprint.
    let centered = in.position * scaled_size + inflate;

    // Shift so that the declared anchor becomes the pivot for rotation and scale.
    // anchor [0.5, 0.5] = center → no shift.
    // Anchor uses Y-down screen convention: [0,0] = top-left, [1,1] = bottom-right.
    // The Y component is negated to convert from screen-Y-down to world-Y-up so
    // [0,0] pins the top-left corner of the component as expected by callers.
    let anchor_shift = (in.inst_anchor - vec2(0.5, 0.5)) * scaled_size * vec2(1.0, -1.0);
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

    // local_pos is the inflated centered position — the actual pixel coordinate
    // relative to the component center.  For fragments inside the main shape this
    // behaves exactly as before; for shadow-only fragments (|local_pos| > half_size)
    // the shadow SDF in fs_main takes over.
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
    out.shadow_params = in.inst_shadow_params;
    out.shadow_color  = in.inst_shadow_color;

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

    // Unpack shadow params.
    let shadow_offset   = in.shadow_params.xy;
    let shadow_softness = in.shadow_params.z;
    let shadow_spread   = in.shadow_params.w;

    // --- Shadow alpha ---
    // Compute shadow contribution first so we can avoid discarding shadow-only
    // fragments that lie outside the main shape boundary.
    var shadow_alpha = 0.0;
    if in.shadow_color.a > 0.0 {
        // The shadow shape is the main rounded rect, expanded by spread and shifted
        // by the shadow offset.  A higher softness value widens the penumbra.
        let shadow_half_size = in.half_size + vec2(shadow_spread, shadow_spread);
        let shadow_radius    = in.corner_radius + shadow_spread;
        let shadow_dist      = sdf_rounded_rect(in.local_pos - shadow_offset, shadow_half_size, shadow_radius);
        // Clamp softness so smoothstep never has a zero-width range.
        let softness   = max(shadow_softness, 0.5);
        shadow_alpha   = (1.0 - smoothstep(-softness, softness, shadow_dist)) * in.shadow_color.a;
    }

    // --- Shape SDF ---
    // dist < 0 inside the rounded rect, > 0 outside.
    let dist       = sdf_rounded_rect(in.local_pos, in.half_size, in.corner_radius);
    // 1-pixel antialiased edge. edge_alpha → 0 as dist → +1 (outside the shape).
    let edge_alpha = 1.0 - smoothstep(-1.0, 1.0, dist);

    // Discard fragments where neither the shadow nor the main shape contribute.
    if edge_alpha <= 0.0 && shadow_alpha <= 0.0 {
        discard;
    }

    // --- Main shape color ---
    // Computed only for fragments inside (or on the edge of) the main shape.
    var main_color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if edge_alpha > 0.0 {
        // Texture sampling.
        // Primary texture: atlas_page selects which atlas to sample.
        // Components without a texture point at a 1×1 white pixel baked into main_atlas
        // at init, so the color tint alone determines their appearance with no branching.
        // textureSampleLevel (LOD 0) is used instead of textureSample because both
        // branches vary per-instance (non-uniform control flow). textureSample requires
        // implicit derivatives, which are undefined in non-uniform control flow per the
        // WGSL spec and fail on strict backends. Level 0 is always correct here: our
        // atlases have exactly one mip level.
        var tex_color: vec4<f32>;
        if in.atlas_page == 0u {
            tex_color = textureSampleLevel(main_atlas,       atlas_sampler, in.atlas_uv,      0.0);
        } else {
            tex_color = textureSampleLevel(transition_atlas, atlas_sampler, in.atlas_uv,      0.0);
        }

        // Crossfade: blend from-state (always in transition_atlas) into to-state.
        // When crossfade_t == 0.0 this branch is skipped entirely.
        if in.crossfade_t > 0.0 {
            let base_color = textureSampleLevel(transition_atlas, atlas_sampler, in.base_atlas_uv, 0.0);
            tex_color = mix(base_color, tex_color, in.crossfade_t);
        }

        // Color tint + opacity.
        // color.a tints the texture alpha independently; opacity is the whole-component multiplier.
        main_color    = tex_color * in.color;
        main_color.a *= in.opacity * edge_alpha;

        // Border (SDF-based).
        // Zero cost when border_width == 0.0 — the branch is never entered.
        //
        // LIMITATION (M4+): only inner borders (border_offset = -1.0) render correctly.
        // border_offset = 0.0 (centered) shows only the inner half of the band.
        // border_offset = 1.0 (outer) renders nothing: fragments beyond the rect edge
        // are never rasterized, and edge_alpha goes to zero right at the boundary.
        // Full outer-border support requires inflating the quad geometry by the border's
        // outer extent so outer fragments are actually rasterized.
        if in.border_width > 0.0 {
            let half_w = in.border_width * 0.5;
            // border_offset shifts the border band relative to the shape edge:
            //   -1.0 = fully inside, 0.0 = centered on edge, 1.0 = fully outside (broken, see above)
            let border_center = in.border_offset * half_w;
            let border_dist   = abs(dist - border_center) - half_w;
            let border_alpha  = (1.0 - smoothstep(-1.0, 1.0, border_dist)) * edge_alpha;

            // Composite border over fill using standard alpha blending.
            let b      = in.border_color;
            let b_a    = b.a * border_alpha;
            main_color = vec4(
                mix(main_color.rgb, b.rgb, b_a),
                max(main_color.a, b_a),
            );
        }
    }

    // --- Composite: shadow UNDER main shape ---
    // Porter-Duff "over" (straight alpha): main_color over shadow.
    //   result.a   = main.a + shadow.a * (1 − main.a)
    //   result.rgb = (main.rgb × main.a + shadow.rgb × shadow.a × (1 − main.a)) / result.a
    //
    // When there is no shadow (shadow_alpha == 0) this collapses to returning
    // main_color unchanged — identical to the pre-M8 behavior.
    let src_a = main_color.a;
    let dst_a = shadow_alpha;
    let out_a = src_a + dst_a * (1.0 - src_a);
    var out_rgb = vec3<f32>(0.0);
    if out_a > 0.0 {
        out_rgb = (main_color.rgb * src_a + in.shadow_color.rgb * dst_a * (1.0 - src_a)) / out_a;
    }

    return vec4(out_rgb, out_a);
}
