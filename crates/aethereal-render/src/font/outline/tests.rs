//! A contour walked from TrueType's points: its implied on-curve points, and where it starts.

use super::*;

/// A point on the curve at `x` and `y`.
const fn on(x: f32, y: f32) -> Point {
    Point { at: Vec2::new(x, y), on_curve: true }
}

/// A control point at `x` and `y`.
const fn off(x: f32, y: f32) -> Point {
    Point { at: Vec2::new(x, y), on_curve: false }
}

#[test]
fn two_control_points_in_a_row_imply_a_point_midway_between_them() {
    // Catches the implied on-curve point left out, which joins two curves into one wrong one, or put anywhere but
    // midway: every round letter's outline would bulge or dent.
    let contour = Contour::from_points(&[on(0.0, 0.0), off(10.0, 0.0), off(10.0, 10.0), on(0.0, 10.0)]);

    let expected = vec![
        Segment::Quadratic { from: Vec2::new(0.0, 0.0), control: Vec2::new(10.0, 0.0), to: Vec2::new(10.0, 5.0) },
        Segment::Quadratic { from: Vec2::new(10.0, 5.0), control: Vec2::new(10.0, 10.0), to: Vec2::new(0.0, 10.0) },
        Segment::Line { from: Vec2::new(0.0, 10.0), to: Vec2::new(0.0, 0.0) },
    ];
    assert_eq!(contour.segments, expected);
}

#[test]
fn a_contour_of_only_control_points_starts_between_its_last_and_first() {
    // Catches a contour of no on-curve point, as a TrueType circle can be, walked from a control point as if it were
    // on the curve: the circle would grow corners.
    let contour = Contour::from_points(&[off(0.0, 10.0), off(10.0, 10.0), off(10.0, 0.0), off(0.0, 0.0)]);

    let expected = vec![
        Segment::Quadratic { from: Vec2::new(0.0, 5.0), control: Vec2::new(0.0, 10.0), to: Vec2::new(5.0, 10.0) },
        Segment::Quadratic { from: Vec2::new(5.0, 10.0), control: Vec2::new(10.0, 10.0), to: Vec2::new(10.0, 5.0) },
        Segment::Quadratic { from: Vec2::new(10.0, 5.0), control: Vec2::new(10.0, 0.0), to: Vec2::new(5.0, 0.0) },
        Segment::Quadratic { from: Vec2::new(5.0, 0.0), control: Vec2::new(0.0, 0.0), to: Vec2::new(0.0, 5.0) },
    ];
    assert_eq!(contour.segments, expected);
}
