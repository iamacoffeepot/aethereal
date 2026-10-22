mod bend;
mod crossing;
mod nearest;

pub use nearest::NearestSegment;

use crate::{Segment, Vec2};

/// A line through points in order, borrowed: segment `i` runs from point `i` to point `i + 1`, and a
/// closed polyline adds a segment from its last point back to its first.
///
/// A closed polyline of fewer than three points has the segments of an open one: a closing segment
/// would only retrace the one it closes. Repeated points are segments of zero length, kept in the
/// numbering.
#[derive(Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Polyline<'a> {
    /// The points the line passes through, in order.
    pub points: &'a [Vec2],
    /// Whether the line returns from its last point to its first.
    pub ends: Ends,
}

/// Whether a polyline returns from its last point to its first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ends {
    /// The line stops at its last point.
    Open,
    /// The line runs on from its last point back to its first.
    Closed,
}

/// The segments of a polyline in order, from [`Polyline::segments`].
#[derive(Debug, Clone)]
#[must_use]
pub struct Segments<'a> {
    polyline: Polyline<'a>,
    next: usize,
}

impl<'a> Polyline<'a> {
    /// The open polyline through `points`.
    #[inline]
    pub const fn open(points: &'a [Vec2]) -> Self {
        Self { points, ends: Ends::Open }
    }

    /// The closed polyline through `points`, running from the last back to the first.
    #[inline]
    pub const fn closed(points: &'a [Vec2]) -> Self {
        Self { points, ends: Ends::Closed }
    }

    /// Returns the number of segments: one fewer than the points when open, as many as the points
    /// when closed through three or more, and none for fewer than two points.
    #[inline]
    #[must_use]
    pub fn segment_count(self) -> usize {
        let count = self.points.len();
        let loops = self.loops();
        if loops {
            count
        } else {
            count.saturating_sub(1)
        }
    }

    /// Returns segment `index`, or `None` past the last segment.
    #[inline]
    #[must_use]
    pub fn segment(self, index: usize) -> Option<Segment> {
        let present = index < self.segment_count();
        if !present {
            return None;
        }
        let start = self.points[index];
        let end = self.points[(index + 1) % self.points.len()];
        Some(Segment::new(start, end))
    }

    /// Returns the segments in order.
    #[inline]
    pub fn segments(self) -> Segments<'a> {
        Segments { polyline: self, next: 0 }
    }

    /// Returns the sum of the segments' lengths.
    #[inline]
    #[must_use]
    pub fn length(self) -> f32 {
        self.segments().map(Segment::length).sum()
    }

    /// Returns true when the line has a closing segment.
    #[inline]
    fn loops(self) -> bool {
        let closed = self.ends == Ends::Closed;
        let enough = self.points.len() >= 3;
        closed && enough
    }
}

impl Iterator for Segments<'_> {
    type Item = Segment;

    #[inline]
    fn next(&mut self) -> Option<Segment> {
        let segment = self.polyline.segment(self.next)?;
        self.next += 1;
        Some(segment)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.polyline.segment_count().saturating_sub(self.next);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Segments<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIANGLE: [Vec2; 3] = [Vec2::new(0.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 4.0)];

    // Closing adds the segment back to the start, and only around three or more points.
    #[test]
    fn closing_adds_the_return_segment() {
        let open = Polyline::open(&TRIANGLE);
        let closed = Polyline::closed(&TRIANGLE);
        assert_eq!(open.segments().len(), 2);
        assert_eq!(closed.segments().len(), 3);
        assert_eq!(closed.segment(2), Some(Segment::new(TRIANGLE[2], TRIANGLE[0])));
        assert_eq!(open.segment(2), None);
        assert_eq!(open.length(), 7.0);
        assert_eq!(closed.length(), 12.0);
        assert_eq!(Polyline::closed(&TRIANGLE[..2]).segment_count(), 1);
        assert_eq!(Polyline::closed(&TRIANGLE[..1]).segment_count(), 0);
        assert_eq!(Polyline::open(&[]).segment_count(), 0);
    }
}
