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
    BakedComposite, BakedImage, BakedText, GroupSource, GroupTarget, MergeLayout, NToOneRequest,
    OneToNRequest, QuadState, SignalId, SplitStrategy, TextureRef, TransitionConfig,
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
    pub fn baked_text_size(&self, app: &Proteus) -> Option<Vec2> {
        app.world
            .world
            .get::<BakedText>(self.0)
            .map(|b| Vec2::from(b.pixel_size))
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
