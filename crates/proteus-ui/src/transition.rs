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

/// How a transition speeds up and slows down: a curve from linear progress,
/// `0` to `1`, to eased progress. The default is [`Easing::EaseInOutQuad`].
///
/// For a curve that isn't built in, use [`Easing::CubicBezier`], which takes the
/// same four numbers as CSS's `cubic-bezier()`, so a curve can be copied from CSS
/// or any easing tool. Rust code can also pass any function with
/// [`Easing::Custom`].
///
/// Eased progress may go past `0` or `1`, for a curve that overshoots and
/// settles back. The geometry follows it, but sizes and corner radii never go
/// below zero, and colors stay within `0`–`1`.
///
/// # Examples
///
/// ```
/// use proteus_ui::{Easing, TransitionConfig};
///
/// // Overshoots its target slightly, then settles: CSS's "back out" curve.
/// let config = TransitionConfig {
///     duration: 0.4,
///     easing: Easing::CubicBezier { x1: 0.34, y1: 1.56, x2: 0.64, y2: 1.0 },
///     ..TransitionConfig::default()
/// };
/// assert!(config.easing.apply(0.5) > 0.5);
/// ```
#[derive(Copy, Clone, Debug, Default)]
#[non_exhaustive]
pub enum Easing {
    /// Constant speed.
    Linear,
    /// Starts slowly, then speeds up.
    EaseInQuad,
    /// Starts quickly, then slows to a stop.
    EaseOutQuad,
    /// Starts slowly, speeds up, then slows to a stop. The default.
    #[default]
    EaseInOutQuad,
    /// Like [`Easing::EaseOutQuad`], with a stronger slowdown at the end.
    EaseOutCubic,
    /// A cubic Bézier curve from `(0, 0)` to `(1, 1)` through the control
    /// points `(x1, y1)` and `(x2, y2)`, as in CSS's
    /// `cubic-bezier(x1, y1, x2, y2)`. `x1` and `x2` are clamped to `0`–`1`;
    /// `y1` and `y2` may go outside it, for a curve that overshoots.
    CubicBezier {
        /// The first control point's x, from `0` to `1`.
        x1: f32,
        /// The first control point's y.
        y1: f32,
        /// The second control point's x, from `0` to `1`.
        x2: f32,
        /// The second control point's y.
        y2: f32,
    },
    /// Any function from linear to eased progress, for Rust code. It should
    /// map `0` to `0` and `1` to `1`, or the transition jumps at its start or
    /// end.
    Custom(fn(f32) -> f32),
}

impl Easing {
    /// Returns the eased progress for linear progress `t`, which is first
    /// clamped to `0`–`1`.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseInQuad => t * t,
            Easing::EaseOutQuad => t * (2.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
            Easing::EaseOutCubic => {
                let u = 1.0 - t;
                1.0 - u * u * u
            }
            Easing::CubicBezier { x1, y1, x2, y2 } => cubic_bezier(x1, y1, x2, y2, t),
            Easing::Custom(f) => f(t),
        }
    }
}

/// Evaluates the CSS-style cubic Bézier `(x1, y1, x2, y2)` at time `t`: finds
/// the curve parameter whose x is `t`, then returns that point's y.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    // With x1 and x2 in 0..=1, x rises steadily with the curve parameter, so
    // there is exactly one parameter for each t.
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    // Polynomial coefficients, so each coordinate is ((a*s + b)*s + c)*s.
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let x_at = |s: f32| ((ax * s + bx) * s + cx) * s;
    let y_at = |s: f32| ((ay * s + by) * s + cy) * s;
    let dx_at = |s: f32| (3.0 * ax * s + 2.0 * bx) * s + cx;

    // Newton's method converges in a few steps on most curves...
    let mut s = t;
    for _ in 0..8 {
        let err = x_at(s) - t;
        if err.abs() < 1e-6 {
            return y_at(s);
        }
        let slope = dx_at(s);
        if slope.abs() < 1e-6 {
            break;
        }
        s -= err / slope;
    }
    // ...and bisection finishes the ones where the slope is too flat for it.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    s = t;
    for _ in 0..32 {
        let x = x_at(s);
        if (x - t).abs() < 1e-6 {
            break;
        }
        if x < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) * 0.5;
    }
    y_at(s)
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
    /// How long the transition takes, in seconds. `0.0` means instant: the
    /// transition completes on the next tick, after any `delay`. A negative
    /// or NaN duration logs a warning and is treated as `0.0`.
    pub duration: f32,
    /// Seconds to wait before t starts advancing. Useful for staggered animations.
    pub delay: f32,
    /// How the transition speeds up and slows down. Defaults to
    /// [`Easing::EaseInOutQuad`].
    pub easing: Easing,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            duration: 0.3,
            delay: 0.0,
            easing: Easing::default(),
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
    /// `true` for a change of interaction style, such as a hover effect,
    /// started by [`crate::interaction::interaction_style_system`]. It leaves
    /// the entity's `Lifecycle` alone, so the entity keeps taking input, and
    /// its completion isn't recorded in [`CompletedTransitions`].
    pub interaction_style: bool,
}

impl ActiveTransition {
    /// Starts a transition from `from` to `to`. A negative or NaN
    /// `config.duration` logs a warning and is treated as `0.0`, instant.
    pub fn new(from: QuadState, to: QuadState, config: TransitionConfig) -> Self {
        // `!(d >= 0.0)` is true for NaN as well as for negative values.
        let duration = if config.duration >= 0.0 {
            config.duration
        } else {
            log::warn!(
                "TransitionConfig::duration is {}; treating it as 0.0, instant",
                config.duration
            );
            0.0
        };
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
            interaction_style: false,
        }
    }

    /// Progress through the transition, from `0.0` to `1.0`, before easing:
    /// `0.0` during the delay, and `1.0` as soon as the delay is over for a
    /// duration of `0.0`.
    pub fn raw_t(&self) -> f32 {
        if self.delay_remaining > 0.0 {
            0.0
        } else if self.config.duration > 0.0 {
            (self.elapsed / self.config.duration).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// Starts a change of interaction style from `from` to `to`; see
    /// [`ActiveTransition::interaction_style`](Self#structfield.interaction_style).
    pub fn for_interaction_style(from: QuadState, to: QuadState, config: TransitionConfig) -> Self {
        Self {
            interaction_style: true,
            ..Self::new(from, to, config)
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
/// entity is: a channel's `to` starts from its `from`, and a
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

        if active.delay_remaining > 0.0 {
            continue;
        }
        // An instant transition completes even on a tick with no time step.
        if effective_dt == 0.0 && active.config.duration > 0.0 {
            continue;
        }

        active.elapsed += effective_dt;
        let raw_t = active.raw_t();
        let eased_t = active.config.easing.apply(raw_t);

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
/// their `Lifecycle` restored to `Idle`. A change of interaction style is
/// removed without being recorded, and its `Lifecycle` was never changed.
///
/// Clears `CompletedTransitions` at the top of each call so the resource always
/// holds exactly this tick's completions.
pub fn transition_complete_system(
    mut commands: Commands,
    // Exclude Virtual entities — their completions are handled by
    // `group_transition_complete_system` in `topology.rs`.
    // `Lifecycle` is optional: a change of interaction style doesn't give the
    // entity one.
    mut query: Query<(Entity, &ActiveTransition, Option<&mut Lifecycle>), Without<Virtual>>,
    mut completed: ResMut<CompletedTransitions>,
) {
    completed.entities.clear();
    for (entity, active, lifecycle) in query.iter_mut() {
        if active.is_complete {
            if !active.interaction_style {
                if let Some(mut lifecycle) = lifecycle {
                    *lifecycle = Lifecycle::Idle;
                }
                completed.entities.push(entity);
            }
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

    // An overshooting easing curve passes `t` outside 0..=1 to `lerp`. The
    // result must stay drawable: no negative size, corner radius or scale, and
    // colors within 0..=1.
    #[test]
    fn lerp_past_either_end_stays_drawable() {
        let small = QuadState {
            size: Vec2::new(10.0, 10.0),
            scale: 0.5,
            corner_radius: 2.0,
            color: Vec4::new(0.2, 0.2, 0.2, 0.5),
            ..state_a()
        };
        let large = QuadState {
            size: Vec2::new(100.0, 100.0),
            scale: 1.0,
            corner_radius: 20.0,
            color: Vec4::new(1.0, 1.0, 1.0, 1.0),
            ..state_b()
        };
        for t in [-0.5, 1.5] {
            let q = small.lerp(&large, t);
            assert!(
                q.size.x >= 0.0 && q.size.y >= 0.0,
                "size at {t}: {:?}",
                q.size
            );
            assert!(q.corner_radius >= 0.0, "corner radius at {t}");
            assert!(q.scale >= 0.0, "scale at {t}");
            assert!(
                q.color.cmpge(Vec4::ZERO).all() && q.color.cmple(Vec4::ONE).all(),
                "color at {t}"
            );
        }
        // Overshoot still moves past the target where it's safe to.
        assert!(small.lerp(&large, 1.5).size.x > large.size.x);
    }

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

    // --- Easing ---

    const BUILT_INS: [Easing; 5] = [
        Easing::Linear,
        Easing::EaseInQuad,
        Easing::EaseOutQuad,
        Easing::EaseInOutQuad,
        Easing::EaseOutCubic,
    ];

    #[test]
    fn every_built_in_maps_0_to_0_and_1_to_1() {
        for e in BUILT_INS {
            assert!(e.apply(0.0).abs() < 1e-6, "{e:?} at 0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6, "{e:?} at 1");
        }
    }

    #[test]
    fn linear_is_identity() {
        assert!((Easing::Linear.apply(0.3) - 0.3).abs() < 1e-6);
        assert!((Easing::Linear.apply(0.7) - 0.7).abs() < 1e-6);
    }

    #[test]
    fn ease_in_quad_is_behind_linear_at_the_midpoint() {
        assert!((Easing::EaseInQuad.apply(0.5) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn ease_out_quad_is_ahead_of_linear_at_the_midpoint() {
        assert!((Easing::EaseOutQuad.apply(0.5) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn ease_in_out_quad_is_symmetric_around_the_midpoint() {
        let e = Easing::EaseInOutQuad;
        assert!((e.apply(0.5) - 0.5).abs() < 1e-6);
        assert!((e.apply(0.3) + e.apply(0.7) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ease_out_cubic_is_ahead_of_ease_out_quad_at_the_midpoint() {
        assert!(Easing::EaseOutCubic.apply(0.5) > Easing::EaseOutQuad.apply(0.5));
    }

    #[test]
    fn progress_outside_0_to_1_is_clamped() {
        for e in BUILT_INS {
            assert_eq!(e.apply(-0.5), e.apply(0.0), "{e:?} below 0");
            assert_eq!(e.apply(1.5), e.apply(1.0), "{e:?} above 1");
        }
    }

    #[test]
    fn a_straight_bezier_is_linear() {
        // cubic-bezier(0, 0, 1, 1) is a straight line.
        let e = Easing::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        for t in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] {
            assert!((e.apply(t) - t).abs() < 1e-4, "at {t}: {}", e.apply(t));
        }
    }

    #[test]
    fn a_bezier_matches_css_reference_values() {
        // CSS's `ease`, cubic-bezier(0.25, 0.1, 0.25, 1), at t = 0.5 is about
        // 0.8024 (the same value browsers compute).
        let ease = Easing::CubicBezier {
            x1: 0.25,
            y1: 0.1,
            x2: 0.25,
            y2: 1.0,
        };
        assert!(
            (ease.apply(0.5) - 0.8024).abs() < 1e-3,
            "{}",
            ease.apply(0.5)
        );
        assert!(ease.apply(0.0).abs() < 1e-6);
        assert!((ease.apply(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_bezier_can_overshoot() {
        // A "back out" curve goes past 1 before settling there.
        let back = Easing::CubicBezier {
            x1: 0.34,
            y1: 1.56,
            x2: 0.64,
            y2: 1.0,
        };
        let peak = (1..100)
            .map(|i| back.apply(i as f32 / 100.0))
            .fold(0.0f32, f32::max);
        assert!(peak > 1.05, "peak {peak}");
        assert!((back.apply(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bezier_x_outside_0_to_1_is_clamped_not_broken() {
        let e = Easing::CubicBezier {
            x1: -2.0,
            y1: 0.0,
            x2: 3.0,
            y2: 1.0,
        };
        let clamped = Easing::CubicBezier {
            x1: 0.0,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        };
        for t in [0.2, 0.5, 0.9] {
            assert!((e.apply(t) - clamped.apply(t)).abs() < 1e-4);
        }
    }

    #[test]
    fn custom_easing_calls_the_function() {
        fn half_step(t: f32) -> f32 {
            if t < 0.5 {
                0.0
            } else {
                1.0
            }
        }
        assert_eq!(Easing::Custom(half_step).apply(0.4), 0.0);
        assert_eq!(Easing::Custom(half_step).apply(0.6), 1.0);
    }

    // --- TransitionConfig default ---

    #[test]
    fn transition_config_default_values() {
        let cfg = TransitionConfig::default();
        assert!((cfg.duration - 0.3).abs() < 1e-6);
        assert!(cfg.delay == 0.0);
        assert!(matches!(cfg.easing, Easing::EaseInOutQuad));
    }
}
