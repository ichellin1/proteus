//! [`ComponentSpec`], the builder passed to [`Proteus::component`](crate::Proteus::component).

use proteus_ui::{
    Border, DropShadow, Glow, Image, QuadState, StyleOverride, Text, TransitionInteractionConfig,
};

use crate::handle::Handle;

/// A description of a component, passed to
/// [`Proteus::component`](crate::Proteus::component) to create it.
///
/// Starts from the component's declared geometry, then adds any of: styles
/// for interaction states, children, content (text or an image), effects,
/// and initial visibility, opacity and input settings. Interaction styles are
/// sparse: declare only the fields that change in that state.
///
/// # Examples
///
/// ```
/// use glam::{Vec2, Vec3, Vec4};
/// use proteus_sdk::{ComponentSpec, Proteus, QuadState, StyleOverride};
///
/// let mut app = Proteus::new();
/// let button = app.component(
///     ComponentSpec::new(QuadState {
///         position: Vec3::new(200.0, 120.0, 0.0),
///         size: Vec2::new(160.0, 48.0),
///         color: Vec4::new(0.2, 0.4, 0.9, 1.0),
///         corner_radius: 8.0,
///         ..QuadState::default()
///     })
///     .hover(StyleOverride {
///         scale: Some(1.05),
///         ..StyleOverride::default()
///     }),
/// );
/// assert!(app.get(button).is_some());
/// ```
#[derive(Debug, Clone)]
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
    pub(crate) visible: bool,
    pub(crate) opacity: Option<f32>,
    pub(crate) start_disabled: bool,
    pub(crate) transition_interaction: Option<TransitionInteractionConfig>,
}

impl Default for ComponentSpec {
    fn default() -> Self {
        Self {
            geometry: QuadState::default(),
            hover: None,
            pressed: None,
            focused: None,
            disabled: None,
            children: Vec::new(),
            bake: false,
            text: None,
            image: None,
            border: None,
            glow: None,
            drop_shadow: None,
            non_interactive: false,
            // The only field whose default is "on" rather than "absent".
            visible: true,
            opacity: None,
            start_disabled: false,
            transition_interaction: None,
        }
    }
}

impl ComponentSpec {
    /// Creates a spec with `geometry` as the component's declared geometry.
    ///
    /// The declared geometry is what the component shows when no
    /// interaction style applies, and where a transition into this component
    /// lands.
    pub fn new(geometry: QuadState) -> Self {
        Self {
            geometry,
            ..Default::default()
        }
    }

    /// Sets the opacity of this component and everything under it, clamped
    /// to `0.0..=1.0`. Defaults to `1.0`.
    ///
    /// Opacity multiplies down the hierarchy: a child at `0.6` under a
    /// parent at `0.6` is drawn at `0.36`.
    ///
    /// Opacity only affects drawing. A component at `0.0` is invisible but
    /// still receives pointer input; use [`ComponentSpec::visible`] to take
    /// it out of input as well.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity.clamp(0.0, 1.0));
        self
    }

    /// Creates the component in the disabled state: drawn, but ignoring all
    /// input and showing its [`ComponentSpec::disabled`] style.
    ///
    /// Use this for a control that is enabled later with
    /// [`Handle::set_disabled`]. For something that is never a control,
    /// such as a background or a label, use
    /// [`ComponentSpec::non_interactive`] instead.
    pub fn start_disabled(mut self) -> Self {
        self.start_disabled = true;
        self
    }

    /// Sets whether this component accepts input while it is transitioning.
    ///
    /// Without this, a transitioning component ignores input.
    /// `allow_navigation` is not read yet; it is reserved for keyboard
    /// navigation.
    pub fn transition_interaction(mut self, config: TransitionInteractionConfig) -> Self {
        self.transition_interaction = Some(config);
        self
    }

    /// Sets whether the component starts visible. Defaults to `true`.
    ///
    /// A hidden component is neither drawn nor hit-tested. A transition into
    /// it through [`TransitionChannel::set`](crate::TransitionChannel::set) reveals it, so a component that
    /// should first appear through a transition can start hidden.
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets the style applied while the pointer is over this component.
    pub fn hover(mut self, style: StyleOverride) -> Self {
        self.hover = Some(style);
        self
    }

    /// Sets the style applied while this component is pressed.
    pub fn pressed(mut self, style: StyleOverride) -> Self {
        self.pressed = Some(style);
        self
    }

    /// Sets the style applied while this component has focus.
    pub fn focused(mut self, style: StyleOverride) -> Self {
        self.focused = Some(style);
        self
    }

    /// Sets the style applied while this component is disabled.
    ///
    /// See [`ComponentSpec::start_disabled`] and [`Handle::set_disabled`].
    pub fn disabled(mut self, style: StyleOverride) -> Self {
        self.disabled = Some(style);
        self
    }

    /// Adds `child` as a child of this component.
    ///
    /// The child's geometry is relative to this component's, so it moves,
    /// scales and fades with it.
    pub fn child(mut self, child: Handle) -> Self {
        self.children.push(child);
        self
    }

    /// Bakes this component and its children into a single texture,
    /// permanently.
    ///
    /// The host renders the subtree once and then destroys the children, so
    /// their handles become stale. Suited to detailed content that never
    /// changes. Baking happens the next time a host renders a frame.
    pub fn bake(mut self) -> Self {
        self.bake = true;
        self
    }

    /// Draws a single line of text on this component.
    ///
    /// The host bakes the text the next time it renders a frame. Until then,
    /// [`Handle::baked_text_size`] returns `None`.
    pub fn text(mut self, text: Text) -> Self {
        self.text = Some(text);
        self
    }

    /// Draws an image on this component, from its encoded bytes.
    ///
    /// The host decodes and bakes the image the next time it renders a
    /// frame. Until then, [`Handle::baked_image_size`] returns `None`.
    pub fn image(mut self, image: Image) -> Self {
        self.image = Some(image);
        self
    }

    /// Draws a border around this component.
    pub fn border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    /// Draws a soft glow behind this component.
    ///
    /// A component shows a glow or a drop shadow, not both. If both are set,
    /// the drop shadow is drawn.
    pub fn glow(mut self, glow: Glow) -> Self {
        self.glow = Some(glow);
        self
    }

    /// Draws a drop shadow behind this component.
    ///
    /// Takes precedence over [`ComponentSpec::glow`] if both are set.
    pub fn drop_shadow(mut self, shadow: DropShadow) -> Self {
        self.drop_shadow = Some(shadow);
        self
    }

    /// Makes this component ignore all input.
    ///
    /// Components are interactive by default, so handlers such as
    /// [`Handle::on_click`] work without an opt-in. A non-interactive
    /// component is never the target of any input, whether pointer, touch,
    /// keyboard, gamepad or remote. Use it for passive elements such as
    /// backgrounds and labels. See [`Handle::set_interactive`].
    pub fn non_interactive(mut self) -> Self {
        self.non_interactive = true;
        self
    }

    /// Whether any interaction style was declared. A component with none
    /// gets no interaction styling, so hovering or pressing it starts no
    /// style transition.
    pub(crate) fn has_interaction_styles(&self) -> bool {
        self.hover.is_some()
            || self.pressed.is_some()
            || self.focused.is_some()
            || self.disabled.is_some()
    }
}
