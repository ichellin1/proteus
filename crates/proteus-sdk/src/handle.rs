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
use glam::Vec2;

use proteus_render::{TextureId, TextureKind};
use proteus_ui::{
    BakedComposite, BakedImage, BakedText, GroupSource, GroupTarget, Interactable, MergeLayout,
    NToOneRequest, OneToNRequest, QuadState, SignalId, SplitStrategy, TextureRef, TransitionConfig,
    TransitionRequest, VideoCrossfade, VideoPlayer,
};

use crate::app::DeclaredGeometry;
use crate::callback::EventKind;
use crate::Proteus;

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
    /// any other way (methods no-op or `Proteus::get` returns `None`).
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
    pub fn set_declared_geometry(&self, app: &mut Proteus, state: QuadState) {
        app.world
            .world
            .entity_mut(self.0)
            .insert((state.clone(), DeclaredGeometry(state)));
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
    pub fn animate_to(&self, app: &mut Proteus, to: QuadState, config: TransitionConfig) {
        app.world
            .world
            .entity_mut(self.0)
            .insert(TransitionRequest {
                to,
                config,
                from_state: None,
            });
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
    pub fn start_video(&self, app: &mut Proteus) {
        app.world
            .world
            .entity_mut(self.0)
            .insert((VideoPlayer, VideoCrossfade { video_t: 1.0 }));
    }

    /// Reverses [`Handle::start_video`] — back to showing whatever
    /// `BakedImage`/solid color this component had before.
    pub fn stop_video(&self, app: &mut Proteus) {
        app.world
            .world
            .entity_mut(self.0)
            .remove::<VideoPlayer>()
            .remove::<VideoCrossfade>();
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
    pub fn set_video_crossfade(&self, app: &mut Proteus, video_t: f32) {
        if let Some(mut crossfade) = app.world.world.get_mut::<VideoCrossfade>(self.0) {
            crossfade.video_t = video_t;
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
    pub fn copy_baked_image_from(&self, app: &mut Proteus, source: Handle) -> bool {
        let Some(baked) = app.world.world.get::<BakedImage>(source.0).cloned() else {
            return false;
        };
        let texture_ref = app.world.world.get::<TextureRef>(source.0).copied();
        let mut entity = app.world.world.entity_mut(self.0);
        entity.insert(baked);
        if let Some(texture_ref) = texture_ref {
            entity.insert(texture_ref);
        }
        true
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
    pub fn center_crop_to_square(&self, app: &mut Proteus) -> bool {
        let Some(baked) = app.world.world.get::<BakedImage>(self.0).cloned() else {
            return false;
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
        app.world.world.entity_mut(self.0).insert(BakedImage {
            uv_offset,
            uv_scale,
            page: baked.page,
            pixel_size: baked.pixel_size,
        });
        true
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
    pub fn set_interactive(&self, app: &mut Proteus, interactive: bool) {
        if interactive {
            app.world.world.entity_mut(self.0).insert(Interactable);
        } else {
            app.world.world.entity_mut(self.0).remove::<Interactable>();
        }
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
    ) {
        let group_targets = targets
            .iter()
            .map(|h| GroupTarget {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        app.world.world.entity_mut(self.0).insert(OneToNRequest {
            targets: group_targets,
            default_config: config,
            child_behavior: None,
            strategy,
        });
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
    ) {
        let group_targets = targets
            .iter()
            .map(|(h, state)| GroupTarget {
                entity: h.0,
                state: state.clone(),
            })
            .collect();
        app.world.world.entity_mut(self.0).insert(OneToNRequest {
            targets: group_targets,
            default_config: config,
            child_behavior: None,
            strategy,
        });
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
    ) {
        let group_sources = sources
            .iter()
            .map(|h| GroupSource {
                entity: h.0,
                state: declared_geometry(app, h.0),
            })
            .collect();
        app.world.world.entity_mut(self.0).insert(NToOneRequest {
            sources: group_sources,
            default_config: config,
            child_behavior: None,
            layout,
        });
    }

    /// Parent `child` to this component — `child`'s `QuadState` becomes
    /// relative to this component's own (M10).
    pub fn add_child(&self, app: &mut Proteus, child: Handle) {
        app.world.world.entity_mut(child.0).insert(ChildOf(self.0));
    }

    /// Detach `child` from this component. `destroy: false` leaves `child`
    /// alive as its own root entity; `destroy: true` despawns it.
    pub fn remove_child(&self, app: &mut Proteus, child: Handle, destroy: bool) {
        if destroy {
            app.world.world.despawn(child.0);
        } else {
            app.world.world.entity_mut(child.0).remove::<ChildOf>();
        }
    }

    /// Remove this component from the ECS entirely. `bevy_ecs`'s
    /// `ChildOf`/`Children` relationship cascades the despawn to every
    /// descendant automatically.
    pub fn destroy(self, app: &mut Proteus) {
        app.world.world.despawn(self.0);
    }

    /// Release the GPU resources this component references (baked text,
    /// baked image, baked composite) while leaving the entity itself in the
    /// ECS. Removing `TextureRef` triggers M11's `ComponentHooks` to decref
    /// the shared `TextureRegistry` automatically — this is the correct way
    /// to free a texture a component owns; see [`TextureHandle`]'s doc for
    /// why it has no `.free()` of its own.
    pub fn free_resources(&self, app: &mut Proteus) {
        app.world
            .world
            .entity_mut(self.0)
            .remove::<TextureRef>()
            .remove::<BakedImage>()
            .remove::<BakedText>()
            .remove::<BakedComposite>();
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
    pub fn set_texture(&self, app: &mut Proteus, texture: TextureHandle) -> bool {
        let Some(pipeline) = app
            .world
            .world
            .get_resource::<proteus_render::QuadPipeline>()
        else {
            return false;
        };
        let Some(uv) = pipeline.texture_registry.main_atlas_uv(texture.0) else {
            return false;
        };
        let Some((_, width, height)) = pipeline.texture_registry.info(texture.0) else {
            return false;
        };
        app.world.world.entity_mut(self.0).insert((
            BakedImage {
                uv_offset: uv.uv_offset,
                uv_scale: uv.uv_scale,
                page: uv.page,
                pixel_size: [width as f32, height as f32],
            },
            TextureRef(texture.0),
        ));
        true
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
    /// resolved here via the internal [`DeclaredGeometry`] `component()`
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
