//! [`ComponentSpec`] — the builder [`crate::Proteus::component`] takes.

use proteus_ui::{QuadState, StyleOverride};

use crate::handle::Handle;

/// Declares a component: its rest geometry, sparse per-state style overrides
/// (Phase A: "only the properties that change for a given state need to be
/// declared"), children, and whether it should be permanently baked.
#[derive(Debug, Clone, Default)]
pub struct ComponentSpec {
    pub(crate) geometry: QuadState,
    pub(crate) hover: Option<StyleOverride>,
    pub(crate) pressed: Option<StyleOverride>,
    pub(crate) focused: Option<StyleOverride>,
    pub(crate) disabled: Option<StyleOverride>,
    pub(crate) children: Vec<Handle>,
    pub(crate) bake: bool,
}

impl ComponentSpec {
    /// Start a new spec with `geometry` as the declared rest state — the
    /// shape returned to whenever the component isn't hovered, pressed,
    /// focused, or disabled, and the target `signal().set()` resolves to
    /// automatically.
    pub fn new(geometry: QuadState) -> Self {
        Self {
            geometry,
            ..Default::default()
        }
    }

    /// Style applied while the pointer hovers this component.
    pub fn hover(mut self, style: StyleOverride) -> Self {
        self.hover = Some(style);
        self
    }

    /// Style applied while this component is pressed.
    pub fn pressed(mut self, style: StyleOverride) -> Self {
        self.pressed = Some(style);
        self
    }

    /// Style applied while this component has focus.
    pub fn focused(mut self, style: StyleOverride) -> Self {
        self.focused = Some(style);
        self
    }

    /// Style applied while this component is disabled
    /// (`Handle`s don't carry a `disable()` toggle yet — attach
    /// `proteus_ui::component::Disabled` directly via `Proteus::world_mut()`
    /// until a `Handle`-level convenience lands).
    pub fn disabled(mut self, style: StyleOverride) -> Self {
        self.disabled = Some(style);
        self
    }

    /// Declare `child` as a child of this component — its `QuadState` is
    /// relative to this component's own, per M10's composition model.
    pub fn child(mut self, child: Handle) -> Self {
        self.children.push(child);
        self
    }

    /// Collapse this component and its children into a single permanent
    /// textured quad (M10.5). Requires `GpuContext`/`QuadPipeline` resources
    /// to be present in the world to actually run — a no-op (retried every
    /// frame) otherwise, matching `bake_system`'s own graceful-degradation
    /// contract.
    pub fn bake(mut self) -> Self {
        self.bake = true;
        self
    }

    /// True if any per-state style was declared — used by `Proteus::component`
    /// to decide whether to attach `InteractionDef` at all (avoids a
    /// permanently-no-op mini-transition firing on every hover/press of a
    /// component with no visual states).
    pub(crate) fn has_interaction_styles(&self) -> bool {
        self.hover.is_some()
            || self.pressed.is_some()
            || self.focused.is_some()
            || self.disabled.is_some()
    }
}
