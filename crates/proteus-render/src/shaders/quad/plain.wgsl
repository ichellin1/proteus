// ---------------------------------------------------------------------------
// The plain pipeline: a second pair of entry points, for plain components, a
// solid color with no texture, border, shadow, glow or crossfade, which are
// most of what a UI draws. The rest of the shader is compiled out of it, so the
// GPU runs more of these pixels at once than it can of the full shader's,
// whose most demanding path sets how much room every pixel gets. It draws
// exactly what `fs_main` draws for the same instance.
//
// `QuadPipeline::draw` sends it runs of plain instances big enough to be worth
// a draw call of their own; `is_plain` in pipeline.rs is the same test as
// this pipeline assumes.
// ---------------------------------------------------------------------------

struct PlainOut {
    @builtin(position)               clip_position: vec4<f32>,
    @location(0)                     local_pos:     vec2<f32>,
    @location(1) @interpolate(flat)  half_size:     vec2<f32>,
    @location(2) @interpolate(flat)  color:         vec4<f32>,
    @location(3) @interpolate(flat)  opacity:       f32,
    @location(4) @interpolate(flat)  corner_radii:  vec4<f32>,
}

@vertex
fn vs_plain(in: VertexIn) -> PlainOut {
    var out: PlainOut;
    let placed = place(in, draw_depth(in.instance));
    out.clip_position = placed.clip_position;
    out.local_pos     = placed.local_pos;
    out.half_size     = placed.half_size;
    out.color         = in.inst_color;
    out.opacity       = in.inst_border_offset_opacity.y;
    out.corner_radii  = in.inst_corner_radii;
    return out;
}

// The same steps as `without_effects` for a component with no texture.
@fragment
fn fs_plain(in: PlainOut) -> @location(0) vec4<f32> {
    let edge = edge_alpha(sdf_rounded_rect(in.local_pos, in.half_size, in.corner_radii));
    if edge <= 0.0 {
        discard;
    }
    var fill = in.color;
    fill.a *= in.opacity * edge;
    return over_shadow(fill, 0.0, vec3(0.0));
}
