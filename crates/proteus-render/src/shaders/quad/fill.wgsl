// ---------------------------------------------------------------------------
// The fill: the component's own color, from its texture, a crossfade from a
// second texture, and its tint.
// ---------------------------------------------------------------------------

// All three atlases hold premultiplied-alpha pixels: the main atlas is
// premultiplied on upload (`QuadPipeline::write_to_main_atlas`), the transition
// atlas is written by this shader's premultiplied output, and video is always
// opaque. Premultiplied pixels are what make the sampler's bilinear filtering
// correct where alpha varies between texels, such as a PNG's antialiased
// edges; filtering straight alpha gives dark or light fringes. Samples are
// un-premultiplied straight away, so the straight-alpha math below (tint,
// crossfade, border, shadow) is unchanged. For alpha 0 or 1 this does
// nothing.
fn unpremultiply(c: vec4<f32>) -> vec4<f32> {
    if c.a <= 0.0001 {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(c.rgb / c.a, c.a);
}

// The pixel at `uv` in the atlas `page` selects, straight alpha.
//
// The low bits of `page` select which atlas; for main_atlas the high bits
// select the page (array layer). Components without a texture sample the white
// block at the origin of main_atlas layer 0, so the color tint alone decides
// how they look, with no branch.
//
// textureSampleLevel (LOD 0) is used instead of textureSample because the
// branches vary per instance (non-uniform control flow). textureSample
// requires implicit derivatives, which are undefined in non-uniform control
// flow per the WGSL spec and fail on strict backends. Level 0 is always correct
// here: our atlases have exactly one mip level.
fn sample_atlas(page: u32, uv: vec2<f32>) -> vec4<f32> {
    let selector = page & ATLAS_SELECTOR_MASK;
    let layer    = page >> ATLAS_PAGE_SHIFT;

    var color: vec4<f32>;
    if selector == 0u {
        color = textureSampleLevel(main_atlas,       atlas_sampler, uv, layer, 0.0);
    } else if selector == 1u {
        color = textureSampleLevel(transition_atlas, atlas_sampler, uv, 0.0);
    } else {
        // selector == 2: the video
        color = textureSampleLevel(video_atlas,      atlas_sampler, uv, 0.0);
    }
    return unpremultiply(color);
}

// Where a pixel is within the component, from 0 to 1 across it, for sampling
// its texture.
//
// local_pos spans the inflated quad (including shadow/glow padding).
// half_size is the un-inflated entity half-extent.
// Mapping local_pos through half_size gives a 0..1 coordinate that
// covers exactly the entity bounds, independent of inflation amount.
// Clamping keeps shadow-region fragments from sampling outside [0,1].
//
// Y-axis flip: the world coordinate system has Y increasing UPWARD
// (top of entity = +half_size.y, bottom = -half_size.y), but the UV
// convention is Y-DOWN (top of texture = 0, bottom = 1).  Without the
// flip, texture content appears vertically inverted.  The quad mesh
// (QUAD_VERTICES in mesh.rs) encodes the same flip: the top-left vertex
// has position.y=+0.5 and uv.y=0.0; the bottom-left has position.y=-0.5
// and uv.y=1.0.
fn texture_position(local_pos: vec2<f32>, half_size: vec2<f32>) -> vec2<f32> {
    let safe_half    = max(half_size, vec2(0.5, 0.5)); // guard against 0-size entities
    let norm_uv_raw  = (local_pos + safe_half) / (safe_half * 2.0);
    return clamp(
        // X is unchanged; Y is flipped to convert from world-Y-up to UV-Y-down.
        vec2(norm_uv_raw.x, 1.0 - norm_uv_raw.y),
        vec2(0.0, 0.0),
        vec2(1.0, 1.0),
    );
}

// The component's own color at this pixel, straight alpha: its texture, faded
// from a start texture while `crossfade_t` is above 0, tinted by its color,
// and faded by its opacity and how much of the pixel is inside its edge.
//
// For the white pixel (uv_scale == 0) the texture position doesn't matter.
fn fill_color(
    local_pos: vec2<f32>,
    half_size: vec2<f32>,
    uv_params: vec4<f32>,
    atlas_page: u32,
    base_uv_params: vec4<f32>,
    base_atlas_page: u32,
    crossfade_t: f32,
    color: vec4<f32>,
    opacity: f32,
    edge: f32,
) -> vec4<f32> {
    let norm_uv       = texture_position(local_pos, half_size);
    let atlas_uv      = uv_params.xy      + norm_uv * uv_params.zw;
    let base_atlas_uv = base_uv_params.xy + norm_uv * base_uv_params.zw;

    var tex_color = sample_atlas(atlas_page, atlas_uv);

    // Crossfade: blend from the start image to the end image.
    // base_atlas_page selects the start image's atlas, independently of
    // atlas_page. It is usually the transition atlas, for a split's slices,
    // but fading an image into playing video needs the main atlas for the
    // start and the video atlas for the end, so the video keeps playing.
    // Skipped when crossfade_t == 0.0.
    if crossfade_t > 0.0 {
        let base_color = sample_atlas(base_atlas_page, base_atlas_uv);
        tex_color = mix(base_color, tex_color, crossfade_t);
    }

    // Color tint + opacity.
    // color.a tints the texture alpha independently; opacity is the whole-component multiplier.
    var fill = tex_color * color;
    fill.a *= opacity * edge;
    return fill;
}
