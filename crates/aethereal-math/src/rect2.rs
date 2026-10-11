use aethereal_data::Plain;

use crate::Vec2;

/// An axis-aligned rectangle as two corners.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Rect2 {
    /// The smallest corner.
    pub min: Vec2,
    /// The largest corner.
    pub max: Vec2,
}

impl Rect2 {
    /// The empty rectangle, with `min` at positive infinity and `max` at negative infinity.
    pub const EMPTY: Self = Self { min: Vec2::splat(f32::INFINITY), max: Vec2::splat(f32::NEG_INFINITY) };

    /// Returns the rectangle from `origin` to `origin + size`. A negative size gives an empty
    /// rectangle.
    #[inline]
    pub fn from_origin_size(origin: Vec2, size: Vec2) -> Self {
        Self { min: origin, max: origin + size }
    }

    /// Returns the smallest rectangle holding every point.
    #[inline]
    pub fn from_points(points: &[Vec2]) -> Self {
        points.iter().fold(Self::EMPTY, |bound, point| bound.including(*point))
    }

    /// Returns true when `min` is above `max` on any axis.
    #[inline]
    #[must_use]
    pub fn is_empty(self) -> bool {
        let empty_x = self.min.x > self.max.x;
        let empty_y = self.min.y > self.max.y;
        empty_x || empty_y
    }

    /// Returns true when every bound is finite.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        let finite_min_x = self.min.x.is_finite();
        let finite_min_y = self.min.y.is_finite();
        let finite_max_x = self.max.x.is_finite();
        let finite_max_y = self.max.y.is_finite();
        finite_min_x && finite_min_y && finite_max_x && finite_max_y
    }

    /// Returns `max - min`.
    #[inline]
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    /// Returns the width of `self`.
    #[inline]
    #[must_use]
    pub fn width(self) -> f32 {
        self.max.x - self.min.x
    }

    /// Returns the height of `self`.
    #[inline]
    #[must_use]
    pub fn height(self) -> f32 {
        self.max.y - self.min.y
    }

    /// Returns the centre of `self`.
    #[inline]
    pub fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Returns the four corners in order: (`min`,`min`) (`max`,`min`) (`min`,`max`) (`max`,`max`).
    #[inline]
    pub fn corners(self) -> [Vec2; 4] {
        [self.min, Vec2::new(self.max.x, self.min.y), Vec2::new(self.min.x, self.max.y), self.max]
    }

    /// Returns true when `point` lies inside `self`, boundary included.
    #[inline]
    #[must_use]
    pub fn contains_point(self, point: Vec2) -> bool {
        let empty = self.is_empty();
        if empty {
            return false;
        }
        let above_low_x = point.x >= self.min.x;
        let below_high_x = point.x <= self.max.x;
        let inside_x = above_low_x && below_high_x;
        let above_low_y = point.y >= self.min.y;
        let below_high_y = point.y <= self.max.y;
        let inside_y = above_low_y && below_high_y;
        inside_x && inside_y
    }

    /// Returns the smallest rectangle holding `self` and `point`.
    #[inline]
    pub fn including(self, point: Vec2) -> Self {
        Self { min: self.min.min(point), max: self.max.max(point) }
    }

    /// Returns the smallest rectangle holding `self` and `other`.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// Returns the rectangle shared by `self` and `other`.
    #[inline]
    pub fn intersection(self, other: Self) -> Self {
        Self { min: self.min.max(other.min), max: self.max.min(other.max) }
    }

    /// Returns true when `self` and `other` share a point.
    #[inline]
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        let shared = self.intersection(other);
        let empty = shared.is_empty();
        !empty
    }

    /// Returns `self` moved by `offset`.
    #[inline]
    pub fn translate(self, offset: Vec2) -> Self {
        let empty = self.is_empty();
        if empty {
            self
        } else {
            Self { min: self.min + offset, max: self.max + offset }
        }
    }

    /// Returns `self` with every edge pulled inward; past the centre the result is empty.
    #[inline]
    pub fn inset(self, amount: Vec2) -> Self {
        let empty = self.is_empty();
        if empty {
            return self;
        }
        Self { min: self.min + amount, max: self.max - amount }
    }

    /// Returns what `other` does not cover, as four parts; a part with no area is `EMPTY`.
    #[inline]
    pub fn subtract(self, other: Self) -> [Self; 4] {
        let cut = self.intersection(other);
        let missing = cut.is_empty();
        if missing {
            return [self, Self::EMPTY, Self::EMPTY, Self::EMPTY];
        }
        [
            Self::band(self.min, Vec2::new(self.max.x, cut.min.y)),
            Self::band(Vec2::new(self.min.x, cut.max.y), self.max),
            Self::band(Vec2::new(self.min.x, cut.min.y), Vec2::new(cut.min.x, cut.max.y)),
            Self::band(Vec2::new(cut.max.x, cut.min.y), Vec2::new(self.max.x, cut.max.y)),
        ]
    }

    /// Returns one `subtract` part, collapsing a part with no area to `EMPTY`.
    #[inline]
    fn band(low: Vec2, high: Vec2) -> Self {
        let empty_x = low.x >= high.x;
        let empty_y = low.y >= high.y;
        let empty = empty_x || empty_y;
        if empty {
            Self::EMPTY
        } else {
            Self { min: low, max: high }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(low_x: f32, low_y: f32, high_x: f32, high_y: f32) -> Rect2 {
        Rect2 { min: Vec2::new(low_x, low_y), max: Vec2::new(high_x, high_y) }
    }

    // The empty rectangle is the identity for union.
    #[test]
    fn empty_unions_as_identity() {
        let plain = rect(0.0, 0.0, 1.0, 1.0);
        assert_eq!(Rect2::EMPTY.union(plain), plain);
        assert_eq!(plain.union(Rect2::EMPTY), plain);
    }

    // Flat is a value, not an absence: it still holds its edge.
    #[test]
    fn flat_rect_holds_its_edge() {
        let flat = rect(0.0, 0.0, 1.0, 0.0);
        assert!(!flat.is_empty());
        assert!(flat.contains_point(Vec2::new(0.5, 0.0)));
    }

    // Touching is sharing; apart shares nothing.
    #[test]
    fn touching_intersects_apart_does_not() {
        let left = rect(0.0, 0.0, 1.0, 1.0);
        let touching = rect(1.0, 0.0, 2.0, 1.0);
        let apart = rect(1.5, 0.0, 2.5, 1.0);
        assert!(left.intersects(touching));
        assert!(!left.intersects(apart));
        assert!(left.intersection(apart).is_empty());
    }

    // Empty holds nothing and meets nothing.
    #[test]
    fn empty_meets_and_holds_nothing() {
        let plain = rect(0.0, 0.0, 1.0, 1.0);
        assert!(!Rect2::EMPTY.intersects(plain));
        assert!(!plain.intersects(Rect2::EMPTY));
        assert!(!Rect2::EMPTY.contains_point(Vec2::ZERO));
    }

    // A backwards size gets nothing, not something plausible.
    #[test]
    fn negative_size_is_empty() {
        let origin = Vec2::new(4.0, 4.0);
        let size = Vec2::new(-1.0, 2.0);
        assert!(Rect2::from_origin_size(origin, size).is_empty());
    }

    // Padding past the centre crosses the bounds, which means empty.
    #[test]
    fn inset_past_centre_is_empty() {
        let plain = rect(0.0, 0.0, 4.0, 10.0);
        assert!(plain.inset(Vec2::splat(3.0)).is_empty());
    }

    // A hole leaves four bands that tile the remainder.
    #[test]
    fn subtract_hole_leaves_four_bands() {
        let outer = rect(0.0, 0.0, 10.0, 10.0);
        let hole = rect(4.0, 4.0, 6.0, 6.0);
        let parts = outer.subtract(hole);
        assert_eq!(parts[0], rect(0.0, 0.0, 10.0, 4.0));
        assert_eq!(parts[1], rect(0.0, 6.0, 10.0, 10.0));
        assert_eq!(parts[2], rect(0.0, 4.0, 4.0, 6.0));
        assert_eq!(parts[3], rect(6.0, 4.0, 10.0, 6.0));
        let pairs = [
            (parts[0], parts[1]),
            (parts[0], parts[2]),
            (parts[0], parts[3]),
            (parts[1], parts[2]),
            (parts[1], parts[3]),
            (parts[2], parts[3]),
        ];
        for (index, pair) in pairs.iter().enumerate() {
            let shared = pair.0.intersection(pair.1);
            let thin = shared.width() <= 0.0 || shared.height() <= 0.0;
            assert!(thin, "pair {index} overlaps with area");
        }
        let area = parts[0].width() * parts[0].height()
            + parts[1].width() * parts[1].height()
            + parts[2].width() * parts[2].height()
            + parts[3].width() * parts[3].height();
        let outer_area = outer.width() * outer.height();
        let hole_area = hole.width() * hole.height();
        let tiles = area == outer_area - hole_area;
        assert!(tiles);
    }

    // No overlap means nothing to remove.
    #[test]
    fn subtract_disjoint_keeps_self() {
        let plain = rect(0.0, 0.0, 10.0, 10.0);
        let parts = plain.subtract(rect(20.0, 20.0, 30.0, 30.0));
        assert_eq!(parts[0], plain);
        assert!(parts[1].is_empty());
        assert!(parts[2].is_empty());
        assert!(parts[3].is_empty());
    }

    // Covering everything leaves nothing.
    #[test]
    fn subtract_covering_leaves_nothing() {
        let parts = rect(0.0, 0.0, 10.0, 10.0).subtract(rect(-1.0, -1.0, 11.0, 11.0));
        assert!(parts[0].is_empty());
        assert!(parts[1].is_empty());
        assert!(parts[2].is_empty());
        assert!(parts[3].is_empty());
    }

    // Covering the left half leaves only the right band.
    #[test]
    fn subtract_side_cut_leaves_one_band() {
        let parts = rect(0.0, 0.0, 10.0, 10.0).subtract(rect(-5.0, -5.0, 5.0, 15.0));
        assert!(parts[0].is_empty());
        assert!(parts[1].is_empty());
        assert!(parts[2].is_empty());
        assert_eq!(parts[3], rect(5.0, 0.0, 10.0, 10.0));
    }
}
