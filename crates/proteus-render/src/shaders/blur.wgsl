// =============================================================================
// blur.wgsl — Separable Gaussian blur pass (M8.5)
//
// Used twice per blurred entity: once with direction=[1,0] (horizontal),
// once with direction=[0,1] (vertical).
//
// No vertex buffer — uses the full-screen-triangle trick (3 hard-coded vertices
// driven by @builtin(vertex_index) with no VBO).
//
// Bind group 0:
//   binding 0: Uniforms { direction: vec2<f32>, radius: f32, _pad: f32 }
//   binding 1: src texture (bake_src for H pass, bake_h for V pass)
//   binding 2: sampler (linear, clamp-to-edge)
// =============================================================================

struct Uniforms {
    /// [1.0, 0.0] for horizontal pass, [0.0, 1.0] for vertical pass.
    direction: vec2<f32>,
    /// Blur radius in pixels.  Gaussian sigma = radius / 3.0; the furthest
    /// sample tap is at ±radius texels from the centre.
    radius: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform>  u:       Uniforms;
@group(0) @binding(1) var           src:     texture_2d<f32>;
@group(0) @binding(2) var           src_smp: sampler;

// ---------------------------------------------------------------------------
// Vertex stage — full-screen triangle, no VBO
// ---------------------------------------------------------------------------

struct VertexOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
    // Three vertices that together cover the NDC square [-1,1]×[-1,1].
    //
    //   vi=0: NDC (-1, -1)  UV (0, 1)   bottom-left
    //   vi=1: NDC ( 3, -1)  UV (2, 1)   far right (clip-space, outside viewport)
    //   vi=2: NDC (-1,  3)  UV (0, -1)  far top   (clip-space, outside viewport)
    //
    // UV convention: V=0 at top, V=1 at bottom (standard texture coords).
    // At NDC Y=+1 (top edge)  → V=0; at NDC Y=-1 (bottom edge) → V=1.
    var clip = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    var texc = array<vec2<f32>, 3>(
        vec2(0.0, 1.0),
        vec2(2.0, 1.0),
        vec2(0.0, -1.0),
    );
    var out: VertexOut;
    out.pos = vec4(clip[vi], 0.0, 1.0);
    out.uv  = texc[vi];
    return out;
}

// ---------------------------------------------------------------------------
// Fragment stage — 7-tap separable Gaussian (sigma ≈ radius/3)
// ---------------------------------------------------------------------------
//
// Gaussian weights at offsets 0, ±1, ±2, ±3 (in sigma units, sigma = 1):
//   exp(-x²/2) evaluated and normalised so the seven taps sum to 1.
//
// Unnormalised:  1.0, 0.6065, 0.1353, 0.0111
// Sum of all 7:  1.0 + 2*(0.6065+0.1353+0.0111) = 2.5058
// Normalised:    W0=0.3991, W1=0.2420, W2=0.0540, W3=0.0044

const W0: f32 = 0.3991;
const W1: f32 = 0.2420;
const W2: f32 = 0.0540;
const W3: f32 = 0.0044;

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(src));
    // step: one sigma in texture-coordinate space.  Samples are at ±1,±2,±3 sigma.
    let step = u.direction * (u.radius / 3.0) / tex_size;

    var c  = textureSample(src, src_smp, in.uv           ) * W0;
    c     += textureSample(src, src_smp, in.uv + 1.0*step) * W1;
    c     += textureSample(src, src_smp, in.uv - 1.0*step) * W1;
    c     += textureSample(src, src_smp, in.uv + 2.0*step) * W2;
    c     += textureSample(src, src_smp, in.uv - 2.0*step) * W2;
    c     += textureSample(src, src_smp, in.uv + 3.0*step) * W3;
    c     += textureSample(src, src_smp, in.uv - 3.0*step) * W3;
    return c;
}
