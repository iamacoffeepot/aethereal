use crate::{Segment, Vec2};

/// A polygon through points in order, borrowed: its boundary runs from each point to the next and
/// from the last back to the first.
///
/// The points may wind either way and the boundary may cross itself; what is inside is decided by
/// crossings (the even-odd rule). Fewer than three points enclose nothing: two are a segment and one
/// a point, which hold only their own points.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Polygon<'a> {
    /// The corners, in order around the boundary.
    pub points: &'a [Vec2],
}

impl<'a> Polygon<'a> {
    /// The polygon through `points`.
    #[inline]
    pub const fn new(points: &'a [Vec2]) -> Self {
        Self { points }
    }

    /// Returns the area enclosed, positive when the points wind counter-clockwise (each turn is
    /// toward [`Side::Positive`](crate::Side::Positive)) and negative clockwise. A boundary that
    /// crosses itself sums its loops, each signed by its own winding. Fewer than three points, or
    /// points on one line, enclose no area and answer zero.
    #[must_use]
    pub fn signed_area(self) -> f32 {
        let Some((&anchor, rest)) = self.points.split_first() else {
            return 0.0;
        };
        let doubled: f32 = rest.windows(2).map(|pair| (pair[0] - anchor).perp_dot(pair[1] - anchor)).sum();
        doubled * 0.5
    }

    /// Returns true when `point` is inside the polygon or on its boundary.
    ///
    /// Inside is where a ray from the point crosses the boundary an odd number of times, so a
    /// boundary that crosses itself leaves out the areas it covers twice. A point exactly on the
    /// boundary, a corner included, is inside, as a point on the edge of a [`Rect2`](crate::Rect2)
    /// is; the test is exact, so a point placed on an edge by arithmetic may land a rounding either
    /// side of it. A ray that passes through a corner counts it once, by taking each edge's lower
    /// end as on the edge and its upper end as off it.
    #[must_use]
    pub fn contains_point(self, point: Vec2) -> bool {
        let on_boundary = self.edges().any(|edge| edge.contains_point(point));
        let crossings = self.edges().filter(|edge| passes_right_of(*edge, point)).count();
        let odd = crossings % 2 == 1;
        on_boundary || odd
    }

    /// Returns the distance from `point` to the nearest point of the boundary, whether `point` is
    /// inside or outside. `None` when the polygon has no points.
    #[must_use]
    pub fn distance_to_boundary(self, point: Vec2) -> Option<f32> {
        self.edges().map(|edge| edge.distance_to(point)).reduce(f32::min)
    }

    /// Returns the edges of the boundary, the last running back to the first point. One point is
    /// one edge of zero length.
    fn edges(self) -> impl Iterator<Item = Segment> + 'a {
        let points = self.points;
        let followers = points.iter().cycle().skip(1);
        points.iter().zip(followers).map(|(start, end)| Segment::new(*start, *end))
    }
}

/// Returns true when `edge` crosses the ray from `point` toward positive `x`. An edge's lower end
/// counts as on the ray's level and its upper end as off it, so a ray through a corner meets the
/// two edges there once in all, or not at all where both run away on one side.
fn passes_right_of(edge: Segment, point: Vec2) -> bool {
    let start_above = edge.start.y > point.y;
    let end_above = edge.end.y > point.y;
    let spans = start_above != end_above;
    if !spans {
        return false;
    }
    let turn = edge.direction().perp_dot(point - edge.start);
    if end_above {
        turn > 0.0
    } else {
        turn < 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close;
    use core::f32::consts::SQRT_2;

    fn points<const COUNT: usize>(coordinates: [(f32, f32); COUNT]) -> [Vec2; COUNT] {
        coordinates.map(|(x, y)| Vec2::new(x, y))
    }

    // A U open at the top: its notch runs from x 1 to 2, down to y 1.
    fn u_shape() -> [Vec2; 8] {
        points([(0.0, 0.0), (3.0, 0.0), (3.0, 3.0), (2.0, 3.0), (2.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)])
    }

    // The area's sign follows the winding, and its size the shape, here a concave L of 3.
    #[test]
    fn signed_area_follows_winding() {
        let mut shape = points([(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]);
        let counter_clockwise = Polygon::new(&shape).signed_area();
        shape.reverse();
        let clockwise = Polygon::new(&shape).signed_area();
        assert!(close(counter_clockwise, 3.0, 1e-6), "{counter_clockwise}");
        assert!(close(clockwise, -3.0, 1e-6), "{clockwise}");
    }

    // Far from the origin a unit square still measures one, which a sum of raw corner products
    // in f32 loses.
    #[test]
    fn signed_area_holds_far_from_origin() {
        let far = points([(10000.0, 10000.0), (10001.0, 10000.0), (10001.0, 10001.0), (10000.0, 10001.0)]);
        let area = Polygon::new(&far).signed_area();
        assert!(close(area, 1.0, 1e-6), "{area}");
    }

    // A crossed bow tie sums its two loops, which wind opposite ways; too few points enclose none.
    #[test]
    fn signed_area_of_degenerate_shapes() {
        let bow_tie = points([(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)]);
        let collinear = points([(0.0, 0.0), (1.0, 0.0), (3.0, 0.0)]);
        assert_eq!(Polygon::new(&bow_tie).signed_area(), 0.0);
        assert_eq!(Polygon::new(&collinear).signed_area(), 0.0);
        assert_eq!(Polygon::new(&collinear[..2]).signed_area(), 0.0);
        assert_eq!(Polygon::new(&[]).signed_area(), 0.0);
    }

    // The arms of a concave U are inside and its notch outside, wound either way.
    #[test]
    fn concave_inside_and_outside() {
        let mut shape = u_shape();
        for _ in 0..2 {
            let polygon = Polygon::new(&shape);
            assert!(polygon.contains_point(Vec2::new(0.5, 2.0)));
            assert!(polygon.contains_point(Vec2::new(2.5, 2.0)));
            assert!(polygon.contains_point(Vec2::new(1.5, 0.5)));
            assert!(!polygon.contains_point(Vec2::new(1.5, 2.0)));
            assert!(!polygon.contains_point(Vec2::new(4.0, 2.0)));
            shape.reverse();
        }
    }

    // Rays level with the corners pass through them and still count each crossing once.
    #[test]
    fn rays_through_corners_count_once() {
        let shape = u_shape();
        let polygon = Polygon::new(&shape);
        assert!(polygon.contains_point(Vec2::new(0.5, 1.0)));
        assert!(!polygon.contains_point(Vec2::new(-1.0, 1.0)));
        assert!(!polygon.contains_point(Vec2::new(-1.0, 3.0)));
        assert!(!polygon.contains_point(Vec2::new(1.5, 3.0)));
        assert!(!polygon.contains_point(Vec2::new(-1.0, 0.0)));
    }

    // A point on a corner or an edge is inside, a reflex corner included.
    #[test]
    fn boundary_points_are_inside() {
        let shape = u_shape();
        let polygon = Polygon::new(&shape);
        assert!(polygon.contains_point(Vec2::new(1.0, 1.0)));
        assert!(polygon.contains_point(Vec2::new(3.0, 3.0)));
        assert!(polygon.contains_point(Vec2::new(0.0, 0.0)));
        assert!(polygon.contains_point(Vec2::new(1.5, 1.0)));
        assert!(polygon.contains_point(Vec2::new(0.0, 1.5)));
    }

    // Too few points hold only their own.
    #[test]
    fn degenerate_polygons_hold_their_points() {
        let pair = points([(0.0, 0.0), (2.0, 0.0)]);
        let pair_polygon = Polygon::new(&pair);
        assert!(pair_polygon.contains_point(Vec2::new(1.0, 0.0)));
        assert!(!pair_polygon.contains_point(Vec2::new(1.0, 0.5)));
        assert!(Polygon::new(&pair[..1]).contains_point(Vec2::ZERO));
        assert!(!Polygon::new(&[]).contains_point(Vec2::ZERO));
    }

    // From inside, from outside, and in the notch of a concave shape.
    #[test]
    fn distance_to_boundary_from_either_side() {
        let shape = u_shape();
        let polygon = Polygon::new(&shape);
        let cases = [
            (Vec2::new(0.25, 2.0), 0.25),
            (Vec2::new(1.5, 0.25), 0.25),
            (Vec2::new(5.0, 2.0), 2.0),
            (Vec2::new(4.0, 4.0), SQRT_2),
            (Vec2::new(1.5, 2.0), 0.5),
            (Vec2::new(1.0, 1.0), 0.0),
        ];
        for (point, expected) in cases {
            let Some(distance) = polygon.distance_to_boundary(point) else {
                panic!("the polygon has a boundary");
            };
            assert!(close(distance, expected, 1e-6), "{point:?}: {distance} against {expected}");
        }
        assert_eq!(Polygon::new(&[]).distance_to_boundary(Vec2::ZERO), None);
    }
}
