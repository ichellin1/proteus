# Rounded corners on the slices of a baked split or merge

**Status:** done in v0.2.0, with a radius per corner and square bakes; see steps 3 and 8 of
[v0.2.0's planning](../archive/PLANNING-v0.2.0.md), and ARCHITECTURE's
[Splits and merges](../../ARCHITECTURE.md#splits-and-merges). A
rounded component with a border is still baked with its rounded corners, which show in its
slices; drawing its border on the pieces instead is a candidate in ROADMAP, for v0.6.0.

In a split, each virtual component fades from its slice of the source's bake to its target's
bake (a merge is the reverse; see [ARCHITECTURE.md](../../ARCHITECTURE.md#splits-and-merges)).
The two ends treat corners differently, in `one_to_n_setup_system` and `n_to_one_setup_system`
(`crates/proteus-ui/src/topology.rs`):

- **The target's end** is a whole component, so it uses the target's real corner radius. Its
  corners stay round however small it gets.
- **The slice's end** has a corner radius of 0. Only slices at the source's corners should be
  rounded, and the bake already contains the source's rounded edges, so the slice shows them as
  they were baked.

The atlases have no mipmaps (the shader samples level 0), so when a virtual is drawn much
smaller than its bake, which is routine partway through a transition, the bake's antialiased
edge is undersampled and the rounding aliases toward square. Seen on the reference demo's
video tiles and the gallery's grid transitions. The demo's workaround is a smaller corner
radius, so the aliasing is less noticeable.

## Approaches

- **A rounded-rectangle mask generated per shared bake,** sized to it, and sampled alongside the
  bake instead of relying on the bake's own edges. Needs a mask atlas, or space in an existing
  one, and a way to address it per instance in the shader, whose vertex attributes are all in
  use (see [ARCHITECTURE.md](../../ARCHITECTURE.md#the-instance-and-its-limits)).
- **Give each slice the corners it should have:** a slice at the source's corner gets that
  corner's radius, and the others none. Needs a radius per corner, which the instance doesn't
  have.
- **Mipmaps for bakes,** so a small virtual samples a smaller, properly filtered image. Costs a
  third more atlas space and a mip pass after each bake.
