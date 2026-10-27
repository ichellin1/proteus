// ---------------------------------------------------------------------------
// The vertex stage: the instance's layout, and placing each quad.
// ---------------------------------------------------------------------------

// QuadInstance fields are packed into fewer attribute locations to stay within
// the Metal/wgpu limit of 16 vertex attributes: 2 vertex + 14 instance = 16.
//
// Packed fields are unpacked at the top of vs_main with named locals.
struct VertexIn {
    // QuadVertex — locations 0–1, step per vertex
    @location(0) position:             vec2<f32>,
    @location(1) uv:                   vec2<f32>,

    // The instance's place in drawing order, counted from the start of the
    // instance buffer whichever run a draw call starts at: on WebGL2 and
    // Direct3D, wgpu adds the draw's first instance itself. Read it only in an
    // entry point, never in a function an entry point calls: on Direct3D,
    // naga adds the first instance only where an entry point reads it, so a
    // function would get the index within the draw call.
    @builtin(instance_index) instance: u32,

    // QuadInstance — locations 2–15, step per instance
    @location(2)  inst_position:       vec3<f32>,
    // .xy = size, .z = rotation (radians), .w = scale
    @location(3)  inst_size_rot_scale: vec4<f32>,
    @location(4)  inst_anchor:         vec2<f32>,  // Y-down screen: [0,0]=top-left [0.5,0.5]=center [1,1]=bottom-right
    @location(5)  inst_color:          vec4<f32>,
    // Each corner's radius in pixels: top-left, top-right, bottom-right,
    // bottom-left. Half floats in the instance buffer, widened by the GPU.
    @location(6)  inst_corner_radii:   vec4<f32>,
    // .xy = uv_offset, .zw = uv_scale
    @location(7)  inst_uv:             vec4<f32>,
    @location(8)  inst_atlas_page:     u32,        // packed: selector | (main_atlas page << ATLAS_PAGE_SHIFT)
    // .xy = base_uv_offset, .zw = base_uv_scale
    @location(9)  inst_base_uv:        vec4<f32>,
    // .x = crossfade_t, .y = border_width (0.0 = no border)
    @location(10) inst_crossfade_bw:   vec2<f32>,
    @location(11) inst_border_color:   vec4<f32>,
    // .x = border_offset (-1.0 inner / 0.0 center / 1.0 outer), .y = opacity
    @location(12) inst_border_offset_opacity: vec2<f32>,
    // .x = shadow offset X, .y = shadow offset Y, .z = softness (px), .w = spread (px)
    @location(13) inst_shadow_params:  vec4<f32>,
    @location(14) inst_shadow_color:   vec4<f32>,  // RGBA; alpha == 0.0 disables shadow
    // Which atlas base_uv samples from during crossfade — independent of
    // inst_atlas_page (the "to" side). Same packed selector/page layout as
    // inst_atlas_page above. See mesh.rs's QuadInstance::base_atlas_page.
    @location(15) inst_base_atlas_page: u32,
}

struct VertexOut {
    @builtin(position)               clip_position:   vec4<f32>,

    // Position relative to the component center in pixels — used for SDF.
    // Always centered regardless of anchor setting.
    // When shadow is active, fragments outside the main shape (in the shadow area)
    // have |local_pos| > half_size; the shadow SDF handles them correctly.
    // The only value that varies across the quad, so the only one interpolated.
    @location(0)                     local_pos:       vec2<f32>,

    // The rest are the same at every vertex of an instance, so they're `flat`:
    // the GPU passes them to each fragment as they are, rather than
    // interpolating them, which would give the same values for more work.

    // Half-extents of the original (un-inflated) component in pixels.
    @location(1)  @interpolate(flat) half_size:       vec2<f32>,

    // Raw UV parameters — .xy = offset, .zw = scale — passed through from the
    // instance buffer without baking in.uv.  The fragment shader derives the
    // actual sample coordinate from local_pos + half_size so that shadow/glow
    // inflation does not distort textured content (video, BakedText).
    @location(2)  @interpolate(flat) uv_params:       vec4<f32>,  // primary (to-state)
    @location(3)  @interpolate(flat) base_uv_params:  vec4<f32>,  // from-state crossfade

    // Fragment-stage per-instance data
    @location(4)  @interpolate(flat) color:           vec4<f32>,
    @location(5)  @interpolate(flat) opacity:         f32,
    @location(6)  @interpolate(flat) corner_radii:    vec4<f32>, // tl, tr, br, bl
    @location(7)  @interpolate(flat) crossfade_t:     f32,
    @location(8)  @interpolate(flat) border_width:    f32,
    @location(9)  @interpolate(flat) border_color:    vec4<f32>,
    @location(10) @interpolate(flat) border_offset:   f32,
    @location(11) @interpolate(flat) atlas_page:      u32,

    // Drop shadow / glow
    @location(12) @interpolate(flat) shadow_params:   vec4<f32>, // (offset_x, offset_y, softness, spread)
    @location(13) @interpolate(flat) shadow_color:    vec4<f32>, // RGBA; alpha == 0.0 = no shadow

    @location(14) @interpolate(flat) base_atlas_page: u32,  // which atlas base_uv samples from
}

// Where an instance's vertex goes: its clip-space position, and its position
// relative to the component's center, in pixels, with the component's half
// size. Both pipelines' vertex stages place quads with it, so they line up
// exactly.
struct Placed {
    clip_position: vec4<f32>,
    local_pos:     vec2<f32>,
    half_size:     vec2<f32>,
}

fn place(in: VertexIn, depth: f32) -> Placed {
    var out: Placed;

    // Unpack packed fields.
    let inst_size          = in.inst_size_rot_scale.xy;
    let inst_rotation      = in.inst_size_rot_scale.z;
    let inst_scale         = in.inst_size_rot_scale.w;

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

    out.clip_position = to_clip(in, centered, depth);

    // local_pos is the inflated centered position: the pixel coordinate relative
    // to the component center. Fragments inside the main shape use it as is; for
    // shadow-only fragments (|local_pos| > half_size) the shadow SDF in fs_main
    // takes over.
    out.local_pos = centered;
    out.half_size = half;
    return out;
}

// Where `centered`, a point relative to the component's center in pixels, goes
// in clip space: pivoted on the anchor, rotated, moved to the component's
// position and projected, at `depth`, from `draw_depth`.
fn to_clip(in: VertexIn, centered: vec2<f32>, depth: f32) -> vec4<f32> {
    let inst_rotation = in.inst_size_rot_scale.z;
    let scaled_size   = in.inst_size_rot_scale.xy * in.inst_size_rot_scale.w;

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

    // Translate to world space and project. `z` is left out: it only orders
    // components, which the instance order already does, and as a depth it
    // would clip any component outside the projection's depth range. The
    // depth is the instance's place in drawing order instead; see `depth.wgsl`.
    let world = vec4(rotated + in.inst_position.xy, 0.0, 1.0);
    var clip = uniforms.view_projection * world;
    clip.z = depth * clip.w;
    return clip;
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let placed = place(in, draw_depth(in.instance));
    out.clip_position = placed.clip_position;
    out.local_pos     = placed.local_pos;
    out.half_size     = placed.half_size;


    // Pass raw UV parameters through to the fragment stage.  The fragment shader
    // derives the sample coordinate from local_pos + half_size rather than from
    // the per-vertex in.uv, which avoids distortion caused by shadow/glow
    // geometry inflation stretching the vertex-interpolated UV across a larger area.
    out.uv_params      = in.inst_uv;       // .xy = offset, .zw = scale
    out.base_uv_params = in.inst_base_uv;  // .xy = base_offset, .zw = base_scale

    out.color         = in.inst_color;
    out.opacity       = in.inst_border_offset_opacity.y;
    out.corner_radii  = in.inst_corner_radii;
    out.crossfade_t   = in.inst_crossfade_bw.x;
    out.border_width  = in.inst_crossfade_bw.y;
    out.border_color  = in.inst_border_color;
    out.border_offset = in.inst_border_offset_opacity.x;
    out.atlas_page    = in.inst_atlas_page;
    out.shadow_params = in.inst_shadow_params;
    out.shadow_color  = in.inst_shadow_color;
    out.base_atlas_page = in.inst_base_atlas_page;

    return out;
}
