//! [`Handle`], [`SignalHandle`] and [`TextureHandle`]: the IDs of components,
//! signals and textures.
//!
//! A handle holds no state. Its methods take the [`Proteus`] it came from,
//! which holds everything: `button.on_click(&mut app, |app| { ... })`.

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::Entity;
use bevy_ecs::world::EntityWorldMut;
use glam::Vec2;

use proteus_render::{TextureId, TextureKind};
use proteus_ui::{
    BakedComposite, BakedImage, BakedText, Disabled, GroupSource, GroupTarget, Interactable,
    MergeLayout, NToOneRequest, OneToNRequest, Opacity, QuadState, SignalId, SplitStrategy,
    TextureRef, TransitionConfig, TransitionRequest, TransitioningConfig, VideoCrossfade,
    VideoPlayer, Visibility,
};

use crate::app::DeclaredGeometry;
use crate::callback::EventKind;
use crate::Proteus;

// ---------------------------------------------------------------------------
// HandleError
// ---------------------------------------------------------------------------

/// Describes why a [`Handle`] method could not run.
///
/// Methods that return this error also log it, so the failure is visible even
/// if the result is ignored. Using a handle after its component is destroyed
/// never panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleError {
    /// This handle's component no longer exists: it was destroyed, directly
    /// or along with its parent.
    EntityNotFound,
    /// Another component the call needs no longer exists: a child, the
    /// source of an image, or a component in a split or merge.
    OtherEntityNotFound,
}

impl std::fmt::Display for HandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EntityNotFound => f.write_str("handle refers to a dead entity"),
            Self::OtherEntityNotFound => {
                f.write_str("a handle passed to this call refers to a dead entity")
            }
        }
    }
}

impl std::error::Error for HandleError {}

/// `world.entity_mut(entity)` without the panic: logs the failure, naming the
/// `Handle` method `op`, and returns [`HandleError::EntityNotFound`].
fn entity_mut<'a>(
    app: &'a mut Proteus,
    entity: Entity,
    op: &str,
) -> Result<EntityWorldMut<'a>, HandleError> {
    app.world.world.get_entity_mut(entity).map_err(|_| {
        log::warn!("Handle::{op}: entity {entity:?} is no longer alive — call ignored");
        HandleError::EntityNotFound
    })
}

/// [`entity_mut`] for an entity other than the handle's own, reporting
/// [`HandleError::OtherEntityNotFound`].
fn other_entity_mut<'a>(
    app: &'a mut Proteus,
    entity: Entity,
    op: &str,
    role: &str,
) -> Result<EntityWorldMut<'a>, HandleError> {
    app.world.world.get_entity_mut(entity).map_err(|_| {
        log::warn!("Handle::{op}: {role} entity {entity:?} is no longer alive — call ignored");
        HandleError::OtherEntityNotFound
    })
}

/// Returns `Err` if `entity` no longer exists, without borrowing it.
fn check_alive(app: &Proteus, entity: Entity, op: &str) -> Result<(), HandleError> {
    if app.world.world.entities().contains(entity) {
        Ok(())
    } else {
        log::warn!("Handle::{op}: entity {entity:?} is no longer alive — call ignored");
        Err(HandleError::EntityNotFound)
    }
}

/// [`check_alive`] for every component in a split or merge. A group
/// transition is all or nothing: with one component missing it would never
/// complete, so the whole call fails instead.
fn check_all_alive(
    app: &Proteus,
    entities: impl Iterator<Item = Entity>,
    op: &str,
    role: &str,
) -> Result<(), HandleError> {
    for entity in entities {
        if !app.world.world.entities().contains(entity) {
            log::warn!("Handle::{op}: {role} entity {entity:?} is no longer alive — call ignored");
            return Err(HandleError::OtherEntityNotFound);
        }
    }
    Ok(())
}

/// Returns every entity in `root`'s subtree, `root` included.
fn subtree(app: &Proteus, root: Entity) -> Vec<Entity> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        if let Some(children) = app.world.world.get::<proteus_ui::Children>(out[i]) {
            out.extend(children.iter());
        }
        i += 1;
    }
    out
}

/// Forgets the callbacks of every entity in `root`'s subtree, and the
/// `on_dropped` handlers of the signals they own. Call just before despawning:
/// despawning a subtree destroys its owned signals but not their handlers,
/// which this crate keeps separately.
fn forget_subtree(app: &mut Proteus, root: Entity) {
    for entity in subtree(app, root) {
        let owned = app
            .world
            .world
            .get::<proteus_ui::OwnedSignals>(entity)
            .map(|s| s.0.clone())
            .unwrap_or_default();
        for signal in owned {
            app.callbacks.forget_signal(signal);
        }
        app.callbacks.forget_entity(entity);
    }
}

/// Where a transition into `entity` should end: its declared geometry, or
/// its current geometry if it has none.
fn declared_geometry(app: &Proteus, entity: Entity) -> QuadState {
    app.world
        .world
        .get::<DeclaredGeometry>(entity)
        .map(|d| d.0.clone())
        .or_else(|| app.world.world.get::<QuadState>(entity).cloned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// The ID of a component.
///
/// Cheap to copy and store. Once the component is destroyed, methods that
/// change it return [`HandleError::EntityNotFound`] and
/// [`Proteus::get`] returns `None`.
///
/// Callbacks registered with the `on_*` methods run every time their event
/// happens, until the component is destroyed. They run after
/// [`Proteus::tick`] has updated the app, so anything they start takes effect
/// on the next tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub(crate) Entity);

impl Handle {
    /// Wraps an ECS entity as a handle; the inverse of [`Handle::id`].
    ///
    /// Doesn't check that the entity exists. If it doesn't, the handle behaves
    /// like one whose component was destroyed.
    pub fn from_entity(entity: Entity) -> Self {
        Self(entity)
    }

    /// The underlying ECS entity, for use with [`Proteus::world_mut`].
    pub fn id(&self) -> Entity {
        self.0
    }

    /// Sets this component's declared geometry and moves it there
    /// immediately.
    ///
    /// Use this when a component's layout can only be worked out after it is
    /// created, such as a cell sized to fit its baked label. Transitions into
    /// the component, and its interaction styles, use the new geometry.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_declared_geometry(
        &self,
        app: &mut Proteus,
        state: QuadState,
    ) -> Result<(), HandleError> {
        entity_mut(app, self.0, "set_declared_geometry")?
            .insert((state.clone(), DeclaredGeometry(state.clone())));
        // Interaction styles resolve against their own copy of the declared
        // geometry. Left stale, the component would snap back to its old
        // geometry when the pointer leaves it.
        if let Some(mut interaction) = app
            .world
            .world
            .get_mut::<proteus_ui::InteractionState>(self.0)
        {
            interaction.declared = state;
        }
        Ok(())
    }

    /// Transitions this component from its current geometry to `to`.
    ///
    /// Unlike [`SignalHandle::set`], only this component is involved, which
    /// makes this a good fit for moving a component around repeatedly. Calling
    /// it during a transition starts a new one from wherever the component is.
    /// The transition starts on the next tick.
    ///
    /// # Examples
    ///
    /// Slide a component 300 units to the right over a third of a second:
    ///
    /// ```
    /// use glam::Vec3;
    /// use proteus_sdk::{ease_out_cubic, ComponentSpec, Proteus, QuadState, TransitionConfig};
    ///
    /// let mut app = Proteus::new();
    /// let card = app.component(ComponentSpec::new(QuadState::default()));
    ///
    /// let moved = QuadState {
    ///     position: Vec3::new(300.0, 0.0, 0.0),
    ///     ..QuadState::default()
    /// };
    /// let config = TransitionConfig {
    ///     duration: 0.3,
    ///     delay: 0.0,
    ///     easing: ease_out_cubic,
    /// };
    /// card.animate_to(&mut app, moved.clone(), config)?;
    ///
    /// // Run half a second of ticks: the transition starts, then finishes.
    /// for _ in 0..30 {
    ///     app.tick(1.0 / 60.0);
    /// }
    /// let data = app.get(card).unwrap();
    /// assert!(data.transition.is_none());
    /// assert_eq!(data.geometry.position, moved.position);
    /// # Ok::<(), proteus_sdk::HandleError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn animate_to(
        &self,
        app: &mut Proteus,
        to: QuadState,
        config: TransitionConfig,
    ) -> Result<(), HandleError> {
        entity_mut(app, self.0, "animate_to")?.insert(TransitionRequest {
            to,
            config,
            from_state: None,
        });
        Ok(())
    }

    /// Shows the playing video on this component.
    ///
    /// Proteus doesn't play video: the app's own player supplies the frames
    /// (see `proteus_runtime::HostServices::open_video`), and the app starts
    /// and stops it with `proteus_runtime::Frame::play_video` and
    /// `stop_video`. Only one video plays at a time, and every component
    /// showing video shows the same one. If the component also has an
    /// image, [`Handle::set_video_crossfade`] blends between the two; this
    /// call starts fully on the video.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn start_video(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "start_video")?
            .insert((VideoPlayer, VideoCrossfade { video_t: 1.0 }));
        Ok(())
    }

    /// Stops showing video on this component. The component returns to
    /// showing its image or color.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn stop_video(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "stop_video")?
            .remove::<VideoPlayer>()
            .remove::<VideoCrossfade>();
        Ok(())
    }

    /// Sets the blend between this component's image (`0.0`) and the video
    /// (`1.0`).
    ///
    /// To fade the video in, call this with `0.0` right after
    /// [`Handle::start_video`], then raise it over time, for example in step
    /// with a transition on the same component.
    ///
    /// Returns `Ok(false)` if the component isn't showing video.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_video_crossfade(
        &self,
        app: &mut Proteus,
        video_t: f32,
    ) -> Result<bool, HandleError> {
        check_alive(app, self.0, "set_video_crossfade")?;
        match app.world.world.get_mut::<VideoCrossfade>(self.0) {
            Some(mut crossfade) => {
                crossfade.video_t = video_t;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// The size in pixels of this component's baked text, or `None` if the
    /// text hasn't been baked yet or the component has none.
    ///
    /// The host bakes text the next time it renders a frame. Use this for
    /// layout that depends on the text's real width.
    pub fn baked_text_size(&self, app: &Proteus) -> Option<Vec2> {
        app.world
            .world
            .get::<BakedText>(self.0)
            .map(|b| Vec2::from(b.pixel_size))
    }

    /// The size in pixels of this component's baked image, or `None` if the
    /// image hasn't been baked yet or the component has none.
    ///
    /// This is the image's full size, even after
    /// [`Handle::center_crop_to_square`].
    pub fn baked_image_size(&self, app: &Proteus) -> Option<Vec2> {
        app.world
            .world
            .get::<BakedImage>(self.0)
            .map(|b| Vec2::from(b.pixel_size))
    }

    /// Replaces current image with the `source`'s baked image.
    ///
    /// The two components share the texture, which stays in the atlas as long
    /// as either one references it.
    ///
    /// Returns `Ok(false)` if `source` has no baked image yet.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists,
    /// [`HandleError::OtherEntityNotFound`] if `source` no longer exists.
    pub fn copy_baked_image_from(
        &self,
        app: &mut Proteus,
        source: Handle,
    ) -> Result<bool, HandleError> {
        check_alive(app, self.0, "copy_baked_image_from")?;
        if !app.world.world.entities().contains(source.0) {
            log::warn!(
                "Handle::copy_baked_image_from: source entity {:?} is no longer alive — call ignored",
                source.0
            );
            return Err(HandleError::OtherEntityNotFound);
        }
        // Not an error: the source exists but has nothing baked yet.
        let Some(baked) = app.world.world.get::<BakedImage>(source.0).cloned() else {
            return Ok(false);
        };
        let texture_ref = app.world.world.get::<TextureRef>(source.0).copied();
        let mut entity = entity_mut(app, self.0, "copy_baked_image_from")?;
        entity.insert(baked);
        if let Some(texture_ref) = texture_ref {
            entity.insert(texture_ref);
        }
        Ok(true)
    }

    /// Crops this component's baked image to a centered square.
    ///
    /// Only the visible region of the texture changes; no pixels are copied
    /// and no atlas space is used. Use it to fill square cells, such as grid
    /// tiles, with images of any shape. To keep an uncropped view as well,
    /// use [`Handle::copy_baked_image_from`] on another component first.
    ///
    /// Returns `Ok(false)` if the component has no baked image yet.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn center_crop_to_square(&self, app: &mut Proteus) -> Result<bool, HandleError> {
        check_alive(app, self.0, "center_crop_to_square")?;
        // handle if nothing has baked.
        let Some(baked) = app.world.world.get::<BakedImage>(self.0).cloned() else {
            return Ok(false);
        };
        let (pw, ph) = (baked.pixel_size[0], baked.pixel_size[1]);
        let mut uv_offset = baked.uv_offset;
        let mut uv_scale = baked.uv_scale;
        if pw > ph {
            let frac = ph / pw;
            uv_offset[0] += uv_scale[0] * (1.0 - frac) / 2.0;
            uv_scale[0] *= frac;
        } else if ph > pw {
            let frac = pw / ph;
            uv_offset[1] += uv_scale[1] * (1.0 - frac) / 2.0;
            uv_scale[1] *= frac;
        }
        entity_mut(app, self.0, "center_crop_to_square")?.insert(BakedImage {
            uv_offset,
            uv_scale,
            page: baked.page,
            pixel_size: baked.pixel_size,
        });
        Ok(true)
    }

    /// Sets whether this component responds to input.
    ///
    /// A non-interactive component is never the target of any input, whether
    /// pointer, touch, keyboard, gamepad or remote: it is never hovered,
    /// pressed, dragged or focused. `false` has the same effect as
    /// [`ComponentSpec::non_interactive`](crate::ComponentSpec::non_interactive),
    /// applied after creation. For a control that is temporarily unavailable
    /// and should look unavailable, use [`Handle::set_disabled`].
    ///
    /// Proteus currently handles pointer input only (on the web, that includes
    /// touch and pen). Other kinds of input will follow the same rule.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_interactive(&self, app: &mut Proteus, interactive: bool) -> Result<(), HandleError> {
        let mut entity = entity_mut(app, self.0, "set_interactive")?;
        if interactive {
            entity.insert(Interactable);
        } else {
            entity.remove::<Interactable>();
        }
        Ok(())
    }

    /// Shows or hides this component and its children.
    ///
    /// A hidden component is neither drawn nor hit-tested.
    /// [`SignalHandle::set`] already hides the component it transitions from
    /// and shows the one it transitions to; use this for everything else.
    ///
    /// A hidden component stops being drawn on the next frame and stops
    /// receiving input one tick later. Input is matched against what was last
    /// drawn, so a click in the same tick as the hide still reaches it.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_visible(&self, app: &mut Proteus, visible: bool) -> Result<(), HandleError> {
        entity_mut(app, self.0, "set_visible")?.insert(Visibility { visible });
        Ok(())
    }

    /// Disables or re-enables this component.
    ///
    /// A disabled component is still drawn but ignores all input, and shows
    /// its [`ComponentSpec::disabled`](crate::ComponentSpec::disabled) style. Use it for a control that isn't available yet, such as a submit
    /// button. For something that is never a control, use
    /// [`Handle::set_interactive`].
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_disabled(&self, app: &mut Proteus, disabled: bool) -> Result<(), HandleError> {
        let mut entity = entity_mut(app, self.0, "set_disabled")?;
        if disabled {
            entity.insert(Disabled);
        } else {
            entity.remove::<Disabled>();
        }
        Ok(())
    }

    /// Sets whether this component accepts input while transitioning. `None`
    /// restores the default, where a transitioning component ignores input.
    /// See [`ComponentSpec::transitioning`](crate::ComponentSpec::transitioning).
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_transitioning_config(
        &self,
        app: &mut Proteus,
        config: Option<TransitioningConfig>,
    ) -> Result<(), HandleError> {
        let mut entity = entity_mut(app, self.0, "set_transitioning_config")?;
        match config {
            Some(cfg) => entity.insert(cfg),
            None => entity.remove::<TransitioningConfig>(),
        };
        Ok(())
    }

    /// Sets this component's opacity, clamped to `0.0..=1.0`.
    ///
    /// Opacity multiplies down the hierarchy and only affects drawing: a
    /// component at `0.0` still receives pointer input. To take a component
    /// out of input as well, use [`Handle::set_visible`].
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_opacity(&self, app: &mut Proteus, opacity: f32) -> Result<(), HandleError> {
        entity_mut(app, self.0, "set_opacity")?.insert(Opacity(opacity.clamp(0.0, 1.0)));
        Ok(())
    }

    fn on(&self, app: &mut Proteus, kind: EventKind, cb: impl FnMut(&mut Proteus) + 'static) {
        app.callbacks.register(self.0, kind, Box::new(cb));
    }

    /// Calls `cb` each time the pointer is pressed on this component.
    ///
    /// A click also gives the component focus.
    pub fn on_click(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Click, cb);
    }

    /// Calls `cb` each time the pointer moves onto this component.
    pub fn on_hover_enter(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::HoverEnter, cb);
    }

    /// Calls `cb` each time the pointer moves off this component.
    pub fn on_hover_exit(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::HoverExit, cb);
    }

    /// Calls `cb` each time the pointer is pressed on this component.
    ///
    /// This fires at the same moment as [`Handle::on_click`]. Use it with
    /// [`Handle::on_release`] to follow a press from start to end.
    pub fn on_press(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Press, cb);
    }

    /// Calls `cb` each time the pointer is released after a press on this
    /// component, even if the pointer has moved off it.
    pub fn on_release(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Release, cb);
    }

    /// Calls `cb` each time this component gains focus.
    pub fn on_focus(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Focus, cb);
    }

    /// Calls `cb` each time this component loses focus.
    pub fn on_blur(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Blur, cb);
    }

    /// Calls `cb` each time a transition finishes on this component.
    ///
    /// Every transition started through this API reports its completion once,
    /// on one component:
    ///
    /// - [`Handle::animate_to`]: this component.
    /// - [`SignalHandle::set`]: the `to` component.
    /// - [`Handle::split_to`] and its variants, with [`SplitStrategy::Slice`]
    ///   or [`SplitStrategy::GridSlice`]: the source, once every target has
    ///   arrived.
    /// - [`Handle::split_to`] and its variants, with
    ///   [`SplitStrategy::PerTarget`]: each target, separately. The source has
    ///   no transition of its own; it is hidden as soon as the split starts.
    /// - [`Handle::merge_from`] and its variants: the destination, once every
    ///   source has arrived.
    ///
    /// Changes of interaction style, such as a hover effect, don't count.
    pub fn on_transition_complete(
        &self,
        app: &mut Proteus,
        cb: impl FnMut(&mut Proteus) + 'static,
    ) {
        self.on(app, EventKind::TransitionComplete, cb);
    }

    /// Calls `cb` every tick while this component is pressed, with the
    /// distance the pointer moved since the previous tick, in world units.
    pub fn on_drag(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus, Vec2) + 'static) {
        app.callbacks.register_drag(self.0, Box::new(cb));
    }

    /// Splits this component into `targets`: a 1→N transition.
    ///
    /// Each target ends at its own declared geometry. This component is
    /// hidden as soon as the split starts. With [`SplitStrategy::Slice`] and
    /// [`SplitStrategy::GridSlice`], slices of this component move into place
    /// and the targets appear when they arrive. With
    /// [`SplitStrategy::PerTarget`], the targets themselves move. The
    /// transition starts on the next tick.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists,
    /// [`HandleError::OtherEntityNotFound`] if a target doesn't. Nothing
    /// starts in either case.
    pub fn split_to(
        &self,
        app: &mut Proteus,
        targets: &[Handle],
        config: TransitionConfig,
        strategy: SplitStrategy,
    ) -> Result<(), HandleError> {
        check_alive(app, self.0, "split_to")?;
        check_all_alive(app, targets.iter().map(|h| h.0), "split_to", "target")?;
        let group_targets = targets
            .iter()
            .map(|h| GroupTarget {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        entity_mut(app, self.0, "split_to")?.insert(OneToNRequest {
            targets: group_targets,
            default_config: config,
            child_configs: None,
            strategy,
        });
        Ok(())
    }

    /// Similar [`Handle::split_to`], with a separate transition config for each
    /// target.
    ///
    /// `child_behavior` is called once per target with `(index, total)`, and
    /// its result replaces `config` for that target.
    ///
    /// ```
    /// # use proteus_sdk::*;
    /// # let mut app = Proteus::new();
    /// # let source = app.component(ComponentSpec::new(QuadState::default()));
    /// # let targets: Vec<Handle> = (0..4)
    /// #     .map(|_| app.component(ComponentSpec::new(QuadState::default())))
    /// #     .collect();
    /// source.split_to_with_behavior(
    ///     &mut app,
    ///     &targets,
    ///     TransitionConfig::default(),
    ///     SplitStrategy::Slice,
    ///     |i, _total| TransitionConfig {
    ///         duration: 0.4,
    ///         delay: i as f32 * 0.08,
    ///         easing: ease_out_cubic,
    ///     },
    /// )?;
    /// # Ok::<(), HandleError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// As for [`Handle::split_to`].
    pub fn split_to_with_behavior(
        &self,
        app: &mut Proteus,
        targets: &[Handle],
        config: TransitionConfig,
        strategy: SplitStrategy,
        child_behavior: impl Fn(usize, usize) -> TransitionConfig,
    ) -> Result<(), HandleError> {
        check_alive(app, self.0, "split_to_with_behavior")?;
        check_all_alive(
            app,
            targets.iter().map(|h| h.0),
            "split_to_with_behavior",
            "target",
        )?;
        let total = targets.len();
        let child_configs = (0..total).map(|i| child_behavior(i, total)).collect();
        let group_targets = targets
            .iter()
            .map(|h| GroupTarget {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        entity_mut(app, self.0, "split_to_with_behavior")?.insert(OneToNRequest {
            targets: group_targets,
            default_config: config,
            child_configs: Some(child_configs),
            strategy,
        });
        Ok(())
    }

    /// Like [`Handle::split_to`], with each target's end geometry given
    /// explicitly instead of taken from its declared geometry.
    ///
    /// Use this when a target should end somewhere other than its declared
    /// geometry, or when this component is also one of the targets. In the
    /// second case, pass its end geometry here rather than calling
    /// [`Handle::set_declared_geometry`] first, which would move it before
    /// the split begins.
    ///
    /// # Errors
    ///
    /// As for [`Handle::split_to`].
    pub fn split_to_with_states(
        &self,
        app: &mut Proteus,
        targets: &[(Handle, QuadState)],
        config: TransitionConfig,
        strategy: SplitStrategy,
    ) -> Result<(), HandleError> {
        check_alive(app, self.0, "split_to_with_states")?;
        check_all_alive(
            app,
            targets.iter().map(|(h, _)| h.0),
            "split_to_with_states",
            "target",
        )?;
        let group_targets = targets
            .iter()
            .map(|(h, state)| GroupTarget {
                entity: h.0,
                state: state.clone(),
            })
            .collect();
        entity_mut(app, self.0, "split_to_with_states")?.insert(OneToNRequest {
            targets: group_targets,
            default_config: config,
            child_configs: None,
            strategy,
        });
        Ok(())
    }

    /// Similar to [`Handle::merge_from`], with a separate transition config for
    /// each source.
    ///
    /// `child_behavior` is called once per source with `(index, total)`, and
    /// its result replaces `config` for that source. See
    /// [`Handle::split_to_with_behavior`].
    ///
    /// # Errors
    ///
    /// As for [`Handle::merge_from`].
    pub fn merge_from_with_behavior(
        &self,
        app: &mut Proteus,
        sources: &[Handle],
        config: TransitionConfig,
        layout: MergeLayout,
        child_behavior: impl Fn(usize, usize) -> TransitionConfig,
    ) -> Result<(), HandleError> {
        check_alive(app, self.0, "merge_from_with_behavior")?;
        check_all_alive(
            app,
            sources.iter().map(|h| h.0),
            "merge_from_with_behavior",
            "source",
        )?;
        let total = sources.len();
        let child_configs = (0..total).map(|i| child_behavior(i, total)).collect();
        let group_sources = sources
            .iter()
            .map(|h| GroupSource {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        entity_mut(app, self.0, "merge_from_with_behavior")?.insert(NToOneRequest {
            sources: group_sources,
            default_config: config,
            child_configs: Some(child_configs),
            layout,
        });
        Ok(())
    }

    /// Merges `sources` into this component: an N→1 transition.
    ///
    /// The sources are hidden as soon as the merge starts. `layout` decides
    /// which part of this component each source moves toward. The transition
    /// starts on the next tick.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists,
    /// [`HandleError::OtherEntityNotFound`] if a source doesn't. Nothing
    /// starts in either case.
    pub fn merge_from(
        &self,
        app: &mut Proteus,
        sources: &[Handle],
        config: TransitionConfig,
        layout: MergeLayout,
    ) -> Result<(), HandleError> {
        check_alive(app, self.0, "merge_from")?;
        check_all_alive(app, sources.iter().map(|h| h.0), "merge_from", "source")?;
        let group_sources = sources
            .iter()
            .map(|h| GroupSource {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        entity_mut(app, self.0, "merge_from")?.insert(NToOneRequest {
            sources: group_sources,
            default_config: config,
            child_configs: None,
            layout,
        });
        Ok(())
    }

    /// Makes `child` a child of this component. Its geometry becomes relative
    /// to this component's.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists,
    /// [`HandleError::OtherEntityNotFound`] if `child` doesn't.
    pub fn add_child(&self, app: &mut Proteus, child: Handle) -> Result<(), HandleError> {
        check_alive(app, self.0, "add_child")?;
        other_entity_mut(app, child.0, "add_child", "child")?.insert(ChildOf(self.0));
        Ok(())
    }

    /// Removes `child` from this component.
    ///
    /// If `destroy` is `true`, the child is also destroyed, exactly as by
    /// [`Handle::destroy`]. If it is `false`, the child is kept as a top-level
    /// component. Its geometry is then no longer relative to this component.
    ///
    /// # Examples
    ///
    /// ```
    /// # use proteus_sdk::*;
    /// let mut app = Proteus::new();
    /// let item = app.component(ComponentSpec::new(QuadState::default()));
    /// let list = app.component(ComponentSpec::new(QuadState::default()).child(item));
    ///
    /// // Take the item out of the list, but keep it.
    /// list.remove_child(&mut app, item, false)?;
    /// assert!(app.get(list).unwrap().children.is_empty());
    /// assert!(app.get(item).is_some());
    /// # Ok::<(), HandleError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists,
    /// [`HandleError::OtherEntityNotFound`] if `child` doesn't.
    pub fn remove_child(
        &self,
        app: &mut Proteus,
        child: Handle,
        destroy: bool,
    ) -> Result<(), HandleError> {
        // Checked although `self` isn't used below, so that this and
        // `add_child` log a missing parent consistantly.
        check_alive(app, self.0, "remove_child")?;
        if destroy {
            forget_subtree(app, child.0);
            // `World::despawn` doesn't panic; it returns whether the entity
            // existed.
            return if app.world.world.despawn(child.0) {
                Ok(())
            } else {
                log::warn!(
                    "Handle::remove_child: child entity {:?} is no longer alive — call ignored",
                    child.0
                );
                Err(HandleError::OtherEntityNotFound)
            };
        }
        other_entity_mut(app, child.0, "remove_child", "child")?.remove::<ChildOf>();
        Ok(())
    }

    /// Destroys this component and its children, along with their callbacks
    /// and the signals they own.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if it was already destroyed. Safe to
    /// ignore, but reported so that destroying twice is visible.
    pub fn destroy(self, app: &mut Proteus) -> Result<(), HandleError> {
        // Do this before the despawn, while `Children` and `OwnedSignals` can still be
        // read.
        forget_subtree(app, self.0);
        // `World::despawn` doesn't panic; it returns whether the entity existed.
        if app.world.world.despawn(self.0) {
            Ok(())
        } else {
            log::warn!(
                "Handle::destroy: entity {:?} was already destroyed — call ignored",
                self.0
            );
            Err(HandleError::EntityNotFound)
        }
    }

    /// Releases this component's references to its baked text, image or
    /// content. The component itself remains.
    ///
    /// This doesn't free atlas space immediately. A texture that no component
    /// references becomes available for reuse, and the atlas reclaims its space
    /// when it needs room for another texture. Textures marked `eternal` are
    /// never reclaimed. [`TextureHandle`] has no `free`.
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn free_resources(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "free_resources")?
            .remove::<TextureRef>()
            .remove::<BakedImage>()
            .remove::<BakedText>()
            .remove::<BakedComposite>();
        Ok(())
    }

    /// Sets this component's image to a new `texture`, replacing its current image.
    ///
    /// Only the image changes. Text on the component is still drawn on top,
    /// and video, if the component is showing it, still plays. A component
    /// made with [`ComponentSpec::bake`](crate::ComponentSpec::bake) keeps
    /// showing its baked content.
    ///
    /// # Errors
    ///
    /// [`HandleError::EntityNotFound`] if this component no longer exists.
    pub fn set_texture(
        &self,
        app: &mut Proteus,
        texture: TextureHandle,
    ) -> Result<bool, HandleError> {
        check_alive(app, self.0, "set_texture")?;
        // From here on, failure means there is nothing to show (no GPU, or
        // the texture was evicted), not a dead handle.
        let Some(pipeline) = app
            .world
            .world
            .get_resource::<proteus_render::QuadPipeline>()
        else {
            return Ok(false);
        };
        let Some(uv) = pipeline.texture_registry.main_atlas_uv(texture.0) else {
            return Ok(false);
        };
        let Some((_, width, height)) = pipeline.texture_registry.info(texture.0) else {
            return Ok(false);
        };
        entity_mut(app, self.0, "set_texture")?.insert((
            BakedImage {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [width as f32, height as f32],
            },
            TextureRef(texture.0),
        ));
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// SignalHandle
// ---------------------------------------------------------------------------

/// The ID of a signal, which transitions one component into another. See
/// [`SignalHandle::set`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalHandle(pub(crate) SignalId);

impl SignalHandle {
    /// The underlying signal ID.
    pub fn id(&self) -> SignalId {
        self.0
    }

    /// Transitions `from` into `to`: a 1→1 transition.
    ///
    /// `from` is hidden, and `to` is shown and moves from `from`'s current
    /// geometry to its own declared geometry. If `to` is already
    /// transitioning, the request is dropped unless `interruptible` is set;
    /// then a new transition starts from wherever `to` is. The transition
    /// starts on the next tick.
    ///
    /// A request that can't run is reported to [`SignalHandle::on_dropped`].
    pub fn set(
        &self,
        app: &mut Proteus,
        to: Handle,
        from: Handle,
        config: TransitionConfig,
        interruptible: bool,
    ) {
        let target = app
            .world
            .world
            .get::<DeclaredGeometry>(to.0)
            .map(|d| d.0.clone())
            .or_else(|| app.world.world.get::<proteus_ui::QuadState>(to.0).cloned())
            .unwrap_or_default();
        proteus_ui::set_signal(
            &mut app.world.world,
            self.0,
            to.0,
            from.0,
            target,
            config,
            interruptible,
        );
    }

    /// Calls `cb` with the reason each time a [`SignalHandle::set`] request
    /// on this signal can't run. See [`DropReason`](crate::DropReason).
    pub fn on_dropped(
        &self,
        app: &mut Proteus,
        cb: impl FnMut(&mut Proteus, proteus_ui::TransitionDropped) + 'static,
    ) {
        app.callbacks.register_dropped(self.0, Box::new(cb));
    }

    /// Destroys this signal and its `on_dropped` handlers. Later
    /// [`SignalHandle::set`] calls do nothing.
    pub fn destroy(self, app: &mut Proteus) {
        app.callbacks.forget_signal(self.0);
        proteus_ui::destroy_signal(&mut app.world.world, self.0);
    }
}

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

/// Adds a texture to the atlas. Used by [`Proteus::bake_texture`] and
/// [`Proteus::load_texture`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TextureRequest {
    /// Scale the texture down so that its longer side is at most this many
    /// pixels. `None` keeps its full size.
    pub max_side: Option<u32>,
    /// Keep the texture in the atlas for the life of the app, never evicting
    /// it.
    pub eternal: bool,
}

/// The ID of a texture in the atlas.
///
/// Display the texture on a component with [`Handle::set_texture`]. A texture can't be
/// freed through its handle: once no component references it, the atlas can
/// reclaim its space when it needs room. See [`Handle::free_resources`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub(crate) TextureId);

impl TextureHandle {
    /// Wraps a texture ID. The same as [`Proteus::texture`], without needing
    /// the `Proteus`.
    pub fn from_texture_id(id: TextureId) -> Self {
        Self(id)
    }

    /// The underlying texture ID.
    pub fn id(&self) -> TextureId {
        self.0
    }

    /// The texture's kind and size in pixels, or `None` if it has been
    /// evicted or never existed.
    pub fn state(&self, app: &Proteus) -> Option<(TextureKind, u32, u32)> {
        let pipeline = app
            .world
            .world
            .get_resource::<proteus_render::QuadPipeline>()?;
        if !pipeline.texture_registry.is_active(self.0) {
            return None;
        }
        pipeline.texture_registry.info(self.0)
    }
}
