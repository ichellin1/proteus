//! Converting between world units and the top-left coordinates windows and
//! web pages usually use.
//!
//! Proteus places top-level components in **world units**: logical pixels
//! from the center of the viewport, with y pointing up. Windows, the web's
//! canvas and most image tools measure from the top-left corner instead,
//! with y pointing down. These convert between the two, in either direction.
//! With a `proteus_runtime::Viewport`, its methods of the same names are
//! shorter.

use glam::Vec2;

/// Converts `point`, in logical pixels from the viewport's top-left corner
/// with y pointing down, to world units: from its center, with y pointing up.
/// `viewport_size` is the viewport's size in logical pixels.
///
/// ```
/// # use proteus_sdk::glam::Vec2;
/// # use proteus_sdk::top_left_to_world;
/// let size = Vec2::new(800.0, 600.0);
/// // The top-left corner, and 20 pixels in from the left and 40 down.
/// assert_eq!(top_left_to_world(Vec2::ZERO, size), Vec2::new(-400.0, 300.0));
/// assert_eq!(top_left_to_world(Vec2::new(20.0, 40.0), size), Vec2::new(-380.0, 260.0));
/// ```
pub fn top_left_to_world(point: Vec2, viewport_size: Vec2) -> Vec2 {
    Vec2::new(
        point.x - viewport_size.x / 2.0,
        viewport_size.y / 2.0 - point.y,
    )
}

/// Converts `point`, in world units, back to logical pixels from the
/// viewport's top-left corner with y pointing down: the inverse of
/// [`top_left_to_world`]. `viewport_size` is the viewport's size in logical
/// pixels.
pub fn world_to_top_left(point: Vec2, viewport_size: Vec2) -> Vec2 {
    Vec2::new(
        point.x + viewport_size.x / 2.0,
        viewport_size.y / 2.0 - point.y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = Vec2::new(800.0, 600.0);

    #[test]
    fn the_corners_and_center_convert() {
        assert_eq!(
            top_left_to_world(Vec2::ZERO, SIZE),
            Vec2::new(-400.0, 300.0)
        );
        assert_eq!(top_left_to_world(SIZE / 2.0, SIZE), Vec2::ZERO);
        assert_eq!(top_left_to_world(SIZE, SIZE), Vec2::new(400.0, -300.0));
        assert_eq!(
            world_to_top_left(Vec2::new(-400.0, 300.0), SIZE),
            Vec2::ZERO
        );
        assert_eq!(world_to_top_left(Vec2::ZERO, SIZE), SIZE / 2.0);
    }

    #[test]
    fn converting_there_and_back_gives_the_point_back() {
        for point in [
            Vec2::new(20.0, 40.0),
            Vec2::new(-15.0, 900.0),
            Vec2::new(799.5, 0.25),
        ] {
            assert_eq!(
                world_to_top_left(top_left_to_world(point, SIZE), SIZE),
                point
            );
        }
    }
}
