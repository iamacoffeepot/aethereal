use super::Vec4;
use aethereal_data::Plain;

/// A vector with three `f32` components.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Vec3 {
    /// The first component.
    pub x: f32,
    /// The second component.
    pub y: f32,
    /// The third component.
    pub z: f32,
}

impl Vec3 {
    /// The vector with all components zero.
    pub const ZERO: Self = Self::splat(0.0);
    /// The vector with all components one.
    pub const ONE: Self = Self::splat(1.0);
    /// The unit vector along `x`.
    pub const X: Self = Self::new(1.0, 0.0, 0.0);
    /// The unit vector along `y`.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);
    /// The unit vector along `z`.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Builds a vector from its components.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Builds a vector with every component set to `value`.
    #[inline]
    pub const fn splat(value: f32) -> Self {
        Self { x: value, y: value, z: value }
    }

    /// Builds a vector from its components in `x`, `y`, `z` order.
    #[inline]
    pub const fn from_array(components: [f32; 3]) -> Self {
        Self::new(components[0], components[1], components[2])
    }

    /// Returns the components in `x`, `y`, `z` order.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Returns the dot product of `self` and `other`.
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Returns the squared length of `self`.
    #[inline]
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Returns the length of `self`.
    #[inline]
    #[must_use]
    pub fn length(self) -> f32 {
        libm::sqrtf(self.length_squared())
    }

    /// Returns `self` with length one. Zero length gives `ZERO`.
    #[inline]
    pub fn normalize(self) -> Self {
        let length = self.length();
        let nonzero = length > 0.0;
        if nonzero {
            let reciprocal = 1.0 / length;
            self * reciprocal
        } else {
            Self::ZERO
        }
    }

    /// Returns `self` with length one, or `fallback` when `length_squared()` is below `1e-12`.
    #[inline]
    pub fn normalize_or(self, fallback: Self) -> Self {
        let squared = self.length_squared();
        let negligible = squared < 1e-12;
        if negligible {
            fallback
        } else {
            let reciprocal = 1.0 / libm::sqrtf(squared);
            self * reciprocal
        }
    }

    /// Right-handed: `X.cross(Y) == Z`.
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Turns `self` about the unit vector `axis` by `angle` radians (Rodrigues' formula).
    #[inline]
    pub fn rotate_about(self, axis: Self, angle: f32) -> Self {
        let cosine = libm::cosf(angle);
        let sine = libm::sinf(angle);
        let along = axis.dot(self);
        let crossed = axis.cross(self);
        let weight = 1.0 - cosine;
        Self::new(
            self.x * cosine + crossed.x * sine + axis.x * along * weight,
            self.y * cosine + crossed.y * sine + axis.y * along * weight,
            self.z * cosine + crossed.z * sine + axis.z * along * weight,
        )
    }

    /// Returns the linear interpolation between `self` and `other` at `amount`.
    #[inline]
    pub fn lerp(self, other: Self, amount: f32) -> Self {
        self + (other - self) * amount
    }

    /// The smaller of each component.
    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y), self.z.min(other.z))
    }

    /// The larger of each component.
    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y), self.z.max(other.z))
    }

    /// Builds a `Vec4` from `self` and the `w` component.
    #[inline]
    pub const fn extend(self, w: f32) -> Vec4 {
        Vec4::new(self.x, self.y, self.z, w)
    }
}

impl_vec_ops!(Vec3 { x, y, z });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::{close, close_vec3};
    use core::f32::consts::FRAC_PI_2;

    // Catches a field dropped from a macro invocation.
    #[test]
    fn operators_act_on_every_component() {
        let left = Vec3::new(1.0, 2.0, 3.0);
        let right = Vec3::new(4.0, 8.0, 12.0);
        assert_eq!(left + right, Vec3::new(5.0, 10.0, 15.0));
        assert_eq!(right - left, Vec3::new(3.0, 6.0, 9.0));
        assert_eq!(-left, Vec3::new(-1.0, -2.0, -3.0));
        assert_eq!(left * 2.0, Vec3::new(2.0, 4.0, 6.0));
        assert_eq!(right / 2.0, Vec3::new(2.0, 4.0, 6.0));
        let mut sum = left;
        sum += right;
        assert_eq!(sum, Vec3::new(5.0, 10.0, 15.0));
        let mut difference = right;
        difference -= left;
        assert_eq!(difference, Vec3::new(3.0, 6.0, 9.0));
        let mut scaled = left;
        scaled *= 2.0;
        assert_eq!(scaled, Vec3::new(2.0, 4.0, 6.0));
        let mut divided = right;
        divided /= 2.0;
        assert_eq!(divided, Vec3::new(2.0, 4.0, 6.0));
    }

    // Tripwire: right-handed cross product.
    #[test]
    fn cross_follows_right_hand_rule() {
        assert_eq!(Vec3::X.cross(Vec3::Y), Vec3::Z);
        assert_eq!(Vec3::Y.cross(Vec3::Z), Vec3::X);
        assert_eq!(Vec3::Z.cross(Vec3::X), Vec3::Y);
    }

    // Tripwire: a quarter turn about `Y` takes `X` to `-Z` (ADR-0005).
    #[test]
    fn rotate_about_y_quarter_turn_takes_x_to_neg_z() {
        let turned = Vec3::X.rotate_about(Vec3::Y, FRAC_PI_2);
        let expected = -Vec3::Z;
        let aligned = close_vec3(turned, expected, 1e-5);
        assert!(aligned);
    }

    // Zero has no direction to keep, so normalizing keeps zero.
    #[test]
    fn normalize_of_zero_is_zero() {
        assert_eq!(Vec3::ZERO.normalize(), Vec3::ZERO);
    }

    // A near-zero vector would blow up when normalized, so the fallback wins.
    #[test]
    fn normalize_or_returns_fallback_for_tiny_input() {
        let tiny = Vec3::new(1e-7, 0.0, 0.0);
        let fallback = Vec3::Z;
        assert_eq!(tiny.normalize_or(fallback), fallback);
    }

    // A plain vector normalizes to a unit vector.
    #[test]
    fn normalize_or_returns_unit_vector_for_plain_input() {
        let plain = Vec3::new(3.0, 4.0, 0.0);
        let unit = plain.normalize_or(Vec3::Y);
        let unit_length = close(unit.length(), 1.0, 1e-5);
        assert!(unit_length);
    }
}
