//! [`ComponentSpec`] — the builder [`crate::Proteus::component`] takes.

use proteus_ui::{Border, DropShadow, Glow, Image, QuadState, StyleOverride, Text};

use crate::handle::Handle;

/// Declares a component: its rest geometry, sparse per-state style overrides
/// (Phase A: "only the properties that change for a given state need to be
/// declared"), children, whether it should be permanently baked, and any of
/// the visual/content components (`Text`/`Image`/`Border`/`Glow`/
/// `DropShadow`) it should carry from the moment it's spawned.
#[derive(Debug, Clone, Default)]
pub struct ComponentSpec {
    pub(crate) geometry: QuadState,
    pub(crate) hover: Option<StyleOverride>,
    pub(crate) pressed: Option<StyleOverride>,
    pub(crate) focused: Option<StyleOverride>,
    pub(crate) disabled: Option<StyleOverride>,
    pub(crate) children: Vec<Handle>,
    pub(crate) bake: bool,
    pub(crate) text: Option<Text>,
    pub(crate) image: Option<Image>,
    pub(crate) border: Option<Border>,
    pub(crate) glow: Option<Glow>,
    pub(crate) drop_shadow: Option<DropShadow>,
    pub(crate) non_interactive: bool,
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

    /// Render a single line of text on this component (M4). Rasterized and
    /// baked into `main_atlas` by whichever bake path the host application
    /// drives — `proteus-sdk` doesn't do this itself yet (see
    /// `proteus_ui::text`'s module doc: baking is currently a shell
    /// responsibility, not a scheduled system).
    pub fn text(mut self, text: Text) -> Self {
        self.text = Some(text);
        self
    }

    /// Attach a static image (M9.7), given its already-loaded bytes. Like
    /// `.text()`, actually decoding/baking the bytes into `main_atlas` is
    /// driven by the host application, not `proteus-sdk` itself.
    pub fn image(mut self, image: Image) -> Self {
        self.image = Some(image);
        self
    }

    /// Draw an SDF-based border around this component.
    pub fn border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    /// Draw a soft radial glow behind this component. Mutually exclusive
    /// with `.drop_shadow()` at the shader level — if both are set, the
    /// drop shadow wins (matches `proteus_ui::effects`'s existing behavior;
    /// this builder doesn't add its own validation on top of that).
    pub fn glow(mut self, glow: Glow) -> Self {
        self.glow = Some(glow);
        self
    }

    /// Draw an SDF-based drop shadow behind this component. See `.glow()`
    /// for the mutual-exclusivity note.
    pub fn drop_shadow(mut self, shadow: DropShadow) -> Self {
        self.drop_shadow = Some(shadow);
        self
    }

    /// Opts this component out of `Interactable` entirely.
    /// `Proteus::component` attaches it to everything by default (so
    /// `.on_click`/etc. "just work" without a separate opt-in), which is
    /// wrong for passive chrome like a full-window background: hit-testing
    /// resolves overlapping candidates by "last hit wins, matches draw
    /// order" (`proteus_ui::input::hit_test_system`'s own doc), so a quad
    /// spanning the whole viewport — visited late enough in the query's
    /// iteration order — can silently swallow clicks meant for a real
    /// button underneath the cursor.
    pub fn non_interactive(mut self) -> Self {
        self.non_interactive = true;
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
