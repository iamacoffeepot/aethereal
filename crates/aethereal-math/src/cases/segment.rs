//! The cases of `Segment`: what it answers about a point, and whether two segments cross. The WGSL `Segment` of
//! `aethereal-render`'s shader package follows them (ADR-0030).
//!
//! Every coordinate is a small number of quarters, so each product the answers are made of is exact in `f32`, and the
//! sides, the containment, and the crossings have one answer on any machine, fused multiply-add or not. The lengths
//! and distances are rounded, and are compared within a tolerance.

use crate::{Segment, Side, Vec2};
use core::f32::consts::SQRT_2;

/// What a segment answers about a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointCase {
    /// The segment asked.
    pub segment: Segment,
    /// The point asked about.
    pub point: Vec2,
    /// `segment.along(point)`, to the foot on the whole line.
    pub along: f32,
    /// `segment.closest_point(point)`, clipped to the segment.
    pub closest: Vec2,
    /// `segment.distance_to(point)`, clipped to the segment.
    pub distance: f32,
    /// `segment.side(point)`.
    pub side: Side,
    /// `segment.contains_point(point)`.
    pub contains: bool,
}

/// Whether two segments cross.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossingCase {
    /// The segment asked.
    pub segment: Segment,
    /// The segment it is asked about.
    pub other: Segment,
    /// `segment.crosses(other)`.
    pub crosses: bool,
}

const fn segment(start_x: f32, start_y: f32, end_x: f32, end_y: f32) -> Segment {
    Segment::new(Vec2::new(start_x, start_y), Vec2::new(end_x, end_y))
}

const fn point(
    segment: Segment,
    point: Vec2,
    along: f32,
    closest: Vec2,
    distance: f32,
    side: Side,
    contains: bool,
) -> PointCase {
    PointCase { segment, point, along, closest, distance, side, contains }
}

const fn crossing(segment: Segment, other: Segment, crosses: bool) -> CrossingCase {
    CrossingCase { segment, other, crosses }
}

const EASTWARD: Segment = segment(1.0, 1.0, 3.0, 1.0);
const WESTWARD: Segment = segment(3.0, 1.0, 1.0, 1.0);
const DIAGONAL: Segment = segment(0.0, 2.0, 2.0, 0.0);
const POINT: Segment = segment(1.0, 1.0, 1.0, 1.0);
const BASE: Segment = segment(0.0, 0.0, 2.0, 0.0);
const RISING: Segment = segment(0.0, 0.0, 2.0, 2.0);
const FALLING: Segment = segment(0.0, 2.0, 2.0, 0.0);

/// A segment's answers about points: before, past, beside, on, and at an end of it, on either side, for a segment run
/// either way, a diagonal, and a segment of zero length.
pub const POINTS: &[PointCase] = &[
    // Before the start: the foot runs back past it, and the distance is to the start.
    point(EASTWARD, Vec2::new(0.0, 2.0), -1.0, Vec2::new(1.0, 1.0), SQRT_2, Side::Positive, false),
    // On the line past the end: on the line, off the segment.
    point(EASTWARD, Vec2::new(5.0, 1.0), 4.0, Vec2::new(3.0, 1.0), 2.0, Side::On, false),
    point(EASTWARD, Vec2::new(2.0, 4.0), 1.0, Vec2::new(2.0, 1.0), 3.0, Side::Positive, false),
    point(EASTWARD, Vec2::new(2.0, -2.0), 1.0, Vec2::new(2.0, 1.0), 3.0, Side::Negative, false),
    point(EASTWARD, Vec2::new(2.0, 1.0), 1.0, Vec2::new(2.0, 1.0), 0.0, Side::On, true),
    point(EASTWARD, Vec2::new(3.0, 1.0), 2.0, Vec2::new(3.0, 1.0), 0.0, Side::On, true),
    // The side follows the direction of travel: the same point is on the other side of the segment run back.
    point(WESTWARD, Vec2::new(2.0, 4.0), 1.0, Vec2::new(2.0, 1.0), 3.0, Side::Negative, false),
    point(WESTWARD, Vec2::new(0.0, 2.0), 3.0, Vec2::new(1.0, 1.0), SQRT_2, Side::Negative, false),
    point(DIAGONAL, Vec2::new(1.0, 1.0), SQRT_2, Vec2::new(1.0, 1.0), 0.0, Side::On, true),
    point(DIAGONAL, Vec2::new(0.0, 2.0), 0.0, Vec2::new(0.0, 2.0), 0.0, Side::On, true),
    point(DIAGONAL, Vec2::new(3.0, -1.0), 3.0 * SQRT_2, Vec2::new(2.0, 0.0), SQRT_2, Side::On, false),
    point(DIAGONAL, Vec2::new(1.0, 1.5), 0.75 * SQRT_2, Vec2::new(0.75, 1.25), SQRT_2 / 4.0, Side::Positive, false),
    // A segment of zero length is measured as its point, and has no line or side.
    point(POINT, Vec2::new(4.0, 5.0), 0.0, Vec2::new(1.0, 1.0), 5.0, Side::On, false),
    point(POINT, Vec2::new(1.0, 1.0), 0.0, Vec2::new(1.0, 1.0), 0.0, Side::On, true),
];

/// Whether segments cross: through each other either way round, short of each other, touching at an end or with an end
/// on the other, on one line with a stretch, an end, or a gap between them, parallel, and a segment of zero length.
pub const CROSSINGS: &[CrossingCase] = &[
    crossing(RISING, FALLING, true),
    crossing(FALLING, RISING, true),
    crossing(RISING, segment(2.0, 0.0, 0.0, 2.0), true),
    crossing(RISING, segment(0.0, 2.0, 0.5, 1.5), false),
    crossing(BASE, segment(1.0, -1.0, 1.0, 1.0), true),
    // A shared end, and an end resting on the other segment, are touches.
    crossing(BASE, segment(2.0, 0.0, 3.0, 1.0), false),
    crossing(BASE, segment(1.0, 0.0, 1.0, 1.0), false),
    crossing(segment(1.0, 0.0, 1.0, 1.0), BASE, false),
    // On one line, a shared stretch crosses, whichever way the other runs; a shared end or a gap does not.
    crossing(BASE, segment(1.0, 0.0, 3.0, 0.0), true),
    crossing(BASE, segment(3.0, 0.0, 1.0, 0.0), true),
    crossing(BASE, segment(2.0, 0.0, 3.0, 0.0), false),
    crossing(BASE, segment(2.5, 0.0, 3.0, 0.0), false),
    crossing(BASE, segment(0.0, 1.0, 2.0, 1.0), false),
    // A segment of zero length is a point, and crosses nothing, even lying inside the other.
    crossing(BASE, segment(1.0, 0.0, 1.0, 0.0), false),
    crossing(segment(1.0, 0.0, 1.0, 0.0), BASE, false),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::{close, close_vec2};

    // `Segment` answers its own table: a change to its meanings fails here, and the shader port that follows the table
    // is held to the change.
    #[test]
    fn segment_answers_its_points_as_the_table_says() {
        for case in POINTS {
            let PointCase { segment, point, .. } = *case;
            let along = close(segment.along(point), case.along, 1e-6);
            let closest = close_vec2(segment.closest_point(point), case.closest, 1e-6);
            let distance = close(segment.distance_to(point), case.distance, 1e-6);
            assert!(along, "along of {case:?} is {}", segment.along(point));
            assert!(closest, "closest point of {case:?} is {:?}", segment.closest_point(point));
            assert!(distance, "distance of {case:?} is {}", segment.distance_to(point));
            assert_eq!(segment.side(point), case.side, "side of {case:?}");
            assert_eq!(segment.contains_point(point), case.contains, "containment of {case:?}");
        }
    }

    #[test]
    fn segments_cross_as_the_table_says() {
        for case in CROSSINGS {
            assert_eq!(case.segment.crosses(case.other), case.crosses, "crossing of {case:?}");
        }
    }
}
