use aethereal_data::Plain;

use crate::Vec2;

/// The straight piece of a line from `start` to `end`.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Segment {
    /// Where the segment starts.
    pub start: Vec2,
    /// Where the segment ends. Equal to `start` for a segment of zero length.
    pub end: Vec2,
}

/// The side of a segment's line a point lies on, by the sign of
/// `segment.direction().perp_dot(point - segment.start)`.
///
/// `Positive` is a quarter turn counter-clockwise from the direction the segment runs: its left
/// when the second axis points up from the first, as `y` does in a `y`-up plane. The crate fixes no
/// direction for the second axis of a plane, so the names say the sign, not left or right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// A quarter turn counter-clockwise from the direction the segment runs.
    Positive,
    /// A quarter turn clockwise from the direction the segment runs.
    Negative,
    /// On the segment's line, or anywhere for a segment of zero length, which has no line.
    On,
}

impl Segment {
    /// Builds the segment from `start` to `end`.
    #[inline]
    pub const fn new(start: Vec2, end: Vec2) -> Self {
        Self { start, end }
    }

    /// Returns `end - start`.
    #[inline]
    pub fn direction(self) -> Vec2 {
        self.end - self.start
    }

    /// Returns the length of `self`.
    #[inline]
    #[must_use]
    pub fn length(self) -> f32 {
        self.direction().length()
    }

    /// Returns how far along the segment's whole line, from `start` toward `end`, the foot of
    /// `point` lies: the foot is not clipped to the segment, so the answer is negative before
    /// `start` and above `length()` past `end`. A segment of zero length has no line, and answers 0.
    #[inline]
    #[must_use]
    pub fn along(self, point: Vec2) -> f32 {
        let length = self.length();
        let nonzero = length > 0.0;
        if nonzero {
            self.direction().dot(point - self.start) / length
        } else {
            0.0
        }
    }

    /// Returns the point of `self` nearest `point`. A segment of zero length answers `start`.
    #[inline]
    pub fn closest_point(self, point: Vec2) -> Vec2 {
        let direction = self.direction();
        let span = direction.length_squared();
        let nonzero = span > 0.0;
        if nonzero {
            let fraction = (direction.dot(point - self.start) / span).clamp(0.0, 1.0);
            self.start + direction * fraction
        } else {
            self.start
        }
    }

    /// Returns the distance from `point` to the nearest point of `self`.
    #[inline]
    #[must_use]
    pub fn distance_to(self, point: Vec2) -> f32 {
        (point - self.closest_point(point)).length()
    }

    /// Returns true when `point` lies exactly on `self`, either end included. The test has no
    /// tolerance: a point placed on the segment by arithmetic may land a rounding off it.
    #[inline]
    #[must_use]
    pub fn contains_point(self, point: Vec2) -> bool {
        let on_line = self.side(point) == Side::On;
        let low = self.start.min(self.end);
        let high = self.start.max(self.end);
        let above_low = point.x >= low.x && point.y >= low.y;
        let below_high = point.x <= high.x && point.y <= high.y;
        on_line && above_low && below_high
    }

    /// Returns the side of the segment's line `point` lies on. A segment of zero length answers
    /// `On` for every point.
    #[inline]
    #[must_use]
    pub fn side(self, point: Vec2) -> Side {
        let turn = self.direction().perp_dot(point - self.start);
        let positive = turn > 0.0;
        let negative = turn < 0.0;
        if positive {
            Side::Positive
        } else if negative {
            Side::Negative
        } else {
            Side::On
        }
    }

    /// Returns true when `self` and `other` cross.
    ///
    /// Two segments cross when each passes strictly from one side of the other's line to the
    /// other side, or when they lie on one line and share a stretch of positive length. Segments
    /// that only touch do not cross: a shared end, or an end of one lying on the other, is a touch.
    /// A segment of zero length is a point and crosses nothing. The sides are compared exactly,
    /// with no tolerance, so an end placed on the other segment by arithmetic may land a rounding
    /// off it and count either way.
    #[inline]
    #[must_use]
    pub fn crosses(self, other: Self) -> bool {
        let own = self.direction();
        let theirs = other.direction();
        let their_start_turn = own.perp_dot(other.start - self.start);
        let their_end_turn = own.perp_dot(other.end - self.start);
        let own_start_turn = theirs.perp_dot(self.start - other.start);
        let own_end_turn = theirs.perp_dot(self.end - other.start);
        let they_straddle = opposite(their_start_turn, their_end_turn);
        let own_straddle = opposite(own_start_turn, own_end_turn);
        if they_straddle && own_straddle {
            return true;
        }
        let their_start_on = their_start_turn == 0.0;
        let their_end_on = their_end_turn == 0.0;
        let has_line = own.length_squared() > 0.0;
        let collinear = their_start_on && their_end_on && has_line;
        collinear && self.overlaps_on_line(other)
    }

    /// Returns true when `other`, lying on the line of `self`, shares a stretch of positive length
    /// with `self`. `self` has positive length.
    #[inline]
    fn overlaps_on_line(self, other: Self) -> bool {
        let own = self.direction();
        let span = own.length_squared();
        let first = own.dot(other.start - self.start) / span;
        let second = own.dot(other.end - self.start) / span;
        let low = first.min(second).max(0.0);
        let high = first.max(second).min(1.0);
        high > low
    }
}

/// Returns true when one of `first` and `second` is above zero and the other below.
#[inline]
fn opposite(first: f32, second: f32) -> bool {
    let rising = first < 0.0 && second > 0.0;
    let falling = first > 0.0 && second < 0.0;
    rising || falling
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close;
    use core::f32::consts::SQRT_2;

    fn segment(start_x: f32, start_y: f32, end_x: f32, end_y: f32) -> Segment {
        Segment::new(Vec2::new(start_x, start_y), Vec2::new(end_x, end_y))
    }

    // Two diagonals of a square cross in its middle, whichever way each runs.
    #[test]
    fn diagonals_cross_either_way_round() {
        let rising = segment(0.0, 0.0, 2.0, 2.0);
        let falling = segment(0.0, 2.0, 2.0, 0.0);
        assert!(rising.crosses(falling));
        assert!(falling.crosses(rising));
        assert!(rising.crosses(segment(2.0, 0.0, 0.0, 2.0)));
    }

    // Segments that would cross only if one were longer do not cross.
    #[test]
    fn segments_short_of_each_other_do_not_cross() {
        let rising = segment(0.0, 0.0, 2.0, 2.0);
        let short = segment(0.0, 2.0, 0.9, 1.1);
        assert!(!rising.crosses(short));
    }

    // A shared end, and an end resting on the other segment, are touches.
    #[test]
    fn touching_ends_do_not_cross() {
        let base = segment(0.0, 0.0, 2.0, 0.0);
        let from_shared_end = segment(2.0, 0.0, 3.0, 1.0);
        let standing_on_middle = segment(1.0, 0.0, 1.0, 1.0);
        let through_middle = segment(1.0, -1.0, 1.0, 1.0);
        assert!(!base.crosses(from_shared_end));
        assert!(!base.crosses(standing_on_middle));
        assert!(!standing_on_middle.crosses(base));
        assert!(base.crosses(through_middle));
    }

    // On one line, a shared stretch crosses and a shared end or a gap does not.
    #[test]
    fn collinear_segments_cross_only_on_a_shared_stretch() {
        let base = segment(0.0, 0.0, 2.0, 0.0);
        assert!(base.crosses(segment(1.0, 0.0, 3.0, 0.0)));
        assert!(base.crosses(segment(3.0, 0.0, 1.0, 0.0)));
        assert!(!base.crosses(segment(2.0, 0.0, 3.0, 0.0)));
        assert!(!base.crosses(segment(2.5, 0.0, 3.0, 0.0)));
    }

    // A point has no line to cross, even lying inside the other segment.
    #[test]
    fn zero_length_segment_crosses_nothing() {
        let base = segment(0.0, 0.0, 2.0, 0.0);
        let point = segment(1.0, 0.0, 1.0, 0.0);
        assert!(!base.crosses(point));
        assert!(!point.crosses(base));
    }

    // The foot runs past either end along the whole line, while the distance is to the segment.
    #[test]
    fn along_is_unclipped_and_distance_is_clipped() {
        let base = segment(1.0, 1.0, 3.0, 1.0);
        let before = Vec2::new(0.0, 2.0);
        let before_along = close(base.along(before), -1.0, 1e-6);
        let before_distance = close(base.distance_to(before), SQRT_2, 1e-6);
        let past_along = close(base.along(Vec2::new(5.0, 1.0)), 4.0, 1e-6);
        let above_distance = close(base.distance_to(Vec2::new(2.0, 4.0)), 3.0, 1e-6);
        assert!(before_along);
        assert!(before_distance);
        assert!(past_along);
        assert!(above_distance);
    }

    // The side follows the turn from the direction of travel, and flips with it.
    #[test]
    fn side_follows_direction() {
        let eastward = segment(0.0, 0.0, 1.0, 0.0);
        let westward = segment(1.0, 0.0, 0.0, 0.0);
        let north = Vec2::new(0.5, 1.0);
        assert_eq!(eastward.side(north), Side::Positive);
        assert_eq!(westward.side(north), Side::Negative);
        assert_eq!(eastward.side(Vec2::new(7.0, 0.0)), Side::On);
    }

    // On the line but past an end is off the segment; both ends are on it.
    #[test]
    fn contains_point_stops_at_the_ends() {
        let diagonal = segment(0.0, 2.0, 2.0, 0.0);
        assert!(diagonal.contains_point(Vec2::new(1.0, 1.0)));
        assert!(diagonal.contains_point(Vec2::new(0.0, 2.0)));
        assert!(diagonal.contains_point(Vec2::new(2.0, 0.0)));
        assert!(!diagonal.contains_point(Vec2::new(3.0, -1.0)));
        assert!(!diagonal.contains_point(Vec2::new(1.0, 1.5)));
    }

    // A point segment answers its one point, with no NaN.
    #[test]
    fn zero_length_segment_answers_its_point() {
        let point = segment(1.0, 1.0, 1.0, 1.0);
        let away = Vec2::new(4.0, 5.0);
        assert_eq!(point.along(away), 0.0);
        assert_eq!(point.closest_point(away), Vec2::new(1.0, 1.0));
        assert_eq!(point.side(away), Side::On);
        let distance = close(point.distance_to(away), 5.0, 1e-6);
        assert!(distance);
    }
}
