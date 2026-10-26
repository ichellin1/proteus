// =============================================================================
// The Proteus shader, which draws every visible component in one instanced
// draw call. It's split into named parts, joined in order into one shader by
// `QUAD_SHADER_SRC` in lib.rs; WGSL allows a declaration to be used before it's
// declared, so the order is only for reading:
//
//   bindings.wgsl  — the uniforms, the atlases and their sampler
//   vertex.wgsl    — the instance's layout, and placing each quad
//   shape.wgsl     — the rounded rectangle and its antialiased edge
//   fill.wgsl      — the component's own color: its texture, crossfade and tint
//   effects.wgsl   — the shadow or glow, the border, and compositing them
//   fragment.wgsl  — each pixel, from those parts in turn
//   plain.wgsl     — the plain pipeline's entry points, for a solid color
//
// Bind group 0:  uniform buffer   — view/projection matrix (one upload per frame)
// Bind group 1:  main_atlas       — long-lived textures (text, images, baked components);
//                                   an array texture with one layer per page (see ATLAS_PAGE_SHIFT)
//                transition_atlas — ephemeral bakes for in-flight transitions
//                atlas_sampler    — shared linear sampler
//                video_atlas      — the video, updated each frame
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
@group(1) @binding(0) var main_atlas:       texture_2d_array<f32>;
@group(1) @binding(1) var transition_atlas: texture_2d<f32>;
@group(1) @binding(2) var atlas_sampler:    sampler;
@group(1) @binding(3) var video_atlas:      texture_2d<f32>;

// ---------------------------------------------------------------------------
// atlas_page / base_atlas_page bit layout — mirrored from mesh.rs's
// `pack_atlas_page`/`unpack_atlas_page`. `wgsl_atlas_page_bit_layout_matches_rust`
// (mesh.rs) parses this shader and fails the tests if the values ever drift apart.
//
//   bits  0..7  — which atlas: 0 = main_atlas, 1 = transition_atlas, 2 = video_atlas
//   bits  8..31 — which main_atlas array layer ("page"). Meaningful only when the selector
//                 is 0; transition_atlas and video_atlas are single-layer textures.
//
// Packed, rather than a 17th vertex attribute, because all 16 locations are in use (see
// mesh.rs's QuadInstance::buffer_layout). pack(selector, 0) == selector.
// ---------------------------------------------------------------------------
const ATLAS_SELECTOR_MASK: u32 = 0xFFu;
const ATLAS_PAGE_SHIFT:    u32 = 8u;
