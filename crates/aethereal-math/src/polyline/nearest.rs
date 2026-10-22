use crate::{Polyline, Segment, Side, Vec2};

/// A point's nearest segment of a polyline, from [`Polyline::nearest_segment`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearestSegment {
    /// The segment's number in [`Polyline::segments`].
    pub index: usize,
    /// The distance from the point to the nearest point of the segment.
    pub distance: f32,
    /// The length along the polyline, from its first point, at the point's foot on the segment's
    /// whole line. The foot is not clipped to the segment, so collinear segments agree on it, and
    /// it runs below zero before the first point and past the line's length after the last.
    pub along: f32,
    /// The side of the segment's line the point lies on.
    pub side: Side,
}

impl NearestSegment {
    /// Measures `point` against `segment`, numbered `index`, which starts `before` along the line.
    fn measure(index: usize, before: f32, segment: Segment, point: Vec2) -> Self {
        Self {
            index,
            distance: segment.distance_to(point),
            along: before + segment.along(point),
            side: segment.side(point),
        }
    }
}

impl Polyline<'_> {
    /// Returns the segment nearest `point`, with the point's distance, length along, and side.
    ///
    /// Where segments are equally near, the first of them answers. A segment of zero length is
    /// measured as its one point: its distance is to that point, its foot is its start, and its
    /// side is [`Side::On`]. `None` when the line has no segment, fewer than two points.
    #[must_use]
    pub fn nearest_segment(self, point: Vec2) -> Option<NearestSegment> {
        let mut segments = self.segments();
        let first = segments.next()?;
        let start = NearestSegment::measure(0, 0.0, first, point);
        let (_, nearest) = segments.enumerate().fold((first.length(), start), |(before, kept), (offset, segment)| {
            let candidate = NearestSegment::measure(offset + 1, before, segment, point);
            let closer = candidate.distance < kept.distance;
            let chosen = if closer {
                candidate
            } else {
                kept
            };
            (before + segment.length(), chosen)
        });
        Some(nearest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close;

    fn points<const COUNT: usize>(coordinates: [(f32, f32); COUNT]) -> [Vec2; COUNT] {
        coordinates.map(|(x, y)| Vec2::new(x, y))
    }

    fn nearest(polyline: Polyline<'_>, point: Vec2) -> NearestSegment {
        let Some(nearest) = polyline.nearest_segment(point) else {
            panic!("the polyline has a segment");
        };
        nearest
    }

    // Before the start and past the end, the foot runs on along the line, unclipped, while the
    // distance is to the nearest end.
    #[test]
    fn foot_runs_past_the_ends() {
        let line = points([(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
        let corner_distance = libm::sqrtf(1.25);
        let before = nearest(Polyline::open(&line), Vec2::new(-0.5, 1.0));
        let after = nearest(Polyline::open(&line), Vec2::new(2.5, -1.0));
        let before_along = close(before.along, -0.5, 1e-6);
        let before_distance = close(before.distance, corner_distance, 1e-6);
        let after_along = close(after.along, 2.5, 1e-6);
        let after_distance = close(after.distance, corner_distance, 1e-6);
        assert_eq!((before.index, before.side), (0, Side::Positive));
        assert_eq!((after.index, after.side), (1, Side::Negative));
        assert!(before_along);
        assert!(before_distance);
        assert!(after_along);
        assert!(after_distance);
    }

    // Either side of the point two collinear segments share, the length along runs on smoothly.
    #[test]
    fn collinear_segments_agree_on_the_foot() {
        let line = points([(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
        let left = nearest(Polyline::open(&line), Vec2::new(0.999, 1.0));
        let right = nearest(Polyline::open(&line), Vec2::new(1.001, 1.0));
        let above = nearest(Polyline::open(&line), Vec2::new(1.0, 1.0));
        let left_along = close(left.along, 0.999, 1e-6);
        let right_along = close(right.along, 1.001, 1e-6);
        let above_along = close(above.along, 1.0, 1e-6);
        assert_eq!((left.index, right.index, above.index), (0, 1, 0));
        assert!(left_along);
        assert!(right_along);
        assert!(above_along);
    }

    // The length along adds the segments before the nearest one.
    #[test]
    fn along_counts_earlier_segments() {
        let corner = points([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        let found = nearest(Polyline::open(&corner), Vec2::new(2.0, 0.5));
        let distance = close(found.distance, 1.0, 1e-6);
        let along = close(found.along, 1.5, 1e-6);
        assert_eq!((found.index, found.side), (1, Side::Negative));
        assert!(distance);
        assert!(along);
    }

    // A closed line's closing segment can be the nearest; open, the same points tie at a corner.
    #[test]
    fn closing_segment_can_be_nearest() {
        let square = points([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
        let point = Vec2::new(-1.0, 0.5);
        let closed = nearest(Polyline::closed(&square), point);
        let open = nearest(Polyline::open(&square), point);
        let closed_distance = close(closed.distance, 1.0, 1e-6);
        let closed_along = close(closed.along, 3.5, 1e-6);
        let open_distance = close(open.distance, libm::sqrtf(1.25), 1e-6);
        assert_eq!((closed.index, closed.side), (3, Side::Negative));
        assert_eq!(open.index, 0);
        assert!(closed_distance);
        assert!(closed_along);
        assert!(open_distance);
    }

    // A repeated point is a segment of zero length, measured as its point, with no NaN.
    #[test]
    fn zero_length_segment_is_its_point() {
        let repeated = points([(1.0, 1.0), (1.0, 1.0)]);
        let found = nearest(Polyline::open(&repeated), Vec2::new(4.0, 5.0));
        let distance = close(found.distance, 5.0, 1e-6);
        assert_eq!((found.index, found.along, found.side), (0, 0.0, Side::On));
        assert!(distance);
    }

    // With no segment there is nothing to be near.
    #[test]
    fn no_segment_is_none() {
        let single = [Vec2::new(1.0, 1.0)];
        assert_eq!(Polyline::open(&single).nearest_segment(Vec2::ZERO), None);
        assert_eq!(Polyline::closed(&[]).nearest_segment(Vec2::ZERO), None);
    }
}
