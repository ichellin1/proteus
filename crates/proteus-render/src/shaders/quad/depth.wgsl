// ---------------------------------------------------------------------------
// Skipping covered pixels: a depth pre-pass, so that a pixel a later opaque
// component covers isn't shaded at all.
//
// Every instance's depth is its place in drawing order, later ones nearer, so
// a depth test keeps exactly the pixels drawing order would show on top. The
// pre-pass draws only opaque components, and only the part of each certainly
// opaque in every pixel, writing depth and nothing else; the main pass then
// tests depth without writing it, and the GPU rejects a covered pixel before
// running the fragment shader. The cover itself passes, at its own depth. It
// draws the same pixels as without it: a pixel it skips is one a component
// drawn later replaces entirely.
// ---------------------------------------------------------------------------

// The depth of instance `index`: 1 - (index + 1) / 2^24, so that later
// instances are nearer. Exact in 32-bit floats for every index up to 2^24 - 1,
// far more than an instance buffer holds, so no two instances share a depth.
fn draw_depth(index: u32) -> f32 {
    return 1.0 - f32(index + 1u) * (1.0 / 16777216.0);
}

// The pre-pass's vertex stage. An opaque component, a solid color with full
// alpha and opacity and no texture, border, shadow, glow or crossfade, is
// drawn shrunk to the rectangle well inside its rounded corners and its
// antialiased edge, where the main pass draws its color with full alpha, so
// whatever is behind is hidden. Anything else is placed outside the clip
// volume, so it draws nothing.
@vertex
fn vs_depth(in: VertexIn) -> @builtin(position) vec4<f32> {
    let opaque = all(in.inst_uv == vec4(0.0))
        && in.inst_atlas_page == 0u
        && in.inst_crossfade_bw.y <= 0.0
        && in.inst_shadow_color.a <= 0.0
        && in.inst_crossfade_bw.x <= 0.0
        && in.inst_color.a >= 1.0
        && in.inst_border_offset_opacity.y >= 1.0;
    if !opaque {
        return vec4(2.0, 2.0, 2.0, 1.0);
    }
    // Inside the largest corner, and 2 pixels in from the edge rather than
    // the 1 the edge's antialiasing takes, so rounding between the two passes
    // can't put a pixel the main pass blends into the rectangle.
    let r     = in.inst_corner_radii;
    let inset = max(max(r.x, r.y), max(r.z, r.w)) + 2.0;
    let scaled_size = in.inst_size_rot_scale.xy * in.inst_size_rot_scale.w;
    let inner = max(scaled_size - vec2(2.0 * inset), vec2(0.0));
    return to_clip(in, in.position * inner, draw_depth(in.instance));
}

// The pre-pass writes depth only. Its pipeline masks off the color target,
// which a render pass's pipelines must all declare.
@fragment
fn fs_depth() -> @location(0) vec4<f32> {
    return vec4(0.0);
}
