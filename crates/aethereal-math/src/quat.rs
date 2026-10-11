use core::ops::Mul;

use aethereal_data::Plain;

use crate::Vec3;

/// A rotation as a quaternion with the scalar last.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Quat {
    /// The `x` component of the vector part.
    pub x: f32,
    /// The `y` component of the vector part.
    pub y: f32,
    /// The `z` component of the vector part.
    pub z: f32,
    /// The scalar part.
    pub w: f32,
}

impl Quat {
    /// The identity rotation.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// Builds a quaternion from its components.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Returns the rotation about `axis` by `angle` radians. A zero-length axis gives `IDENTITY`.
    /// The axis is normalised here.
    #[inline]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let half = angle * 0.5;
        let sine = libm::sinf(half);
        let cosine = libm::cosf(half);
        let unit = axis.normalize();
        let degenerate = unit.length_squared() == 0.0;
        if degenerate {
            Self::IDENTITY
        } else {
            Self::new(unit.x * sine, unit.y * sine, unit.z * sine, cosine)
        }
    }

    /// Returns the rotation for yaw about `Y`, then pitch about the turned `X`, then roll about the turned `Z`.
    #[inline]
    pub fn from_euler(yaw: f32, pitch: f32, roll: f32) -> Self {
        let half_yaw = yaw * 0.5;
        let half_pitch = pitch * 0.5;
        let half_roll = roll * 0.5;
        let sine_yaw = libm::sinf(half_yaw);
        let cosine_yaw = libm::cosf(half_yaw);
        let sine_pitch = libm::sinf(half_pitch);
        let cosine_pitch = libm::cosf(half_pitch);
        let sine_roll = libm::sinf(half_roll);
        let cosine_roll = libm::cosf(half_roll);
        Self::new(
            cosine_roll * sine_pitch * cosine_yaw + sine_roll * cosine_pitch * sine_yaw,
            cosine_roll * cosine_pitch * sine_yaw - sine_roll * sine_pitch * cosine_yaw,
            sine_roll * cosine_pitch * cosine_yaw - cosine_roll * sine_pitch * sine_yaw,
            cosine_roll * cosine_pitch * cosine_yaw + sine_roll * sine_pitch * sine_yaw,
        )
    }

    /// Returns the squared length of `self`.
    #[inline]
    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Returns the length of `self`.
    #[inline]
    #[must_use]
    pub fn length(self) -> f32 {
        libm::sqrtf(self.length_squared())
    }

    /// Returns `self` with length one. Zero length gives `IDENTITY`.
    #[inline]
    pub fn normalize(self) -> Self {
        let length = self.length();
        let nonzero = length > 0.0;
        if nonzero {
            let reciprocal = 1.0 / length;
            Self::new(self.x * reciprocal, self.y * reciprocal, self.z * reciprocal, self.w * reciprocal)
        } else {
            Self::IDENTITY
        }
    }

    /// Returns the conjugate of `self`.
    #[inline]
    pub const fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Returns the inverse of `self`. Zero length gives `IDENTITY`.
    #[inline]
    pub fn inverse(self) -> Self {
        let squared = self.length_squared();
        let nonzero = squared > 0.0;
        if nonzero {
            let reciprocal = 1.0 / squared;
            Self::new(-self.x * reciprocal, -self.y * reciprocal, -self.z * reciprocal, self.w * reciprocal)
        } else {
            Self::IDENTITY
        }
    }
}

impl Mul for Quat {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        )
    }
}

impl Mul<Vec3> for Quat {
    type Output = Vec3;
    #[inline]
    fn mul(self, vector: Vec3) -> Vec3 {
        let axis = Vec3::new(self.x, self.y, self.z);
        let doubled = axis.cross(vector) * 2.0;
        let rotated = doubled * self.w;
        let correction = axis.cross(doubled);
        vector + rotated + correction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::{close, close_vec3};
    use core::f32::consts::FRAC_PI_2;

    // Tripwire: quarter turns follow ADR-0005 (counter-clockwise looking from the tip).
    #[test]
    fn quarter_turns_match_conventions() {
        let about_y = Quat::from_axis_angle(Vec3::Y, FRAC_PI_2) * Vec3::X;
        let about_x = Quat::from_axis_angle(Vec3::X, FRAC_PI_2) * Vec3::Y;
        let about_z = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2) * Vec3::X;
        let turned_y = close_vec3(about_y, -Vec3::Z, 1e-5);
        let turned_x = close_vec3(about_x, Vec3::Z, 1e-5);
        let turned_z = close_vec3(about_z, Vec3::Y, 1e-5);
        assert!(turned_y);
        assert!(turned_x);
        assert!(turned_z);
    }

    // Tripwire: `from_euler` composes as yaw, then pitch, then roll (catches a reversed order).
    #[test]
    fn euler_matches_axis_angle_product() {
        let combined = Quat::from_euler(0.3, 0.5, 0.7);
        let yaw = Quat::from_axis_angle(Vec3::Y, 0.3);
        let pitch = Quat::from_axis_angle(Vec3::X, 0.5);
        let roll = Quat::from_axis_angle(Vec3::Z, 0.7);
        let product = yaw * pitch * roll;
        let point = Vec3::new(1.0, 2.0, 3.0);
        let aligned = close_vec3(combined * point, product * point, 1e-5);
        assert!(aligned);
    }

    // Catches a swapped Hamilton product.
    #[test]
    fn product_applies_right_first() {
        let first = Quat::from_axis_angle(Vec3::X, 0.6);
        let second = Quat::from_axis_angle(Vec3::Y, 0.8);
        let point = Vec3::new(1.0, 2.0, 3.0);
        let left = (first * second) * point;
        let right = first * (second * point);
        let associated = close_vec3(left, right, 1e-5);
        assert!(associated);
    }

    // A zero axis has no direction, so the rotation is the identity.
    #[test]
    fn zero_axis_gives_identity() {
        assert_eq!(Quat::from_axis_angle(Vec3::ZERO, 1.0), Quat::IDENTITY);
    }

    // Zero has no direction to keep, so normalizing gives the identity rotation.
    #[test]
    fn normalize_of_zero_gives_identity() {
        let zero = Quat::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(zero.normalize(), Quat::IDENTITY);
    }

    // A unit quaternion times its conjugate undoes itself.
    #[test]
    fn unit_times_conjugate_is_identity() {
        let axis = Vec3::new(1.0, 2.0, 3.0).normalize();
        let unit = Quat::from_axis_angle(axis, 0.7);
        let roundtrip = unit * unit.conjugate();
        let close_x = close(roundtrip.x, 0.0, 1e-5);
        let close_y = close(roundtrip.y, 0.0, 1e-5);
        let close_z = close(roundtrip.z, 0.0, 1e-5);
        let close_w = close(roundtrip.w, 1.0, 1e-5);
        assert!(close_x);
        assert!(close_y);
        assert!(close_z);
        assert!(close_w);
    }
}
