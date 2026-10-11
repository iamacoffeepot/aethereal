use aethereal_data::Plain;

/// A vector with two `f32` components.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Vec2 {
    /// The first component.
    pub x: f32,
    /// The second component.
    pub y: f32,
}

impl Vec2 {
    /// The vector with both components zero.
    pub const ZERO: Self = Self::splat(0.0);
    /// The vector with both components one.
    pub const ONE: Self = Self::splat(1.0);
    /// The unit vector along `x`.
    pub const X: Self = Self::new(1.0, 0.0);
    /// The unit vector along `y`.
    pub const Y: Self = Self::new(0.0, 1.0);

    /// Builds a vector from its components.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Builds a vector with both components set to `value`.
    #[inline]
    pub const fn splat(value: f32) -> Self {
        Self { x: value, y: value }
    }

    /// Builds a vector from its components in `x`, `y` order.
    #[inline]
    pub const fn from_array(components: [f32; 2]) -> Self {
        Self::new(components[0], components[1])
    }

    /// Returns the components in `x`, `y` order.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// Returns the dot product of `self` and `other`.
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
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

    /// The smaller of each component.
    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self::new(self.x.min(other.x), self.y.min(other.y))
    }

    /// The larger of each component.
    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self::new(self.x.max(other.x), self.y.max(other.y))
    }
}

impl_vec_ops!(Vec2 { x, y });

#[cfg(test)]
mod tests {
    use super::*;

    // Catches a field dropped from a macro invocation.
    #[test]
    fn operators_act_on_every_component() {
        let left = Vec2::new(1.0, 2.0);
        let right = Vec2::new(4.0, 8.0);
        assert_eq!(left + right, Vec2::new(5.0, 10.0));
        assert_eq!(right - left, Vec2::new(3.0, 6.0));
        assert_eq!(-left, Vec2::new(-1.0, -2.0));
        assert_eq!(left * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(right / 2.0, Vec2::new(2.0, 4.0));
        let mut sum = left;
        sum += right;
        assert_eq!(sum, Vec2::new(5.0, 10.0));
        let mut difference = right;
        difference -= left;
        assert_eq!(difference, Vec2::new(3.0, 6.0));
        let mut scaled = left;
        scaled *= 2.0;
        assert_eq!(scaled, Vec2::new(2.0, 4.0));
        let mut divided = right;
        divided /= 2.0;
        assert_eq!(divided, Vec2::new(2.0, 4.0));
    }

    // Zero has no direction to keep, so normalizing keeps zero.
    #[test]
    fn normalize_of_zero_is_zero() {
        assert_eq!(Vec2::ZERO.normalize(), Vec2::ZERO);
    }
}
