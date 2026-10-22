// ---------------------------------------------------------------------------
// Effects: the shadow or glow under the shape, the border over its fill, and
// compositing the two.
// ---------------------------------------------------------------------------

// The shadow or glow's alpha at `local_pos`, before it's masked by the shape
// or faded by opacity: 0 when `shadow_color.a` is 0, which turns it off.
//
// The shadow shape is the main rounded rect, expanded by spread and shifted by
// the shadow offset. A higher softness value widens the penumbra. A glow is a
// shadow with no offset.
fn shadow_alpha(
    local_pos: vec2<f32>,
    half_size: vec2<f32>,
    radii: vec4<f32>,
    shadow_params: vec4<f32>, // (offset_x, offset_y, softness, spread)
    shadow_color: vec4<f32>,
) -> f32 {
    if shadow_color.a <= 0.0 {
        return 0.0;
    }
    let shadow_offset   = shadow_params.xy;
    let shadow_softness = shadow_params.z;
    let shadow_spread   = shadow_params.w;
    let shadow_half_size = half_size + vec2(shadow_spread, shadow_spread);
    let shadow_radii     = radii + vec4(shadow_spread);
    let shadow_dist      = sdf_rounded_rect(local_pos - shadow_offset, shadow_half_size, shadow_radii);
    // Clamp softness so smoothstep never has a zero-width range.
    let softness   = max(shadow_softness, 0.5);
    return (1.0 - smoothstep(-softness, softness, shadow_dist)) * shadow_color.a;
}

// `fill` with the border drawn over it, `dist` from the shape's edge.
//
// LIMITATION: only inner borders (border_offset = -1.0) render correctly.
// border_offset = 0.0 (centered) shows only the inner half of the band.
// border_offset = 1.0 (outer) renders nothing: fragments beyond the rect edge
// are never rasterized, and edge_alpha goes to zero right at the boundary.
// Full outer-border support requires inflating the quad geometry by the border's
// outer extent so outer fragments are actually rasterized.
fn with_border(
    fill: vec4<f32>,
    dist: f32,
    edge: f32,
    border_width: f32,
    border_color: vec4<f32>,
    border_offset: f32,
    opacity: f32,
) -> vec4<f32> {
    let half_w = border_width * 0.5;
    // border_offset shifts the border band relative to the shape edge:
    //   -1.0 = fully inside, 0.0 = centered on edge, 1.0 = fully outside (broken, see above)
    let border_center = border_offset * half_w;
    let border_dist   = abs(dist - border_center) - half_w;
    let border_alpha  = (1.0 - smoothstep(-1.0, 1.0, border_dist)) * edge;

    // Composite the border over the fill: Porter-Duff "over", straight
    // alpha, as for the shadow below. Over an opaque fill this is
    // `mix(fill, border, b_a)`; over a transparent or translucent
    // fill, the border keeps its own alpha.
    let b     = border_color;
    let b_a   = b.a * border_alpha * opacity;
    let out_a = b_a + fill.a * (1.0 - b_a);
    var out_rgb = vec3<f32>(0.0);
    if out_a > 0.0 {
        out_rgb = (b.rgb * b_a + fill.rgb * fill.a * (1.0 - b_a)) / out_a;
    }
    return vec4(out_rgb, out_a);
}

// `main_color` over the shadow, in premultiplied alpha.
//
// Porter-Duff "over" (straight alpha): main_color over shadow.
//   result.a   = main.a + shadow.a * (1 − main.a)
//   result.rgb = (main.rgb × main.a + shadow.rgb × shadow.a × (1 − main.a)) / result.a
//
// With no shadow (shadow_alpha == 0) this returns main_color unchanged.
//
// The result is premultiplied, to match the pipeline's blend state
// (`PREMULTIPLIED_ALPHA_BLENDING`, see `pipeline.rs::build_render_pipeline`)
// and the atlases. For an opaque fragment it is the same as straight
// alpha.
fn over_shadow(main_color: vec4<f32>, shadow: f32, shadow_rgb: vec3<f32>) -> vec4<f32> {
    let src_a = main_color.a;
    let dst_a = shadow;
    let out_a = src_a + dst_a * (1.0 - src_a);
    var out_rgb = vec3<f32>(0.0);
    if out_a > 0.0 {
        out_rgb = (main_color.rgb * src_a + shadow_rgb * dst_a * (1.0 - src_a)) / out_a;
    }
    return vec4(out_rgb * out_a, out_a);
}
