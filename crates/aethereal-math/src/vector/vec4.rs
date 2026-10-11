use super::Vec3;
use aethereal_data::Plain;

/// A vector with four `f32` components.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Vec4 {
    /// The first component.
    pub x: f32,
    /// The second component.
    pub y: f32,
    /// The third component.
    pub z: f32,
    /// The fourth component.
    pub w: f32,
}

impl Vec4 {
    /// The vector with all components zero.
    pub const ZERO: Self = Self::splat(0.0);
    /// The vector with all components one.
    pub const ONE: Self = Self::splat(1.0);
    /// The unit vector along `x`.
    pub const X: Self = Self::new(1.0, 0.0, 0.0, 0.0);
    /// The unit vector along `y`.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0, 0.0);
    /// The unit vector along `z`.
    pub const Z: Self = Self::new(0.0, 0.0, 1.0, 0.0);
    /// The unit vector along `w`.
    pub const W: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Builds a vector from its components.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Builds a vector with every component set to `value`.
    #[inline]
    pub const fn splat(value: f32) -> Self {
        Self { x: value, y: value, z: value, w: value }
    }

    /// Builds a vector from its components in `x`, `y`, `z`, `w` order.
    #[inline]
    pub const fn from_array(components: [f32; 4]) -> Self {
        Self::new(components[0], components[1], components[2], components[3])
    }

    /// Returns the components in `x`, `y`, `z`, `w` order.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }

    /// Returns the dot product of `self` and `other`.
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
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

    /// Returns the linear interpolation between `self` and `other` at `amount`.
    #[inline]
    pub fn lerp(self, other: Self, amount: f32) -> Self {
        self + (other - self) * amount
    }

    /// Returns the first three components as a `Vec3`.
    #[inline]
    pub const fn truncate(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

impl_vec_ops!(Vec4 { x, y, z, w });

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::close_vec4;

    // Catches a field dropped from a macro invocation.
    #[test]
    fn operators_act_on_every_component() {
        let left = Vec4::new(1.0, 2.0, 3.0, 4.0);
        let right = Vec4::new(8.0, 12.0, 16.0, 20.0);
        assert_eq!(left + right, Vec4::new(9.0, 14.0, 19.0, 24.0));
        assert_eq!(right - left, Vec4::new(7.0, 10.0, 13.0, 16.0));
        assert_eq!(-left, Vec4::new(-1.0, -2.0, -3.0, -4.0));
        assert_eq!(left * 2.0, Vec4::new(2.0, 4.0, 6.0, 8.0));
        assert_eq!(right / 2.0, Vec4::new(4.0, 6.0, 8.0, 10.0));
        let mut sum = left;
        sum += right;
        assert_eq!(sum, Vec4::new(9.0, 14.0, 19.0, 24.0));
        let mut difference = right;
        difference -= left;
        assert_eq!(difference, Vec4::new(7.0, 10.0, 13.0, 16.0));
        let mut scaled = left;
        scaled *= 2.0;
        assert_eq!(scaled, Vec4::new(2.0, 4.0, 6.0, 8.0));
        let mut divided = right;
        divided /= 2.0;
        assert_eq!(divided, Vec4::new(4.0, 6.0, 8.0, 10.0));
    }

    // Zero has no direction to keep, so normalizing keeps zero.
    #[test]
    fn normalize_of_zero_is_zero() {
        assert_eq!(Vec4::ZERO.normalize(), Vec4::ZERO);
    }

    // Division by the length is inexact, so the unit vector needs a tolerance.
    #[test]
    fn normalize_gives_unit_vector() {
        let plain = Vec4::new(3.0, 0.0, 4.0, 0.0);
        let unit = plain.normalize();
        let expected = Vec4::new(0.6, 0.0, 0.8, 0.0);
        let aligned = close_vec4(unit, expected, 1e-5);
        assert!(aligned);
    }
}
