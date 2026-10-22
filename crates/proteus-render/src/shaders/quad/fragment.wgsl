// ---------------------------------------------------------------------------
// The fragment stage: each pixel, from the shape, fill and effects in turn.
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // The shadow, computed first so that shadow-only fragments outside the
    // main shape aren't discarded.
    var shadow = shadow_alpha(in.local_pos, in.half_size, in.corner_radii, in.shadow_params, in.shadow_color);

    // dist < 0 inside the rounded rect, > 0 outside.
    let dist = sdf_rounded_rect(in.local_pos, in.half_size, in.corner_radii);
    let edge = edge_alpha(dist);

    // A shadow or glow should only extend outward from the shape, not fill
    // it. But the shadow's alpha stays roughly constant deep inside the shape
    // (always, for a glow with no spread), which shows through wherever a
    // texture is transparent inside the shape, such as gaps in an image.
    // Masking it with the same 1-pixel edge as `edge` removes it inside
    // and leaves the outside unchanged.
    shadow *= smoothstep(-1.0, 1.0, dist);

    // Opacity fades the whole component: the fill below, the border, and the
    // shadow or glow here.
    shadow *= in.opacity;

    // Discard fragments where neither the shadow nor the main shape contribute.
    if edge <= 0.0 && shadow <= 0.0 {
        discard;
    }

    // The fill and border, only for fragments inside or on the edge of the
    // main shape. A border is skipped entirely when its width is 0.
    var main_color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if edge > 0.0 {
        main_color = fill_color(
            in.local_pos,
            in.half_size,
            in.uv_params,
            in.atlas_page,
            in.base_uv_params,
            in.base_atlas_page,
            in.crossfade_t,
            in.color,
            in.opacity,
            edge,
        );
        if in.border_width > 0.0 {
            main_color = with_border(
                main_color,
                dist,
                edge,
                in.border_width,
                in.border_color,
                in.border_offset,
                in.opacity,
            );
        }
    }

    return over_shadow(main_color, shadow, in.shadow_color.rgb);
}
