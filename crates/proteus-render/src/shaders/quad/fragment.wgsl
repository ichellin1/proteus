// ---------------------------------------------------------------------------
// The fragment stage: each pixel, from the shape, fill and effects in turn.
// ---------------------------------------------------------------------------

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Most components have no border, shadow, glow or crossfade, and most of
    // those have no texture either. They take a shorter path, with the same
    // result as the full one below.
    if in.border_width <= 0.0 && in.shadow_color.a <= 0.0 && in.crossfade_t <= 0.0 {
        return without_effects(in);
    }

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

// A pixel of a component with no border, shadow, glow or crossfade: the full
// path in `fs_main`, without the steps that change nothing for it. A component
// with no texture samples the white block, which is exactly 1, so its color is
// its own, without sampling at all.
fn without_effects(in: VertexOut) -> vec4<f32> {
    let edge = edge_alpha(sdf_rounded_rect(in.local_pos, in.half_size, in.corner_radii));
    if edge <= 0.0 {
        discard;
    }

    var fill = in.color;
    let white_block = in.atlas_page == 0u && all(in.uv_params == vec4(0.0));
    if !white_block {
        let norm_uv = texture_position(in.local_pos, in.half_size);
        fill = sample_atlas(in.atlas_page, in.uv_params.xy + norm_uv * in.uv_params.zw) * in.color;
    }
    fill.a *= in.opacity * edge;
    return over_shadow(fill, 0.0, in.shadow_color.rgb);
}
