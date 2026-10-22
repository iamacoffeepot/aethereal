use crate::Polyline;

impl Polyline<'_> {
    /// Returns true when two of the line's segments cross, by [`Segment::crosses`](crate::Segment::crosses).
    ///
    /// Segments that only touch do not cross, so neighbouring segments meeting at their shared
    /// point, a closing segment meeting the first, and a line that comes back to touch a point or
    /// segment it passed earlier do not cross themselves. A line that turns straight back over the
    /// segment it came along shares a stretch with it, and does. Every pair of segments is
    /// compared, so the cost grows with the square of the number of points.
    #[must_use]
    pub fn crosses_itself(self) -> bool {
        self.segments()
            .enumerate()
            .any(|(index, first)| self.segments().skip(index + 1).any(|second| first.crosses(second)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Polyline, Vec2};

    fn points<const COUNT: usize>(coordinates: [(f32, f32); COUNT]) -> [Vec2; COUNT] {
        coordinates.map(|(x, y)| Vec2::new(x, y))
    }

    // A bow tie crosses itself open or closed; a zig-zag does not.
    #[test]
    fn bow_tie_crosses_and_zig_zag_does_not() {
        let bow_tie = points([(0.0, 0.0), (2.0, 2.0), (2.0, 0.0), (0.0, 2.0)]);
        let zig_zag = points([(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)]);
        assert!(Polyline::open(&bow_tie).crosses_itself());
        assert!(Polyline::closed(&bow_tie).crosses_itself());
        assert!(!Polyline::open(&zig_zag).crosses_itself());
    }

    // Only the closing segment crosses, so only the closed line crosses itself.
    #[test]
    fn closing_segment_can_cross() {
        let shape = points([(0.0, 0.0), (2.0, 0.0), (0.0, 2.0), (2.0, 2.0)]);
        assert!(!Polyline::open(&shape).crosses_itself());
        assert!(Polyline::closed(&shape).crosses_itself());
    }

    // A square meets itself only at its corners, even written with its first point repeated last.
    #[test]
    fn closed_square_touches_but_does_not_cross() {
        let square = points([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
        let repeated = points([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]);
        assert!(!Polyline::closed(&square).crosses_itself());
        assert!(!Polyline::closed(&repeated).crosses_itself());
        assert!(!Polyline::open(&repeated).crosses_itself());
    }

    // Coming back to rest on an earlier segment is a touch.
    #[test]
    fn returning_to_touch_is_not_a_crossing() {
        let touching = points([(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (1.0, 0.0)]);
        let through = points([(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (1.0, -1.0)]);
        assert!(!Polyline::open(&touching).crosses_itself());
        assert!(Polyline::open(&through).crosses_itself());
    }

    // Turning straight back runs over the segment it came along.
    #[test]
    fn turning_back_over_itself_crosses() {
        let folded = points([(0.0, 0.0), (2.0, 0.0), (1.0, 0.0)]);
        assert!(Polyline::open(&folded).crosses_itself());
    }

    // Too few segments to cross.
    #[test]
    fn short_lines_do_not_cross() {
        let single = points([(0.0, 0.0), (2.0, 0.0)]);
        assert!(!Polyline::open(&single).crosses_itself());
        assert!(!Polyline::closed(&single).crosses_itself());
        assert!(!Polyline::open(&[]).crosses_itself());
    }
}
