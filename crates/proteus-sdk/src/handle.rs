//! [`Handle`], [`SignalHandle`], [`TextureHandle`] — thin `Copy` identity
//! tokens returned by [`crate::Proteus`]'s constructors.
//!
//! PLANNING.md's Phase A sketches these as JS objects that capture behavior
//! freely in closures (`button.onClick(() => ...)`) — Rust has no implicit
//! shared mutable state to make that work directly, so every behavioral
//! method here takes `&mut Proteus` (or `&Proteus` for reads) explicitly:
//! `button.on_click(&mut app, |app| { ... })`. The handle itself carries no
//! state beyond the wrapped id — all real state lives in `Proteus`'s world,
//! read via [`crate::Proteus::get`].

use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::Entity;
use bevy_ecs::world::EntityWorldMut;
use glam::Vec2;

use proteus_render::{TextureId, TextureKind};
use proteus_ui::{
    BakedComposite, BakedImage, BakedText, GroupSource, GroupTarget, Interactable, MergeLayout,
    NToOneRequest, OneToNRequest, QuadState, SignalId, SplitStrategy, TextureRef, TransitionConfig,
    TransitionRequest, VideoCrossfade, VideoPlayer, Visibility,
};

use crate::app::DeclaredGeometry;
use crate::callback::EventKind;
use crate::Proteus;

// ---------------------------------------------------------------------------
// HandleError
// ---------------------------------------------------------------------------

/// Why a [`Handle`] operation could not be applied.
///
/// Every fallible `Handle` method returns `Result<_, HandleError>` **and** logs
/// the failure at `warn!` before returning it. Both, deliberately: the `Result`
/// lets a caller that cares branch on the outcome, and the log means a caller
/// that deliberately ignores it (`let _ = …`, common in app code that knows its
/// handles are alive) still leaves something in the log rather than failing
/// silently. `bevy_ecs`'s own `World::despawn` uses exactly this shape — `bool`
/// plus an internal `warn!` — for the same situation.
///
/// These were previously **panics**: every mutating method reached
/// `World::entity_mut`, which panics on a despawned entity, while three separate
/// doc comments (here, in `proteus-sdk-web`, and in the TS `handleFromId`
/// JSDoc) promised that a stale handle would quietly no-op. On the wasm target a
/// panic aborts the module and the canvas freezes with no recovery, so this was
/// a page-killer one `destroy()`-then-touch race away.
///
/// **Not an error:** "there was nothing to do." A method that finds no baked
/// image to copy or crop reports that through its `Ok` value, not through this
/// enum — that's a routine, expected state (the image simply hasn't finished
/// baking yet), whereas everything below means the caller is working from a
/// handle that no longer refers to anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleError {
    /// This handle's own entity is no longer alive — destroyed via
    /// [`Handle::destroy`]/[`Handle::remove_child`], despawned as a descendant
    /// of a destroyed parent, or reconstructed from a stale id.
    EntityNotFound,
    /// The *other* entity an operation needs is gone: the `child` of
    /// [`Handle::add_child`]/[`Handle::remove_child`], the `source` of
    /// [`Handle::copy_baked_image_from`], or a `targets`/`sources` entry of a
    /// group transition. `self` is alive; the operation still couldn't run.
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

/// `world.entity_mut(entity)` without the panic: logs which call site hit a
/// dead entity and returns [`HandleError::EntityNotFound`].
///
/// `op` is the `Handle` method name, so the log says what was being attempted
/// rather than just naming an entity id.
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

/// [`entity_mut`] for an entity that isn't `self` — same thing, but reports
/// [`HandleError::OtherEntityNotFound`] so a caller can tell "my handle died"
/// from "the handle I was handed died".
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

/// Returns `Err` if `entity` is not alive, without borrowing it — for methods
/// that need the liveness check but then go on to touch the world through some
/// other path.
fn check_alive(app: &Proteus, entity: Entity, op: &str) -> Result<(), HandleError> {
    if app.world.world.entities().contains(entity) {
        Ok(())
    } else {
        log::warn!("Handle::{op}: entity {entity:?} is no longer alive — call ignored");
        Err(HandleError::EntityNotFound)
    }
}

/// [`check_alive`] across a group-transition's whole target/source list, up
/// front: a group transition is all-or-nothing, so one dead participant fails
/// the call rather than silently running a split/merge with a hole in it (the
/// setup systems would hide the live siblings and then wait forever on a
/// virtual that can never complete).
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

/// Every entity in `root`'s subtree, `root` included.
///
/// `bevy_ecs`'s `ChildOf`/`Children` relationship cascades a despawn to
/// descendants, so destroying a parent destroys them too — and their callbacks
/// have to be forgotten along with the parent's.
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

/// Forget every callback registered against `root`'s subtree, and every
/// `on_dropped` handler for the signals those entities own.
///
/// Call immediately *before* despawning. `proteus-ui` already destroys an
/// owned signal when its owner despawns (`OwnedSignals`' hook), but that only
/// clears the `SignalRegistry` — this crate's own handler map is separate and
/// has to be told too.
///
/// Does not cover a despawn made directly through
/// [`Proteus::world_mut`](crate::Proteus::world_mut): that escape hatch bypasses
/// this crate entirely. Closing that would mean a `proteus-ui`-side despawn hook
/// feeding a "these died this frame" queue for the SDK to drain — worth doing if
/// direct world despawns ever become common, but not for an escape hatch.
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

/// Resolve `entity`'s declared rest geometry — the `DeclaredGeometry`
/// `component()` captured at creation time if present, else its current
/// live `QuadState`, else a default. Shared by the group-transition
/// methods below; `SignalHandle::set` (which predates this helper) does the
/// same resolution inline.
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

/// Identity token for one component. Cheap to copy and hold onto; carries no
/// state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(pub(crate) Entity);

impl Handle {
    /// Wrap an existing entity as a `Handle` — the inverse of [`Handle::id`].
    /// For callers (e.g. `proteus-sdk-web`, M12.4) that receive an entity id
    /// through some other channel — a `component()` spec's `children` field
    /// crossing the wasm boundary as `Entity::to_bits()` values, say — and
    /// need to turn it back into a real `Handle` to call `add_child`/etc. on.
    /// Does not check that `entity` is actually alive; passing a stale or
    /// foreign entity behaves exactly like passing a stale `Handle` obtained
    /// any other way — mutating methods log a warning and return
    /// [`HandleError::EntityNotFound`], and `Proteus::get` returns `None`.
    /// Neither panics.
    pub fn from_entity(entity: Entity) -> Self {
        Self(entity)
    }

    /// The underlying ECS entity — an escape hatch for callers that need to
    /// reach `proteus-ui`/`bevy_ecs` directly (e.g. attaching
    /// `proteus_ui::component::Disabled`, not yet exposed as a `Handle`
    /// convenience).
    pub fn id(&self) -> Entity {
        self.0
    }

    /// The baked glyph run's pixel footprint, if this component's `Text` has
    /// been baked — `None` before baking completes (baking is a shell/host
    /// responsibility; see `proteus-demo`'s crate-root doc) or if this
    /// component was never given `.text(...)`. Useful for layout that has to
    /// wait on a text run's actual measured width (e.g. positioning a label
    /// next to it) rather than guessing at spawn time.
    /// Overwrites both the live `QuadState` and the "declared rest
    /// geometry" `component()` captured at creation time. Plain `QuadState`
    /// mutation via the `world_mut()` escape hatch only updates the live
    /// value — group transitions (`split_to`/`merge_from`) resolve a
    /// target's rest state from the *declared* value (see
    /// `crate::app::DeclaredGeometry`'s doc), which would otherwise stay
    /// stuck at whatever `ComponentSpec::geometry` was at spawn time. Needed
    /// whenever a component's real resting layout can only be computed
    /// *after* spawn — e.g. a grid cell sized from its label's actual baked
    /// width, only known once baking completes.
    pub fn set_declared_geometry(
        &self,
        app: &mut Proteus,
        state: QuadState,
    ) -> Result<(), HandleError> {
        entity_mut(app, self.0, "set_declared_geometry")?
            .insert((state.clone(), DeclaredGeometry(state.clone())));
        // `interaction_style_system` resolves hover/pressed/focused overrides
        // against its *own* snapshot of the rest state, taken the first frame
        // it ever saw this entity — it has no access to `DeclaredGeometry`
        // (private to this crate). Left stale, a component whose rest layout
        // is computed after spawn — a grid cell sized from its baked label,
        // the exact case this method exists for — would snap back to its
        // original spawn geometry the moment the pointer left it.
        if let Some(mut interaction) = app
            .world
            .world
            .get_mut::<proteus_ui::InteractionState>(self.0)
        {
            interaction.declared = state;
        }
        Ok(())
    }

    /// Animates this component to `to` over `config`, starting from its
    /// current live `QuadState` — a 1→1 morph with no other entity
    /// involved, unlike [`crate::Proteus::signal`]'s owner/target-mediated
    /// version. Useful for repeatedly re-targeting the *same* entity to a
    /// fresh ad-hoc destination (e.g. a particle-style effect retriggering
    /// each idle entity to a new random position) where there's no second
    /// entity's declared geometry to resolve against. Check
    /// [`crate::Proteus::get`]'s `transition` field (`None` means idle) to
    /// know when it's safe to call again.
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

    /// Marks this component as showing the live video feed — the render
    /// path samples the shell's shared video texture for any entity
    /// carrying `VideoPlayer`, regardless of *which* video is playing
    /// (there's only ever one at a time; the shell owns starting/stopping
    /// the actual decode — see `proteus-demo`'s crate-root doc on what
    /// stays a shell concern). If this component also has a `BakedImage`
    /// (e.g. box-cover art, from `.image()`/an injected `Image`),
    /// `video_t` blends between it (`0.0`) and the video (`1.0`) —
    /// `1.0` here shows the video immediately, with no crossfade; a caller
    /// wanting a gradual reveal can animate `video_t` down from there
    /// itself via the `world_mut()` escape hatch.
    pub fn start_video(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "start_video")?
            .insert((VideoPlayer, VideoCrossfade { video_t: 1.0 }));
        Ok(())
    }

    /// Reverses [`Handle::start_video`] — back to showing whatever
    /// `BakedImage`/solid color this component had before.
    pub fn stop_video(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "stop_video")?
            .remove::<VideoPlayer>()
            .remove::<VideoCrossfade>();
        Ok(())
    }

    /// Updates `video_t` on a component already showing live video — a
    /// no-op if it isn't (e.g. never [`Handle::start_video`]-ed, or already
    /// [`Handle::stop_video`]-ed). `start_video` itself always inserts
    /// `video_t: 1.0` (no crossfade, video shows immediately) — a caller
    /// wanting a gradual reveal calls this right after to override it back
    /// down, then ramps it back up over time (e.g. driven by this same
    /// component's own [`crate::Proteus::get`]`(..).transition.progress`,
    /// if the reveal is meant to track a geometry morph already running on
    /// it) — see `start_video`'s own doc.
    /// `Ok(false)` means the component is alive but isn't showing video (never
    /// [`Handle::start_video`]-ed, or already stopped) — routine, since callers
    /// ramp this every frame without tracking playback state themselves.
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

    /// The baked glyph run's pixel footprint, if this component's `Text` has
    /// been baked — `None` before baking completes (baking is a shell/host
    /// responsibility; see `proteus-demo`'s crate-root doc) or if this
    /// component was never given `.text(...)`. Useful for layout that has to
    /// wait on a text run's actual measured width (e.g. positioning a label
    /// next to it) rather than guessing at spawn time.
    pub fn baked_text_size(&self, app: &Proteus) -> Option<Vec2> {
        app.world
            .world
            .get::<BakedText>(self.0)
            .map(|b| Vec2::from(b.pixel_size))
    }

    /// The baked image's pixel footprint, if this component's `Image` has
    /// been baked — `None` before baking completes (baking is a shell/host
    /// responsibility; see `proteus-demo`'s crate-root doc) or if this
    /// component was never given an `Image`. Mirrors
    /// [`Handle::baked_text_size`]; also useful as a plain "has this image
    /// finished baking yet" poll (`Some`/`None`) independent of the size
    /// itself.
    pub fn baked_image_size(&self, app: &Proteus) -> Option<Vec2> {
        app.world
            .world
            .get::<BakedImage>(self.0)
            .map(|b| Vec2::from(b.pixel_size))
    }

    /// Copies whichever baked image `source` currently shows onto this
    /// component (replacing this component's own `BakedImage`/`TextureRef`,
    /// same "insert wins" semantics as [`Handle::set_texture`]) — `false`
    /// (no-op) if `source` has no `BakedImage` yet. `TextureRef`'s ref
    /// count (M11) is entity-scoped, not texture-scoped, so two entities
    /// sharing one texture this way is a normal, correctly-counted state,
    /// not a leak or a double-free waiting to happen.
    ///
    /// Useful when one entity needs to *immediately* show what another
    /// already-baked entity looks like — e.g. a dedicated "enlarged view"
    /// coordinator entity that a group transition is about to reveal:
    /// `split_to`/`merge_from`'s reveal only flips `Visibility`, never
    /// touches a target's own `BakedImage` (see [`Handle::split_to`]'s
    /// doc), so without a call like this the coordinator would be revealed
    /// showing nothing at all.
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
        // Distinct from the two errors above: the source is alive and simply
        // has nothing baked yet. Routine — callers poll on exactly this.
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

    /// Crops this component's current `BakedImage` to a centered square, in
    /// place — landscape narrows the UV width, portrait narrows the UV
    /// height, an already-square image is a no-op — by shrinking its UV
    /// sub-rectangle within `main_atlas`. No new atlas registration and no
    /// pixel copy: the crop is purely a smaller UV window into the exact
    /// same uploaded region, so it's essentially free and doesn't consume
    /// any additional atlas space. `pixel_size` is left as the *original*,
    /// uncropped value — same convention as [`Handle::baked_text_size`]'s
    /// doc: it's the decoded image's native size, not resized to track
    /// whatever crop is currently applied.
    ///
    /// `false` (no-op) if this component has no `BakedImage` yet. Useful
    /// for square display cells (e.g. a photo grid tile) fed from photos of
    /// varying aspect ratios — crop instead of stretch. Call
    /// [`Handle::copy_baked_image_from`] onto a separate entity *first* if
    /// the uncropped frame is needed again later (e.g. an enlarged-view
    /// coordinator) — this call is destructive to `self`'s own crop state,
    /// though the underlying atlas pixels are untouched.
    pub fn center_crop_to_square(&self, app: &mut Proteus) -> Result<bool, HandleError> {
        check_alive(app, self.0, "center_crop_to_square")?;
        // Alive but nothing baked yet — routine, not an error.
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

    /// Marks this component interactive (the default at spawn, unless
    /// [`crate::ComponentSpec::non_interactive`] was used) or not — a
    /// runtime toggle for entities whose click/hover eligibility needs to
    /// change after spawn. `false` removes `Interactable` entirely, the
    /// same effect `non_interactive()` has at spawn time, just applied
    /// later; `true` re-adds it. Useful for a mutual-exclusion toggle pair
    /// where only one of two entities should ever be clickable/hoverable
    /// at a time (e.g. a light/dark theme switch: only the icon that
    /// *doesn't* match the current theme should be interactive).
    pub fn set_interactive(&self, app: &mut Proteus, interactive: bool) -> Result<(), HandleError> {
        let mut entity = entity_mut(app, self.0, "set_interactive")?;
        if interactive {
            entity.insert(Interactable);
        } else {
            entity.remove::<Interactable>();
        }
        Ok(())
    }

    /// Shows or hides this component. Hidden components stay in the world
    /// but are skipped by render, input and navigation; children cascade.
    ///
    /// `SignalHandle::set` already hides its `from` and reveals its `to`, so
    /// a signal-driven morph needs no call here. This is for visibility a
    /// signal doesn't own — chrome that appears once past a splash screen,
    /// a panel toggled directly.
    pub fn set_visible(&self, app: &mut Proteus, visible: bool) -> Result<(), HandleError> {
        entity_mut(app, self.0, "set_visible")?.insert(Visibility { visible });
        Ok(())
    }

    fn on(&self, app: &mut Proteus, kind: EventKind, cb: impl FnMut(&mut Proteus) + 'static) {
        app.callbacks.register(self.0, kind, Box::new(cb));
    }

    pub fn on_click(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Click, cb);
    }

    pub fn on_hover_enter(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::HoverEnter, cb);
    }

    pub fn on_hover_exit(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::HoverExit, cb);
    }

    pub fn on_press(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Press, cb);
    }

    pub fn on_release(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Release, cb);
    }

    pub fn on_focus(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Focus, cb);
    }

    pub fn on_blur(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus) + 'static) {
        self.on(app, EventKind::Blur, cb);
    }

    pub fn on_drag(&self, app: &mut Proteus, cb: impl FnMut(&mut Proteus, Vec2) + 'static) {
        app.callbacks.register_drag(self.0, Box::new(cb));
    }

    /// Split this component into `targets` — a 1→N group transition
    /// (Phase B's 1→N topology). Each target's geometry is resolved
    /// automatically from its own `component()`-declared rest state,
    /// mirroring [`SignalHandle::set`]. Not signal-mediated — unlike 1→1
    /// transitions, `proteus-ui`'s group-transition machinery
    /// (`one_to_n_setup_system`) was never routed through the signal system
    /// (M12.1's scope was 1→1 only), so this inserts the request directly,
    /// the same way `proteus-ui`'s own demo callers always have.
    ///
    /// This component (the source) is hidden by the underlying system once
    /// the transition completes — no separate visibility call needed.
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
            child_behavior: None,
            strategy,
        });
        Ok(())
    }

    /// [`Handle::split_to`], but with each target's state given explicitly
    /// instead of resolved from its own declared/live `QuadState` — needed
    /// whenever the natural `declared_geometry(target)` value would be
    /// wrong, or (when `self` is *also* one of `targets` — a shape
    /// splitting back into a group that includes its own slot) unsafe to
    /// derive: this call is synchronous, but the request it inserts is only
    /// processed on the *next* tick (see `split_to`'s own doc), so setting
    /// a target's declared geometry here — [`Handle::set_declared_geometry`]
    /// writes the live `QuadState` too — would corrupt the very "from"
    /// snapshot that next-tick processing is about to capture from this
    /// same entity's live state. Passing the correct state straight through
    /// sidesteps that footgun entirely, the same flexibility a hand-built
    /// `GroupTarget` list already has at the `proteus-ui` layer.
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
            child_behavior: None,
            strategy,
        });
        Ok(())
    }

    /// Merge `sources` into this component — an N→1 group transition
    /// (Phase B's N→1 topology). See [`Handle::split_to`]'s doc for why
    /// this isn't signal-mediated. `sources` are hidden immediately by the
    /// underlying system (`n_to_one_setup_system`) — "the morph is the
    /// exit," same convention as 1→1 signals.
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
            child_behavior: None,
            layout,
        });
        Ok(())
    }

    /// Parent `child` to this component — `child`'s `QuadState` becomes
    /// relative to this component's own (M10).
    pub fn add_child(&self, app: &mut Proteus, child: Handle) -> Result<(), HandleError> {
        check_alive(app, self.0, "add_child")?;
        other_entity_mut(app, child.0, "add_child", "child")?.insert(ChildOf(self.0));
        Ok(())
    }

    /// Detach `child` from this component. `destroy: false` leaves `child`
    /// alive as its own root entity; `destroy: true` despawns it.
    pub fn remove_child(
        &self,
        app: &mut Proteus,
        child: Handle,
        destroy: bool,
    ) -> Result<(), HandleError> {
        // Checked even though nothing below touches `self`: detaching a child
        // from a parent that no longer exists is a caller mistake either way,
        // and `add_child` reports it — an API where one of a symmetric pair
        // validates the receiver and the other doesn't is just a trap.
        check_alive(app, self.0, "remove_child")?;
        if destroy {
            forget_subtree(app, child.0);
            // `World::despawn` is already non-panicking (returns `bool` and
            // warns internally on a missing entity), so this path only needs
            // its result mapped into ours.
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

    /// Remove this component from the ECS entirely. `bevy_ecs`'s
    /// `ChildOf`/`Children` relationship cascades the despawn to every
    /// descendant automatically.
    /// Returns [`HandleError::EntityNotFound`] if it was already destroyed —
    /// harmless to ignore, but reported so a double-destroy shows up rather
    /// than passing for a successful one.
    pub fn destroy(self, app: &mut Proteus) -> Result<(), HandleError> {
        // Before the despawn: `forget_subtree` reads `Children`/`OwnedSignals`
        // off entities that are about to stop existing.
        forget_subtree(app, self.0);
        // Already non-panicking — see `remove_child`'s note on `World::despawn`.
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

    /// Release the GPU resources this component references (baked text,
    /// baked image, baked composite) while leaving the entity itself in the
    /// ECS. Removing `TextureRef` triggers M11's `ComponentHooks` to decref
    /// the shared `TextureRegistry` automatically — this is the correct way
    /// to free a texture a component owns; see [`TextureHandle`]'s doc for
    /// why it has no `.free()` of its own.
    pub fn free_resources(&self, app: &mut Proteus) -> Result<(), HandleError> {
        entity_mut(app, self.0, "free_resources")?
            .remove::<TextureRef>()
            .remove::<BakedImage>()
            .remove::<BakedText>()
            .remove::<BakedComposite>();
        Ok(())
    }

    /// Show an already-registered texture on this component (replacing
    /// whatever image/text/composite it previously showed) — the sanctioned
    /// way to do frame-swap animation off a pre-baked set (e.g. an N-frame
    /// logo loop a shell baked once at startup): bake every frame up front,
    /// wrap each with [`crate::Proteus::texture`], then call this once per
    /// frame-advance with whichever one is current. Looks up `texture`'s
    /// live placement from the `QuadPipeline` resource each call (mirrors
    /// [`TextureHandle::state`]), so it reflects eviction/atlas moves
    /// automatically rather than caching stale UVs.
    ///
    /// Returns `false` (no-op) if no `QuadPipeline` resource is installed
    /// yet, or `texture` is evicted/unknown — same "degrade gracefully"
    /// convention as the rest of this crate's texture handling.
    pub fn set_texture(
        &self,
        app: &mut Proteus,
        texture: TextureHandle,
    ) -> Result<bool, HandleError> {
        check_alive(app, self.0, "set_texture")?;
        // Everything below is "nothing to show", not "you used a dead handle":
        // no GPU pipeline installed (headless), or the texture was evicted.
        // Routine degradation, reported through `Ok(false)`.
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

/// Identity token for one registered signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SignalHandle(pub(crate) SignalId);

impl SignalHandle {
    pub fn id(&self) -> SignalId {
        self.0
    }

    /// Declare a transition: `to` should morph into its own
    /// `component()`-declared rest geometry, appearing to originate from
    /// `from`'s current geometry — mirrors the TypeScript API's
    /// `signal.set([to, from], config)` with no separate target argument,
    /// resolved here via the internal `DeclaredGeometry` `component()`
    /// captures at creation time (closing the gap M12.1's lower-level
    /// `proteus_ui::signal::set` scope-noted as this crate's job).
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

    /// Register a handler for requests on this signal that
    /// `signal_dispatch_system` declined to act on (already transitioning
    /// without `interruptible`, missing/invisible entity). Persistent, like
    /// `Handle`'s `.on_*` methods — fires on every drop, not just the first.
    pub fn on_dropped(
        &self,
        app: &mut Proteus,
        cb: impl FnMut(&mut Proteus, proteus_ui::TransitionDropped) + 'static,
    ) {
        app.callbacks.register_dropped(self.0, Box::new(cb));
    }

    /// Remove this signal from the registry. Further `.set()` calls are
    /// silently dropped (`DropReason::SignalNotFound`).
    pub fn destroy(self, app: &mut Proteus) {
        app.callbacks.forget_signal(self.0);
        proteus_ui::destroy_signal(&mut app.world.world, self.0);
    }
}

// ---------------------------------------------------------------------------
// TextureHandle
// ---------------------------------------------------------------------------

/// Identity token for an already-registered `main_atlas`/video texture.
///
/// **Inspection only — intentionally has no `.free()`.** M11's `TextureRef`
/// ref-counting is entity-scoped (`ComponentHooks` on a *component's*
/// insert/replace/remove — see `proteus_ui::texture_ref`), not an
/// independent resource with its own lifecycle the way PLANNING.md's Phase A
/// originally sketched (`heroImage.free()` on a texture created
/// independently of any component). Actually releasing a texture happens
/// through [`Handle::free_resources`] on whichever entity references it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub(crate) TextureId);

impl TextureHandle {
    /// Wrap an existing texture id. Equivalent to [`crate::Proteus::texture`]
    /// (which just does this, ignoring `self`) but usable without a
    /// `Proteus` reference in hand — e.g. `proteus-sdk-web` (M12.4)
    /// reconstructing one from an id that crossed the wasm boundary.
    pub fn from_texture_id(id: TextureId) -> Self {
        Self(id)
    }

    pub fn id(&self) -> TextureId {
        self.0
    }

    /// `Some((kind, width, height))` if this texture is still registered and
    /// active; `None` if it's been evicted or the id is unknown. Wraps
    /// `TextureRegistry::info`/`is_active` — `None` covers both "evicted"
    /// and "never existed" uniformly, since the registry itself doesn't
    /// distinguish them at this query.
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
