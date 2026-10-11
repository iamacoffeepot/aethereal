use aethereal_data::Plain;

use crate::Vec3;

/// An axis-aligned box as two corners.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Aabb {
    /// The smallest corner.
    pub min: Vec3,
    /// The largest corner.
    pub max: Vec3,
}

impl Aabb {
    /// The empty box, with `min` at positive infinity and `max` at negative infinity.
    pub const EMPTY: Self = Self { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) };

    /// Returns the box centred on the origin with the given half-extents. A negative half-extent
    /// is read as its absolute value.
    #[inline]
    pub fn from_half_extents(half_extents: Vec3) -> Self {
        let half_x = half_extents.x.abs();
        let half_y = half_extents.y.abs();
        let half_z = half_extents.z.abs();
        Self { min: Vec3::new(-half_x, -half_y, -half_z), max: Vec3::new(half_x, half_y, half_z) }
    }

    /// Returns the smallest box holding every point. `EMPTY` for no points.
    #[inline]
    pub fn from_points(points: &[Vec3]) -> Self {
        points.iter().fold(Self::EMPTY, |bound, point| bound.including(*point))
    }

    /// Returns true when `min` is above `max` on any axis.
    #[inline]
    #[must_use]
    pub fn is_empty(self) -> bool {
        let empty_x = self.min.x > self.max.x;
        let empty_y = self.min.y > self.max.y;
        let empty_z = self.min.z > self.max.z;
        empty_x || empty_y || empty_z
    }

    /// Returns the centre of `self`.
    #[inline]
    pub fn center(self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Returns `max - min`.
    #[inline]
    pub fn size(self) -> Vec3 {
        self.max - self.min
    }

    /// Returns the eight corners in order: (`min`,`min`,`min`) (`max`,`min`,`min`)
    /// (`min`,`max`,`min`) (`max`,`max`,`min`) (`min`,`min`,`max`) (`max`,`min`,`max`)
    /// (`min`,`max`,`max`) (`max`,`max`,`max`).
    #[inline]
    pub fn corners(self) -> [Vec3; 8] {
        let low = self.min;
        let high = self.max;
        [
            low,
            Vec3::new(high.x, low.y, low.z),
            Vec3::new(low.x, high.y, low.z),
            Vec3::new(high.x, high.y, low.z),
            Vec3::new(low.x, low.y, high.z),
            Vec3::new(high.x, low.y, high.z),
            Vec3::new(low.x, high.y, high.z),
            high,
        ]
    }

    /// Returns true when `point` lies inside `self`, boundary included.
    #[inline]
    #[must_use]
    pub fn contains_point(self, point: Vec3) -> bool {
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
        let above_low_z = point.z >= self.min.z;
        let below_high_z = point.z <= self.max.z;
        let inside_z = above_low_z && below_high_z;
        inside_x && inside_y && inside_z
    }

    /// Returns the smallest box holding `self` and `point`. `EMPTY.including(p)` is the box of `p` alone.
    #[inline]
    pub fn including(self, point: Vec3) -> Self {
        Self { min: self.min.min(point), max: self.max.max(point) }
    }

    /// Returns the smallest box holding `self` and `other`.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    /// Returns the box shared by `self` and `other`. Empty when the two do not overlap.
    #[inline]
    pub fn intersection(self, other: Self) -> Self {
        Self { min: self.min.max(other.min), max: self.max.min(other.max) }
    }

    /// Returns true when `self` and `other` share a point. Touching counts.
    #[inline]
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        let shared = self.intersection(other);
        let empty = shared.is_empty();
        !empty
    }

    /// Returns `self` moved by `offset`.
    #[inline]
    pub fn translate(self, offset: Vec3) -> Self {
        let empty = self.is_empty();
        if empty {
            self
        } else {
            Self { min: self.min + offset, max: self.max + offset }
        }
    }

    /// Returns `self` scaled by `factor`. A negative factor swaps that axis's bounds so `min <= max`
    /// still holds.
    #[inline]
    pub fn scale(self, factor: Vec3) -> Self {
        let empty = self.is_empty();
        if empty {
            return self;
        }
        let low = Vec3::new(self.min.x * factor.x, self.min.y * factor.y, self.min.z * factor.z);
        let high = Vec3::new(self.max.x * factor.x, self.max.y * factor.y, self.max.z * factor.z);
        Self { min: low.min(high), max: low.max(high) }
    }

    /// Returns the box of the eight corners after turning them about `axis`; a zero axis is read
    /// as `Vec3::Y`.
    #[inline]
    pub fn rotate(self, axis: Vec3, angle: f32) -> Self {
        let empty = self.is_empty();
        if empty {
            return self;
        }
        let unit = axis.normalize_or(Vec3::Y);
        self.corners().iter().fold(Self::EMPTY, |bound, corner| bound.including(corner.rotate_about(unit, angle)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close;
    use core::f32::consts::FRAC_PI_2;

    // The empty box is the identity for union.
    #[test]
    fn empty_unions_as_identity() {
        let plain = Aabb::from_half_extents(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(Aabb::EMPTY.union(plain), plain);
        assert_eq!(plain.union(Aabb::EMPTY), plain);
    }

    // No points means no bound; three points pin the bound.
    #[test]
    fn from_points_collects_extremes() {
        assert!(Aabb::from_points(&[]).is_empty());
        let points = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(-1.0, 2.0, -3.0), Vec3::new(0.5, -1.0, 5.0)];
        let bound = Aabb::from_points(&points);
        assert_eq!(bound.min, Vec3::new(-1.0, -1.0, -3.0));
        assert_eq!(bound.max, Vec3::new(1.0, 2.0, 5.0));
    }

    // Flat is a value, not an absence: it still holds its plane.
    #[test]
    fn flat_box_holds_its_plane() {
        let flat = Aabb { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(1.0, 0.0, 1.0) };
        assert!(!flat.is_empty());
        assert!(flat.contains_point(Vec3::new(0.5, 0.0, 0.5)));
    }

    // Touching is sharing; apart shares nothing.
    #[test]
    fn touching_intersects_apart_does_not() {
        let left = Aabb { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) };
        let touching = Aabb { min: Vec3::new(1.0, 0.0, 0.0), max: Vec3::new(2.0, 1.0, 1.0) };
        let apart = Aabb { min: Vec3::new(1.5, 0.0, 0.0), max: Vec3::new(2.5, 1.0, 1.0) };
        assert!(left.intersects(touching));
        assert!(!left.intersects(apart));
        assert!(left.intersection(apart).is_empty());
    }

    // Empty holds nothing and meets nothing.
    #[test]
    fn empty_meets_and_holds_nothing() {
        let plain = Aabb::from_half_extents(Vec3::new(1.0, 1.0, 1.0));
        assert!(!Aabb::EMPTY.intersects(plain));
        assert!(!plain.intersects(Aabb::EMPTY));
        assert!(!Aabb::EMPTY.contains_point(Vec3::ZERO));
    }

    // A negative factor mirrors instead of inverting the box.
    #[test]
    fn scale_by_negative_keeps_min_below_max() {
        let plain = Aabb { min: Vec3::new(1.0, 2.0, 3.0), max: Vec3::new(4.0, 5.0, 6.0) };
        let scaled = plain.scale(Vec3::new(-1.0, 1.0, 1.0));
        assert_eq!(scaled.min, Vec3::new(-4.0, 2.0, 3.0));
        assert_eq!(scaled.max, Vec3::new(-1.0, 5.0, 6.0));
    }

    // A quarter turn about `Y` swaps an off-centre box's `x` and `z` extents.
    #[test]
    fn rotate_off_centre_box_about_y() {
        let plain = Aabb { min: Vec3::new(1.0, -0.5, -0.5), max: Vec3::new(2.0, 0.5, 0.5) };
        let turned = plain.rotate(Vec3::Y, FRAC_PI_2);
        let close_min_x = close(turned.min.x, -0.5, 1e-5);
        let close_max_x = close(turned.max.x, 0.5, 1e-5);
        let close_min_z = close(turned.min.z, -2.0, 1e-5);
        let close_max_z = close(turned.max.z, -1.0, 1e-5);
        assert!(close_min_x);
        assert!(close_max_x);
        assert!(close_min_z);
        assert!(close_max_z);
    }

    // Infinity plus a finite offset stays infinite, never NaN.
    #[test]
    fn translate_of_empty_stays_empty() {
        let moved = Aabb::EMPTY.translate(Vec3::new(1.0, 2.0, 3.0));
        assert!(moved.is_empty());
    }
}
