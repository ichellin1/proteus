// ---------------------------------------------------------------------------
// The shape: a rectangle with a radius per corner, and its antialiased edge.
// ---------------------------------------------------------------------------

// Signed distance field for a rounded rectangle, centered at the origin.
// Returns: negative inside the shape, positive outside, zero at the edge.
//   p         — fragment position relative to rect center (pixels), y up
//   half_size — half-extents of the rect (pixels)
//   radii     — each corner's radius (pixels): top-left, top-right,
//               bottom-right, bottom-left; 0.0 = a sharp corner
// Each quadrant of the rect is shaped by its own corner's radius, at most half
// the shorter side, as CSS does: a larger one would shrink the shape, while
// this rounds the ends of a short, wide one into a pill.
fn sdf_rounded_rect(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let top    = select(radii.x, radii.y, p.x > 0.0);
    let bottom = select(radii.w, radii.z, p.x > 0.0);
    let r      = min(select(bottom, top, p.y > 0.0), min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2(r, r);
    return length(max(q, vec2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// How much of a pixel `dist` from the shape's edge is inside it: 1 inside, 0
// outside, over a 1-pixel antialiased edge.
fn edge_alpha(dist: f32) -> f32 {
    return 1.0 - smoothstep(-1.0, 1.0, dist);
}
