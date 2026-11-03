// Tests of navigation's geometry: the nearest component in a direction, and
// reading order.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use proteus_ui::navigation::{nearest_in_direction, reading_order};
use proteus_ui::{QuadState, ReadingDirection, Rect};

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        min: Vec2::new(x - w / 2.0, y - h / 2.0),
        max: Vec2::new(x + w / 2.0, y + h / 2.0),
    }
}

fn entities(n: u32) -> Vec<Entity> {
    let mut world = World::new();
    (0..n).map(|_| world.spawn_empty().id()).collect()
}

#[test]
fn a_component_in_line_wins_over_a_nearer_one_at_an_angle() {
    let e = entities(2);
    let from = rect(0.0, 0.0, 100.0, 40.0);
    let in_line = (e[0], rect(0.0, -200.0, 100.0, 40.0));
    let at_angle = (e[1], rect(140.0, -60.0, 100.0, 40.0));
    assert_eq!(
        nearest_in_direction(&from, Vec2::NEG_Y, &[at_angle, in_line]),
        Some(e[0])
    );
}

#[test]
fn without_one_in_line_the_nearest_wins() {
    let e = entities(2);
    let from = rect(0.0, 0.0, 100.0, 40.0);
    let near = (e[0], rect(150.0, -80.0, 100.0, 40.0));
    let far = (e[1], rect(400.0, -300.0, 100.0, 40.0));
    assert_eq!(
        nearest_in_direction(&from, Vec2::NEG_Y, &[far, near]),
        Some(e[0])
    );
}

#[test]
fn only_components_beyond_the_edge_count() {
    let e = entities(2);
    let from = rect(0.0, 0.0, 100.0, 40.0);
    let above = (e[0], rect(0.0, 100.0, 100.0, 40.0));
    let beside = (e[1], rect(150.0, 0.0, 100.0, 40.0));
    assert_eq!(
        nearest_in_direction(&from, Vec2::NEG_Y, &[above, beside]),
        None
    );
    assert_eq!(
        nearest_in_direction(&from, Vec2::X, &[above, beside]),
        Some(e[1])
    );
}

// Two buttons whose tops differ by a few pixels still read as one row.
#[test]
fn reading_order_goes_by_rows_then_across() {
    let e = entities(4);
    let items = [
        (e[0], rect(200.0, 100.0, 100.0, 40.0)),
        (e[1], rect(0.0, 104.0, 100.0, 40.0)),
        (e[2], rect(0.0, 0.0, 100.0, 40.0)),
        (e[3], rect(200.0, 0.0, 100.0, 40.0)),
    ];
    let ltr: Vec<_> = reading_order(&items, ReadingDirection::LeftToRight)
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    assert_eq!(ltr, vec![e[1], e[0], e[2], e[3]]);
    let rtl: Vec<_> = reading_order(&items, ReadingDirection::RightToLeft)
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    assert_eq!(rtl, vec![e[0], e[1], e[3], e[2]]);
}

#[test]
fn a_rotated_component_is_measured_by_the_box_around_it() {
    let qs = QuadState {
        position: Vec3::ZERO,
        size: Vec2::new(100.0, 100.0),
        rotation: std::f32::consts::FRAC_PI_4,
        ..QuadState::default()
    };
    let r = Rect::around(&qs);
    let half_diagonal = 50.0 * std::f32::consts::SQRT_2;
    assert!((r.max.x - half_diagonal).abs() < 0.01);
    assert!((r.min.y + half_diagonal).abs() < 0.01);
}
