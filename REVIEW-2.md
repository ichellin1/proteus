# Proteus — Review 2: Effects & Video

*July 15, 2026. Scope: changes since commit `455787c` (M2–M9), focusing on the effects and video work and doc consistency. Same caveat as last time: static review only — sandbox has no Rust toolchain — but the tree is committed, clean, and was building on your machine.*

First, credit where due: nearly everything from the first review was addressed — lockfile committed, fmt clean, `WHITE_PIXEL_UV` constants at the texel center, `textureSampleLevel` with a spec-citing comment, the outer-border limitation documented in the shader, `Copy` on `TransitionConfig`. The test suite grew from ~30 to ~100+ tests. Good trajectory.

---

## Verdict on the approaches

**Effects (SDF shadow/glow in the main pass): the right call.** No extra render targets, no extra passes, no atlas traffic — the shadow is a smoothstep on a distance field the shader already computes, and geometry inflation makes the penumbra rasterize without a second quad. This is exactly the trade a TV-class GPU wants. Glow reusing the shadow slots with zero offset is a clever, legitimately free win. The issues below are integration bugs, not approach problems.

**Video (BYOV channel → texture upload): right for native, wrong to extend to web.** The bounded `sync_channel(2)` with drain-latest-per-frame is a clean backpressure design for a native decoder thread. But this path requires CPU RGBA frames; on the browser/TV side the equivalent must be `texImage2D` from an `HTMLVideoElement` (or the native video plane with punch-through), never frame bytes through WASM. The web shell has no video yet, so nothing is wrong — this is a flag planted before the API ossifies. The `VideoPlayer` marker itself is transport-agnostic, which is good design.

---

## Bugs

### 1. Shadow/glow distorts textured content (latent, will hit video first)
`vs_main` inflates the quad geometry when a shadow is active but does not compensate the UVs — `atlas_uv` still spans 0→1 across the *inflated* quad. For white-pixel quads (`uv_scale = [0,0]`) this is invisible, which is why every current test and demo entity passes. But a `VideoPlayer` entity uses `uv_scale = [1,1]`, so `VideoPlayer` + `DropShadow`/`Glow` renders the video zoomed-in and shifted toward the shadow direction. Same applies to any future image texture. Fix: compute the fragment's UV from the un-inflated local position (you already have `local_pos` and `half_size`) or counter-scale the UV per vertex by the inflation ratio. Add a regression test for the video+shadow combination — the existing three shadow tests only check instance-field population.

### 2. Video suspend/resume is broken — and PLANNING claims it's done
`suspended()` calls `suspend_video` (texture → 1×1 placeholder), but `resumed()` early-returns when `state` is already initialized, so `resume_video` is **never called**. After one background/foreground cycle the video is permanently black. Additionally, if any redraw fires while suspended, `consume_video_frame` uploads a 320×180 frame into the 1×1 texture — `debug_assert` panic in debug builds. PLANNING.md M9 DoD has "[x] Correct behavior on backgrounding … on foreground, video [resumes]" checked; it isn't true. Wire `resume_video` into `resumed()` (and pause draining while suspended), or uncheck the box.

### 3. Glow intensity is not clamped, despite docs saying it is
`effects.rs` documents "Values > 1.0 are clamped in the shader." No clamp exists anywhere: `collect.rs` uploads `color.a * intensity` raw, and the shader multiplies it straight into `shadow_alpha`, so intensity > 1 yields `out_a > 1` — an invalid alpha fed to the blender, with backend-dependent results. One-line fix in `collect.rs`: `.min(1.0)` (or clamp in the shader and make the doc true).

### 4. Shadow offset: docs say world-space, shader applies it pre-rotation
`DropShadow::offset` is documented as world-space pixels, but the SDF evaluates `local_pos - shadow_offset` in *pre-rotation local* space, and the inflation is pre-rotation too — so the shadow direction rotates with the entity. A card rotating during a morph has its shadow swing around it like a sundial instead of staying screen-down. Either rotate the offset into local space in the shader (`rotate(offset, -rotation)`) to honor the world-space contract, or re-document it as local-space. Given rotation only appears during transitions, the current behavior is visible at exactly the moment Proteus is supposed to look best.

## Dead code

### 5. `blur.rs` + `blur.wgsl` are orphaned and internally false
`blur.rs` is not declared in `proteus-render/src/lib.rs` — it is never compiled. It references `crate::BLUR_SHADER_SRC` (doesn't exist), `proteus_ui::Blur` and `BakedBlur` (don't exist), and `QuadPipeline::blur_atlas_view()` (doesn't exist). Its docs claim the blur atlas is "bind group 1, binding 3" — binding 3 is now `video_atlas`, so even the planned integration is stale. PLANNING honestly leaves M8.5 unchecked, but the in-tree file describes an integration that was never built. Delete both files (they're in git history) or finish the milestone; if finished, the binding-3 collision with video must be resolved first, and note the kernel undersamples — 7 taps spaced a full σ apart will band at radius ≳ 10px; tap per texel or downsample-blur-upsample instead.

## Design notes

### 6. Shadow XOR Glow will bite the TV use case specifically
The slot-sharing makes `DropShadow` and `Glow` mutually exclusive per entity. The single most common TV pattern — a focused card — conventionally wants both: shadow for elevation, glow for focus. Options that avoid new attributes: emit a second instance for the glow layer in `collect_instances` (cheap, uses existing slots), or accept the limitation and implement focus treatment as border + glow. Worth deciding before the SDK API freezes.

### 7. Vertex attribute budget is now exhausted-adjacent
15 of 16 attribute locations are used (2 vertex + 13 instance). The next effect that needs per-instance data cannot have a new attribute. Plan the migration now — likely a per-instance storage buffer indexed by `instance_index` (works on WebGL2 via uniform array fallback, or pack rarely-animated params into a texture).

### 8. Video atlas is sRGB; every other atlas is linear Unorm
`create_video_texture` uses `Rgba8UnormSrgb` while `main_atlas`/`transition_atlas` are `Rgba8Unorm`. The same RGBA values will display differently depending on whether they arrive as a video frame or a quad color/baked texture. The M9-known-limitation "morph poster-image → playing video" is exactly where this shows up as a brightness pop at the crossfade endpoints. Pick one convention (sRGB-in/sRGB-out is the defensible one) and apply it to all atlases together.

## Doc inconsistencies

- **README Status is two eras stale** — still says "CI setup is the remaining M0 exit criterion. M1 (First Pixel) is next." That was item 7 last review; M7, M8, M8.5, M9 have since shipped. ROADMAP's "M0 — Foundation *(in progress)*" likewise. If keeping a status section is a chore, replace it with a one-liner pointing at the git log.
- **PLANNING M9 DoD overclaims**: the backgrounding box (bug 2 above), and "[x] No frame drops in the video during or after a transition" — drain-latest *drops frames by design*, and no test measures this. Reword to what's actually guaranteed (latest-frame-wins, bounded memory).
- **`effects.rs` Glow doc** says radius "controls the Gaussian sigma (sigma ≈ radius / 3)" — that's `blur.wgsl` language; the shipped glow is a smoothstep SDF falloff with `softness = radius`. No Gaussian is involved.
- **`proteus-ui/src/lib.rs`** still promises "M3 adds Entering/Leaving" lifecycle states; M3 shipped using `Virtual` entities instead and `Lifecycle` remains two-state.
- **PLANNING M8 approach text** says "shadow quad rendered before the main shape in the same draw call" — there is no separate shadow quad; it's one inflated quad composited in the fragment shader. Minor, but it describes an architecture you didn't build.

---

## Suggested order

1. Clamp glow intensity (one line) and fix shadow UV compensation + add video+shadow test (bug 1, 3).
2. Fix or de-claim video resume (bug 2) — decide now since suspend/resume is the *normal* lifecycle on TV platforms.
3. Delete `blur.rs`/`blur.wgsl` or schedule M8.5 properly.
4. Settle the sRGB convention before the poster→video morph lands (item 8).
5. Doc sweep: README/ROADMAP status, PLANNING checkboxes, glow doc, shadow offset semantics per your bug-4 decision.
