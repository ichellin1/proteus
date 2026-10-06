//! 1→1 transitions: moving an entity's geometry from one `QuadState` to
//! another over time.
//!
//! Three systems run in order each tick:
//!
//! 1. [`transition_setup_system`] turns a `TransitionRequest` into an
//!    `ActiveTransition`;
//! 2. [`transition_tick_system`] advances it and interpolates the `QuadState`;
//! 3. [`transition_complete_system`] finishes the ones that reached the end
//!    and records them in [`CompletedTransitions`].

use bevy_ecs::prelude::*;

use crate::component::{Lifecycle, QuadState, TransitionRequest, Virtual};

// ---------------------------------------------------------------------------
// Easing
// ---------------------------------------------------------------------------

/// A function mapping normalized time `t ∈ [0,1]` to an eased `t ∈ [0,1]`.
///
/// A plain function pointer keeps `TransitionConfig` `Copy`, with no
/// allocation, so an easing curve can't capture state.
pub type EasingFn = fn(f32) -> f32;

/// t unchanged — constant velocity.
pub fn linear(t: f32) -> f32 {
    t
}

/// Accelerates from rest.
pub fn ease_in_quad(t: f32) -> f32 {
    t * t
}

/// Decelerates to rest.
pub fn ease_out_quad(t: f32) -> f32 {
    t * (2.0 - t)
}

/// Accelerates then decelerates — the most natural feel for UI motion.
pub fn ease_in_out_quad(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

/// Cubic ease-out — slightly more pronounced deceleration than quad.
pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

// ---------------------------------------------------------------------------
// TransitionConfig
// ---------------------------------------------------------------------------

/// Call-site configuration for one transition.
///
/// Part of a `TransitionRequest`. It applies to every field of the geometry;
/// splits and merges can give each piece its own.
#[derive(Copy, Clone, Debug)]
pub struct TransitionConfig {
    // DOC-REVIEW: accurate today; step 5 makes 0 instant and drops the panic,
    // and this doc changes with it.
    /// How long the transition takes, in seconds. Must be positive.
    ///
    /// Zero, a negative value or NaN is a mistake. In debug builds the
    /// transition panics when it starts; in release builds it completes on
    /// the next tick, as if instant.
    pub duration: f32,
    /// Seconds to wait before t starts advancing. Useful for staggered animations.
    pub delay: f32,
    /// Easing function applied to raw t before lerping.
    pub easing: EasingFn,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            duration: 0.3,
            delay: 0.0,
            easing: ease_in_out_quad,
        }
    }
}

// ---------------------------------------------------------------------------
// ActiveTransition — per-entity transition state
// ---------------------------------------------------------------------------

/// Attached to an entity for the duration of its active transition.
///
/// Removed by `transition_complete_system` when `t` reaches 1.0.
#[derive(Component, Debug)]
pub struct ActiveTransition {
    /// Snapshot of the visual state at transition start.
    pub from: QuadState,
    /// Target visual state.
    pub to: QuadState,
    /// Seconds elapsed in the active lerp phase (excludes delay).
    pub elapsed: f32,
    /// Seconds remaining in the delay phase before lerp starts.
    pub delay_remaining: f32,
    /// Config (duration, easing) for this transition.
    pub config: TransitionConfig,
    /// Set to `true` by `transition_tick_system` when `raw_t >= 1.0`.
    /// Read by `transition_complete_system` the same tick.
    /// Exposed `pub` so integration tests can inspect and seed this flag.
    pub is_complete: bool,
}

impl ActiveTransition {
    /// Starts a transition from `from` to `to`.
    ///
    /// # Panics
    ///
    /// In debug builds, if `config.duration` isn't positive. Release builds
    /// treat such a duration as almost instant.
    pub fn new(from: QuadState, to: QuadState, config: TransitionConfig) -> Self {
        debug_assert!(
            config.duration > 0.0,
            "TransitionConfig.duration must be positive (got {}); treating as instant",
            config.duration
        );
        // Clamp to 0.1 ms minimum so division in transition_tick_system never
        // produces NaN.  A zero or negative duration is a caller error; in
        // practice any non-zero tick will immediately satisfy raw_t >= 1.0.
        let duration = config.duration.max(1e-4);
        let delay_remaining = config.delay.max(0.0);
        Self {
            from,
            to,
            elapsed: 0.0,
            delay_remaining,
            config: TransitionConfig {
                duration,
                delay: config.delay,
                easing: config.easing,
            },
            is_complete: false,
        }
    }
}

// ---------------------------------------------------------------------------
// TransitionComplete event
// ---------------------------------------------------------------------------

/// The entities whose transitions finished this tick.
///
/// `transition_complete_system` clears it each tick, then adds each entity
/// that reached `t = 1.0`. Read it after `ProteusWorld::update`.
#[derive(Resource, Default)]
pub struct CompletedTransitions {
    /// The entities. For a split or merge, it is the entity coordinating it,
    /// the source or the destination, never its virtual pieces.
    pub entities: Vec<Entity>,
}

impl CompletedTransitions {
    /// Takes this tick's completed entities, leaving the list empty, so the
    /// same completion can't be handled twice.
    ///
    /// ```
    /// # use proteus_ui::{CompletedTransitions, ProteusWorld};
    /// # let mut world = ProteusWorld::new();
    /// for entity in world.world.resource_mut::<CompletedTransitions>().drain() {
    ///     // React to `entity`'s transition finishing.
    /// }
    /// ```
    pub fn drain(&mut self) -> Vec<Entity> {
        std::mem::take(&mut self.entities)
    }
}

// ---------------------------------------------------------------------------
// FrameTime resource
// ---------------------------------------------------------------------------

/// This tick's time step, set by `ProteusWorld::update`. Systems read it,
/// rather than a clock, so tests can control time.
#[derive(Resource, Default)]
pub struct FrameTime {
    /// Seconds since the previous tick.
    pub delta_secs: f32,
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Converts `TransitionRequest` components into `ActiveTransition` components.
///
/// Reads the current `QuadState` as the from-state (snapshot), inserts
/// `ActiveTransition`, **moves the entity to the from-state**, sets
/// `Lifecycle::Transitioning`, and removes the request.
///
/// ## Why the entity moves to the start at once
///
/// A `from_state` makes a transition start somewhere other than where the
/// entity is: a signal's `to` starts from its `from`, and a
/// [`SplitStrategy::PerTarget`](crate::SplitStrategy::PerTarget) split starts
/// every target from the source. The entity is moved there immediately, not on
/// the first tick of the transition, because:
///
/// - **during a `delay`,** nothing moves, so the entity would sit at its old
///   position, often its final one, and then jump back to the start;
/// - **even without a delay,** the transition is only picked up a tick later,
///   so the entity would show at its old position for one frame.
///
/// With no `from_state`, the start is where the entity already is.
pub fn transition_setup_system(
    mut commands: Commands,
    // `&Lifecycle` is intentionally excluded: any entity with a TransitionRequest
    // is moved to Transitioning regardless of its prior lifecycle state.
    query: Query<(Entity, &TransitionRequest, &QuadState)>,
) {
    for (entity, request, current_state) in query.iter() {
        // `from_state` may override the entity's current position — see the
        // doc above for why it's also written back to `QuadState` here.
        let from = request.from_state.as_ref().unwrap_or(current_state).clone();
        let active = ActiveTransition::new(
            from.clone(),
            request.to.clone(),
            request.config, // Copy — no .clone() needed
        );
        commands
            .entity(entity)
            .insert(active)
            .insert(from)
            .insert(Lifecycle::Transitioning)
            .remove::<TransitionRequest>();
    }
}

/// Advances `t` on all active transitions and lerps `QuadState`.
///
/// - Delay phase: burns off `delay_remaining` before advancing `elapsed`.
/// - Active phase: advances `elapsed`, computes eased `t`, lerps `QuadState`.
/// - When `raw_t >= 1.0`: snaps to final state, marks `is_complete = true`.
///
/// Does NOT remove the component — that is `transition_complete_system`'s job,
/// which runs after this one in the schedule.
pub fn transition_tick_system(
    time: Res<FrameTime>,
    mut query: Query<(&mut ActiveTransition, &mut QuadState)>,
) {
    let dt = time.delta_secs;
    for (mut active, mut state) in query.iter_mut() {
        // Burn off delay, then carry any leftover time into the lerp phase.
        // If the tick straddles the delay boundary the excess is not wasted.
        let effective_dt = if active.delay_remaining > 0.0 {
            let burned = dt.min(active.delay_remaining);
            active.delay_remaining -= burned;
            dt - burned // may be 0.0 if still fully in the delay phase
        } else {
            dt
        };

        if effective_dt == 0.0 {
            continue;
        }

        active.elapsed += effective_dt;
        let raw_t = (active.elapsed / active.config.duration).clamp(0.0, 1.0);
        let eased_t = (active.config.easing)(raw_t);

        *state = active.from.lerp(&active.to, eased_t);

        if raw_t >= 1.0 {
            // Snap to final state regardless of float precision.
            *state = active.to.clone();
            active.is_complete = true;
        }
    }
}

/// Finishes completed transitions and records them in `CompletedTransitions`.
///
/// Runs after `transition_tick_system` in the schedule. Entities whose
/// `ActiveTransition.is_complete` flag is set get the component removed and
/// their `Lifecycle` restored to `Idle`.
///
/// Clears `CompletedTransitions` at the top of each call so the resource always
/// holds exactly this tick's completions.
pub fn transition_complete_system(
    mut commands: Commands,
    // Exclude Virtual entities — their completions are handled by
    // `group_transition_complete_system` in `topology.rs`.
    mut query: Query<(Entity, &ActiveTransition, &mut Lifecycle), Without<Virtual>>,
    mut completed: ResMut<CompletedTransitions>,
) {
    completed.entities.clear();
    for (entity, active, mut lifecycle) in query.iter_mut() {
        if active.is_complete {
            *lifecycle = Lifecycle::Idle;
            completed.entities.push(entity);
            commands.entity(entity).remove::<ActiveTransition>();
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests — pure math (no World required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec2, Vec3, Vec4};

    fn state_a() -> QuadState {
        QuadState {
            position: Vec3::ZERO,
            size: Vec2::new(100.0, 100.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::new(1.0, 0.0, 0.0, 1.0),
            corner_radius: 0.0,
        }
    }

    fn state_b() -> QuadState {
        QuadState {
            position: Vec3::new(200.0, 300.0, 0.0),
            size: Vec2::new(400.0, 200.0),
            rotation: std::f32::consts::PI,
            scale: 2.0,
            anchor: Vec2::new(0.0, 0.0),
            color: Vec4::new(0.0, 1.0, 0.0, 1.0),
            corner_radius: 16.0,
        }
    }

    // --- QuadState::lerp ---

    #[test]
    fn lerp_at_t_zero_returns_from() {
        let a = state_a();
        let b = state_b();
        let out = a.lerp(&b, 0.0);
        assert_eq!(out.position, a.position);
        assert_eq!(out.size, a.size);
        assert_eq!(out.rotation, a.rotation);
        assert_eq!(out.scale, a.scale);
        assert_eq!(out.corner_radius, a.corner_radius);
    }

    #[test]
    fn lerp_at_t_one_returns_to() {
        let a = state_a();
        let b = state_b();
        let out = a.lerp(&b, 1.0);
        assert_eq!(out.position, b.position);
        assert_eq!(out.size, b.size);
        assert!((out.rotation - b.rotation).abs() < 1e-5);
        assert!((out.scale - b.scale).abs() < 1e-5);
        assert!((out.corner_radius - b.corner_radius).abs() < 1e-5);
    }

    #[test]
    fn lerp_at_t_half_is_midpoint() {
        let a = state_a();
        let b = state_b();
        let out = a.lerp(&b, 0.5);
        // position midpoint
        let mid_pos = (a.position + b.position) * 0.5;
        assert!((out.position - mid_pos).length() < 1e-4);
        // size midpoint
        let mid_size = (a.size + b.size) * 0.5;
        assert!((out.size - mid_size).length() < 1e-4);
        // corner_radius midpoint
        assert!((out.corner_radius - 8.0).abs() < 1e-5);
    }

    #[test]
    fn lerp_color_midpoint() {
        let a = state_a(); // red
        let b = state_b(); // green
        let out = a.lerp(&b, 0.5);
        assert!((out.color.x - 0.5).abs() < 1e-5, "R should be 0.5");
        assert!((out.color.y - 0.5).abs() < 1e-5, "G should be 0.5");
    }

    // --- Easing functions ---

    #[test]
    fn easing_boundaries_all_functions() {
        let fns: &[EasingFn] = &[
            linear,
            ease_in_quad,
            ease_out_quad,
            ease_in_out_quad,
            ease_out_cubic,
        ];
        for f in fns {
            assert!((f(0.0)).abs() < 1e-6, "f(0) should be 0");
            assert!((f(1.0) - 1.0).abs() < 1e-6, "f(1) should be 1");
        }
    }

    #[test]
    fn linear_is_identity() {
        assert!((linear(0.3) - 0.3).abs() < 1e-6);
        assert!((linear(0.7) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn ease_in_quad_slower_than_linear_at_midpoint() {
        // ease_in starts slow, so at t=0.5 it should be behind linear
        assert!(ease_in_quad(0.5) < 0.5);
        assert!((ease_in_quad(0.5) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn ease_out_quad_faster_than_linear_at_midpoint() {
        // ease_out decelerates, so at t=0.5 it should be ahead of linear
        assert!(ease_out_quad(0.5) > 0.5);
        assert!((ease_out_quad(0.5) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn ease_in_out_quad_symmetric_around_half() {
        // should equal 0.5 at midpoint and be symmetric: f(t) = 1 - f(1-t)
        assert!((ease_in_out_quad(0.5) - 0.5).abs() < 1e-6);
        let t = 0.3_f32;
        assert!((ease_in_out_quad(t) + ease_in_out_quad(1.0 - t) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ease_out_cubic_ahead_of_ease_out_quad_at_midpoint() {
        // cubic ease-out is more pronounced — reaches further at t=0.5
        assert!(ease_out_cubic(0.5) > ease_out_quad(0.5));
    }

    // --- TransitionConfig default ---

    #[test]
    fn transition_config_default_values() {
        let cfg = TransitionConfig::default();
        assert!((cfg.duration - 0.3).abs() < 1e-6);
        assert!(cfg.delay == 0.0);
        // easing should be ease_in_out_quad
        assert!((cfg.easing)(0.5) == ease_in_out_quad(0.5));
    }
}
