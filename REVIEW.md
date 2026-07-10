# Proteus — Project Review

*July 10, 2026. Full-repo review: code, build/CI health, docs, repo hygiene.*

Overall the project is in good shape. The architecture is clean and well-layered, the code is unusually well-commented, tests are meaningful (including a real headless GPU pixel-readback test), and CI is sensibly designed. The issues below are ordered by severity. One note on verification: this review environment could not run `cargo` (no network access to rustup), so findings are from static analysis plus inspection of your local build artifacts — the workspace did compile on your machine today, including the test binaries. Run `make check` locally to confirm items 1–2.

---

## High priority

### 1. `cargo fmt --check` will fail — which fails CI and the pre-push hook
Three lines exceed rustfmt's 100-char limit in the new (uncommitted) M2 code:

- `crates/proteus-ui/src/transition.rs:337` (104 chars)
- `crates/proteus-ui/tests/transition_systems.rs:202` (107 chars)
- `crates/proteus-ui/tests/transition_systems.rs:302` (102 chars)

Several other lines in `transition_systems.rs` (inline struct literals in `spawn()` tuples) will also likely be rewrapped. **Fix:** run `cargo fmt --all` before committing. Since CI's first step is the fmt check, pushing as-is turns CI red.

### 2. The entire M2 transition system is uncommitted
`git status` shows the core of the recent work is not in version control: modified `transition.rs`, `component.rs`, `lib.rs`, `proteus-ui/Cargo.toml`, plus untracked `schedule.rs`, both test directories (`proteus-ui/tests/`, `proteus-render/tests/`), the `Makefile`, and `scripts/` (the git hooks). If anything happens to this working tree, M2 and the tests are gone. The commit `455787c test(render): headless integration test` exists, yet `crates/proteus-render/tests/` itself is untracked — so that commit's message doesn't match what it actually contains. **Fix:** format (item 1), then commit.

### 3. `Cargo.lock` is gitignored — but CI depends on it
`.gitignore` excludes `Cargo.lock`, yet `ci.yml` keys both caches on `hashFiles('**/Cargo.lock')`. With no lockfile in the repo the hash is empty, so the cache key is the constant `ubuntu-cargo-` — the cache never invalidates when dependencies change, and every CI run resolves dependencies fresh, meaning an upstream semver-compatible release can break CI at any time. The workspace also ships a binary (`proteus-shell-native`), for which committing the lockfile is the standard recommendation (and current Cargo guidance is to commit it even for libraries). **Fix:** remove `Cargo.lock` from `.gitignore` and commit it.

### 4. Outer borders cannot render (shader design flaw)
In `quad.wgsl`, the border band for `border_offset = 1.0` (outer) lies entirely outside the rect edge (`dist > 0`), but:

- the quad geometry is exactly rect-sized, so fragments beyond the edge are never rasterized (except inside rounded-corner cutouts), and
- `border_alpha` is multiplied by `edge_alpha`, which goes to 0 right at the edge, and fragments with `edge_alpha <= 0` are `discard`ed before the border code runs.

Net effect: `border_offset = 0.0` renders only the inner half of the border; `1.0` renders essentially nothing. Either inflate the vertex geometry by the border's outer extent (and widen the SDF accordingly), or document that only inner borders (`-1.0`) are supported for now.

## Medium priority

### 5. `textureSample` under non-uniform control flow (portability risk)
`fs_main` calls `textureSample` inside `if in.atlas_page == 0u { … } else { … }` and inside `if in.crossfade_t > 0.0 { … }`. Both conditions vary per instance, so this is non-uniform control flow, where implicit-derivative sampling is undefined per the WGSL spec. It evidently validates and works on your Metal setup, but it's a portability/conformance hazard (WebGL2 via naga's GLSL backend is exactly where strictness varies). Since the atlases have one mip level, the cheap fix is `textureSampleLevel(…, 0.0)`, which is legal in non-uniform control flow; alternatively sample both atlases unconditionally and `select()`.

### 6. Docs contradict the code in several places
- `component.rs` (`QuadState::lerp`): comment says rotation is "interpolated on the shortest path" — it's a direct lerp; 350°→10° takes the long way around. Fix the comment (or the code).
- `proteus-ui/src/lib.rs`: calls `Lifecycle` a "four-state machine" — it has two states (`Idle`, `Transitioning`).
- `transition.rs:19`: comment says the fn-pointer keeps `TransitionConfig` "Copy + Clone" — it only derives `Clone`. (It could derive `Copy`; that would also let you drop two `.clone()` calls in `transition_setup_system`.)
- `quad.wgsl:112` and the `transform_top_left_anchor` test in `mesh.rs`: anchor `[0,0]` is described as "top-left", but with the documented Y-up ortho projection it's bottom-left. Pick a convention and make comments consistent.
- `mesh.rs`/`component.rs` say "Z controls depth; higher = on top", but the pipeline sets `depth_stencil: None`, so Z currently does nothing except get clipped outside 0–1000. Draw order is what actually determines stacking. Worth a "(future)" caveat.

### 7. README status is stale
README says "CI setup is the remaining M0 exit criterion. M1 (First Pixel) is next" — but CI is committed, the M1 first-pixel commit landed, and M2 (transitions) is substantially built. Update the Status section when you commit M2.

### 8. Dead code / unused dependencies
- `proteus_gpu::GpuContext` is used by nothing — the native shell duplicates its adapter/device logic inline (with `compatible_surface: Some(&surface)`, which `GpuContext` can't do since it takes no surface). Either give `GpuContext` a surface parameter and use it, or note it's not yet wired up.
- Unused deps per crate: `proteus-ui` → `proteus-render`, `serde`, `thiserror`, `anyhow`, `log` (the `proteus-render` dep needlessly pulls all of wgpu into UI-only builds); `proteus-gpu` → `bytemuck`, `glam`, `anyhow`, `pollster`; `proteus-shell-native` → `proteus-gpu`, `proteus-ui`; `proteus-shell-web` → `proteus-render`, `proteus-ui`, `js-sys`, `web-sys`, `anyhow`. Some are "will use soon," but pruning until needed keeps builds fast. `cargo +nightly udeps` or `cargo machete` automates this.

### 9. Transition-system edge cases
- `transition_setup_system`'s query requires `&Lifecycle`, so an entity with a `TransitionRequest` but no `Lifecycle` component is silently ignored forever. Use `Option<&Lifecycle>` or drop it from the query (the value is never read).
- A non-positive `duration` (e.g. from a config file later) makes `raw_t` clamp to 0 permanently — the entity is stuck in `Transitioning` with an `ActiveTransition` that never completes. (`duration == 0.0` happens to work via `inf.clamp()`, but negative values don't.) Validate or clamp duration in `ActiveTransition::new`.
- `CompletedTransitions` is cleared at the top of `transition_complete_system`, so completions are lost if the shell doesn't drain every frame. This is documented, but a `std::mem::take`-style drain API would make misuse harder.

## Low priority

### 10. White-pixel UV magic number and edge bleed
`1.0 / 2048.0` is hardcoded in `proteus-shell-native/src/main.rs` and `headless_render.rs`, duplicating the private `DEFAULT_MAIN_ATLAS_SIZE` in `pipeline.rs` — if the atlas size changes, both call sites silently break. Also, spanning the full texel (UV 0→1/2048) makes linear filtering blend with neighboring zero-initialized texels at the quad edges (the headless test comment acknowledges this). Expose a `QuadPipeline::WHITE_PIXEL_UV` helper that points at the texel *center* (`0.5/2048`) with `uv_scale = [0,0]`.

### 11. `.gitignore` `/pkg/` doesn't match wasm-pack output
The leading slash anchors `/pkg/` to the repo root, but `wasm-pack build crates/proteus-shell-web` writes to `crates/proteus-shell-web/pkg/`. Use unanchored `pkg/` (same consideration for `dist/`).

### 12. Headless render test skips silently without a GPU
If no adapter is found the test prints a warning and passes. On CI, if the lavapipe install step ever breaks, the pixel test would silently stop testing anything. Consider an env var (e.g. `REQUIRE_GPU=1` set in ci.yml) that turns the skip into a failure on CI.

### 13. Minor
- The native shell requests redraws from both `about_to_wait` and the end of `render()` — redundant (harmless; pick one).
- `proteus-ui` isn't wired into the native shell yet: `ProteusWorld` and the transition systems run only in tests, while the shell renders a hardcoded quad. Expected at this milestone, but it means the M2 render-system integration (ECS → instance buffer) is the real remaining M2 work.
- `docs/` is empty and untracked (git doesn't track empty dirs); remove it or add content.
- CI caches the whole `target/` directory keyed as in item 3 — it will grow stale and unbounded. `Swatinem/rust-cache` handles this better with zero config.

---

## Suggested immediate sequence

1. `cargo fmt --all`, then `make check` (fmt + clippy + tests) on your machine.
2. Commit the M2 work (items 1–2).
3. Un-ignore and commit `Cargo.lock` (item 3).
4. Fix the shader border/sampling issues or document the limitations (items 4–5).
5. Sweep the doc-comment mismatches and README status (items 6–7).
