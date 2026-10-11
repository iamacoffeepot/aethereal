use core::ops::Mul;

use aethereal_data::Plain;

use crate::{Quat, Vec3, Vec4};

/// A column-major 4x4 matrix as four column vectors.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Mat4 {
    /// The four columns in order.
    pub columns: [Vec4; 4],
}

impl Mat4 {
    /// The identity matrix.
    pub const IDENTITY: Self = Self {
        columns: [
            Vec4::new(1.0, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, 1.0),
        ],
    };

    /// Builds a matrix from sixteen floats in column-major order. Column 0 is the first four floats,
    /// column 1 the next four.
    #[inline]
    pub const fn from_columns_array(components: [f32; 16]) -> Self {
        Self {
            columns: [
                Vec4::new(components[0], components[1], components[2], components[3]),
                Vec4::new(components[4], components[5], components[6], components[7]),
                Vec4::new(components[8], components[9], components[10], components[11]),
                Vec4::new(components[12], components[13], components[14], components[15]),
            ],
        }
    }

    /// Returns the sixteen floats in column-major order.
    #[inline]
    #[must_use]
    pub const fn to_columns_array(self) -> [f32; 16] {
        let first = self.columns[0];
        let second = self.columns[1];
        let third = self.columns[2];
        let fourth = self.columns[3];
        [
            first.x, first.y, first.z, first.w, //
            second.x, second.y, second.z, second.w, //
            third.x, third.y, third.z, third.w, //
            fourth.x, fourth.y, fourth.z, fourth.w, //
        ]
    }

    /// Builds a matrix that translates by `translation`.
    #[inline]
    pub const fn from_translation(translation: Vec3) -> Self {
        Self {
            columns: [
                Vec4::new(1.0, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0, 0.0),
                Vec4::new(translation.x, translation.y, translation.z, 1.0),
            ],
        }
    }

    /// Builds a matrix that scales by `scale`.
    #[inline]
    pub const fn from_scale(scale: Vec3) -> Self {
        Self {
            columns: [
                Vec4::new(scale.x, 0.0, 0.0, 0.0),
                Vec4::new(0.0, scale.y, 0.0, 0.0),
                Vec4::new(0.0, 0.0, scale.z, 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Builds a rotation matrix from `rotation`.
    #[inline]
    pub fn from_rotation(rotation: Quat) -> Self {
        let x = rotation.x;
        let y = rotation.y;
        let z = rotation.z;
        let w = rotation.w;
        Self {
            columns: [
                Vec4::new(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y), 0.0),
                Vec4::new(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x), 0.0),
                Vec4::new(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y), 0.0),
                Vec4::new(0.0, 0.0, 0.0, 1.0),
            ],
        }
    }

    /// Builds a matrix that rotates, then translates.
    #[inline]
    pub fn from_rotation_translation(rotation: Quat, translation: Vec3) -> Self {
        let rotated = Self::from_rotation(rotation);
        Self {
            columns: [
                rotated.columns[0],
                rotated.columns[1],
                rotated.columns[2],
                Vec4::new(translation.x, translation.y, translation.z, 1.0),
            ],
        }
    }

    /// Returns the transpose of `self`.
    #[inline]
    pub const fn transpose(self) -> Self {
        let first = self.columns[0];
        let second = self.columns[1];
        let third = self.columns[2];
        let fourth = self.columns[3];
        Self {
            columns: [
                Vec4::new(first.x, second.x, third.x, fourth.x),
                Vec4::new(first.y, second.y, third.y, fourth.y),
                Vec4::new(first.z, second.z, third.z, fourth.z),
                Vec4::new(first.w, second.w, third.w, fourth.w),
            ],
        }
    }

    /// Returns the inverse of `self`. `None` when the determinant is zero or not finite,
    /// or its reciprocal is not finite. No tolerance.
    #[inline]
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let components = self.to_columns_array();
        let a00 = components[0];
        let a10 = components[1];
        let a20 = components[2];
        let a30 = components[3];
        let a01 = components[4];
        let a11 = components[5];
        let a21 = components[6];
        let a31 = components[7];
        let a02 = components[8];
        let a12 = components[9];
        let a22 = components[10];
        let a32 = components[11];
        let a03 = components[12];
        let a13 = components[13];
        let a23 = components[14];
        let a33 = components[15];
        let s0 = a00 * a11 - a10 * a01;
        let s1 = a00 * a12 - a10 * a02;
        let s2 = a00 * a13 - a10 * a03;
        let s3 = a01 * a12 - a11 * a02;
        let s4 = a01 * a13 - a11 * a03;
        let s5 = a02 * a13 - a12 * a03;
        let c5 = a22 * a33 - a32 * a23;
        let c4 = a21 * a33 - a31 * a23;
        let c3 = a21 * a32 - a31 * a22;
        let c2 = a20 * a33 - a30 * a23;
        let c1 = a20 * a32 - a30 * a22;
        let c0 = a20 * a31 - a30 * a21;
        let determinant = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
        let reciprocal = 1.0 / determinant;
        let determinant_finite = determinant.is_finite();
        let reciprocal_finite = reciprocal.is_finite();
        let invertible = determinant_finite && reciprocal_finite;
        if invertible {
            let b00 = (a11 * c5 - a12 * c4 + a13 * c3) * reciprocal;
            let b01 = (-a01 * c5 + a02 * c4 - a03 * c3) * reciprocal;
            let b02 = (a31 * s5 - a32 * s4 + a33 * s3) * reciprocal;
            let b03 = (-a21 * s5 + a22 * s4 - a23 * s3) * reciprocal;
            let b10 = (-a10 * c5 + a12 * c2 - a13 * c1) * reciprocal;
            let b11 = (a00 * c5 - a02 * c2 + a03 * c1) * reciprocal;
            let b12 = (-a30 * s5 + a32 * s2 - a33 * s1) * reciprocal;
            let b13 = (a20 * s5 - a22 * s2 + a23 * s1) * reciprocal;
            let b20 = (a10 * c4 - a11 * c2 + a13 * c0) * reciprocal;
            let b21 = (-a00 * c4 + a01 * c2 - a03 * c0) * reciprocal;
            let b22 = (a30 * s4 - a31 * s2 + a33 * s0) * reciprocal;
            let b23 = (-a20 * s4 + a21 * s2 - a23 * s0) * reciprocal;
            let b30 = (-a10 * c3 + a11 * c1 - a12 * c0) * reciprocal;
            let b31 = (a00 * c3 - a01 * c1 + a02 * c0) * reciprocal;
            let b32 = (-a30 * s3 + a31 * s1 - a32 * s0) * reciprocal;
            let b33 = (a20 * s3 - a21 * s1 + a22 * s0) * reciprocal;
            Some(Self {
                columns: [
                    Vec4::new(b00, b10, b20, b30),
                    Vec4::new(b01, b11, b21, b31),
                    Vec4::new(b02, b12, b22, b32),
                    Vec4::new(b03, b13, b23, b33),
                ],
            })
        } else {
            None
        }
    }

    /// Returns `self * (point, 1)` divided by its `w`. `None` when that `w` is zero or not finite.
    #[inline]
    #[must_use]
    pub fn project_point(self, point: Vec3) -> Option<Vec3> {
        let clip = self * point.extend(1.0);
        let nonzero = clip.w != 0.0;
        let finite = clip.w.is_finite();
        let usable = nonzero && finite;
        if usable {
            Some(Vec3::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w))
        } else {
            None
        }
    }

    /// Returns the view matrix for an eye at `eye` looking toward `target` with `up` as up.
    /// The eye at the origin looking down `-Z`, Y up.
    #[inline]
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Self {
        let axis_z = (eye - target).normalize();
        let axis_x = up.cross(axis_z).normalize();
        let axis_y = axis_z.cross(axis_x);
        let translation = Vec3::new(-axis_x.dot(eye), -axis_y.dot(eye), -axis_z.dot(eye));
        Self {
            columns: [
                Vec4::new(axis_x.x, axis_y.x, axis_z.x, 0.0),
                Vec4::new(axis_x.y, axis_y.y, axis_z.y, 0.0),
                Vec4::new(axis_x.z, axis_y.z, axis_z.z, 0.0),
                Vec4::new(translation.x, translation.y, translation.z, 1.0),
            ],
        }
    }

    /// Returns a perspective projection. `vertical_fov` in radians; `near` and `far` are positive
    /// distances; depth 0 at `near`, 1 at `far`.
    #[inline]
    pub fn perspective(vertical_fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        let half = vertical_fov * 0.5;
        let focal = 1.0 / libm::tanf(half);
        let depth = far / (near - far);
        let distance = near * far / (near - far);
        Self {
            columns: [
                Vec4::new(focal / aspect, 0.0, 0.0, 0.0),
                Vec4::new(0.0, focal, 0.0, 0.0),
                Vec4::new(0.0, 0.0, depth, -1.0),
                Vec4::new(0.0, 0.0, distance, 0.0),
            ],
        }
    }

    /// Returns an orthographic projection for the box from (`left`, `bottom`, `near`) to
    /// (`right`, `top`, `far`).
    #[inline]
    pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Self {
        let width = right - left;
        let height = top - bottom;
        let depth = near - far;
        Self {
            columns: [
                Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
                Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
                Vec4::new(0.0, 0.0, 1.0 / depth, 0.0),
                Vec4::new(-(right + left) / width, -(top + bottom) / height, near / depth, 1.0),
            ],
        }
    }
}

impl Mul<Vec4> for Mat4 {
    type Output = Vec4;
    #[inline]
    fn mul(self, vector: Vec4) -> Vec4 {
        let first = self.columns[0] * vector.x;
        let second = self.columns[1] * vector.y;
        let third = self.columns[2] * vector.z;
        let fourth = self.columns[3] * vector.w;
        first + second + third + fourth
    }
}

impl Mul for Mat4 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        let first = self * rhs.columns[0];
        let second = self * rhs.columns[1];
        let third = self * rhs.columns[2];
        let fourth = self * rhs.columns[3];
        Self { columns: [first, second, third, fourth] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::{close, close_vec4};
    use core::f32::consts::FRAC_PI_2;

    // Tripwire: the translation sits in the last column (ADR-0005).
    #[test]
    fn translation_flattens_column_major() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let flat = matrix.to_columns_array();
        assert_eq!(&flat[12..16], &[1.0, 2.0, 3.0, 1.0]);
    }

    // Tripwire: the encoded matrix is the uniform (ADR-0005).
    #[test]
    fn translation_encodes_as_uniform_bytes() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let Ok(bytes) = aethereal_data::to_bytes(&matrix) else {
            panic!("a matrix encodes");
        };
        assert_eq!(bytes.len(), 64);
        let start = bytes.len() - 16;
        let tail = &bytes[start..];
        let one = 1.0f32.to_le_bytes();
        let two = 2.0f32.to_le_bytes();
        let three = 3.0f32.to_le_bytes();
        assert_eq!(&tail[0..4], &one);
        assert_eq!(&tail[4..8], &two);
        assert_eq!(&tail[8..12], &three);
        assert_eq!(&tail[12..16], &one);
    }

    // A translation moves points but leaves directions alone.
    #[test]
    fn translation_moves_point_not_direction() {
        let matrix = Mat4::from_translation(Vec3::new(2.0, 3.0, 4.0));
        let point = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let direction = Vec4::new(1.0, 0.0, 0.0, 0.0);
        assert_eq!(matrix * point, Vec4::new(3.0, 3.0, 4.0, 1.0));
        assert_eq!(matrix * direction, direction);
    }

    // Catches a reversed product.
    #[test]
    fn order_of_scale_and_translate_matters() {
        let translate = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let scale = Mat4::from_scale(Vec3::new(2.0, 2.0, 2.0));
        let point = Vec4::new(1.0, 1.0, 1.0, 1.0);
        assert_eq!((translate * scale) * point, Vec4::new(3.0, 4.0, 5.0, 1.0));
        assert_eq!((scale * translate) * point, Vec4::new(4.0, 6.0, 8.0, 1.0));
    }

    // The matrix and the quaternion say the same rotation.
    #[test]
    fn rotation_matrix_matches_quaternion() {
        let rotation = Quat::from_axis_angle(Vec3::Y, 0.7);
        let matrix = Mat4::from_rotation(rotation);
        let point = Vec3::new(1.0, 2.0, 3.0);
        let via_matrix = matrix * point.extend(0.0);
        let via_quat = (rotation * point).extend(0.0);
        let aligned = close_vec4(via_matrix, via_quat, 1e-5);
        assert!(aligned);
    }

    // Tripwire: transpose layout with sixteen distinct entries.
    #[test]
    fn transpose_swaps_rows_and_columns() {
        let matrix = Mat4::from_columns_array([
            1.0, 2.0, 3.0, 4.0, //
            5.0, 6.0, 7.0, 8.0, //
            9.0, 10.0, 11.0, 12.0, //
            13.0, 14.0, 15.0, 16.0, //
        ]);
        let expected = Mat4::from_columns_array([
            1.0, 5.0, 9.0, 13.0, //
            2.0, 6.0, 10.0, 14.0, //
            3.0, 7.0, 11.0, 15.0, //
            4.0, 8.0, 12.0, 16.0, //
        ]);
        assert_eq!(matrix.transpose(), expected);
    }

    // The inverse undoes a transform with rotation, translation, and scale.
    #[test]
    fn inverse_undoes_transform() {
        let rotation = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0).normalize(), 0.7);
        let matrix = Mat4::from_rotation_translation(rotation, Vec3::new(4.0, -2.0, 5.0))
            * Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));
        let Some(inverted) = matrix.inverse() else {
            panic!("a transform with scale inverts");
        };
        let roundtrip = (inverted * matrix).to_columns_array();
        let identity = Mat4::IDENTITY.to_columns_array();
        for (index, got) in roundtrip.iter().enumerate() {
            let want = identity[index];
            let aligned = close(*got, want, 1e-4);
            assert!(aligned, "element {index}");
        }
    }

    // The inverse undoes a perspective projection.
    #[test]
    fn inverse_undoes_perspective() {
        let projection = Mat4::perspective(FRAC_PI_2 * 0.8, 1.6, 0.5, 40.0);
        let Some(inverted) = projection.inverse() else {
            panic!("a perspective projection inverts");
        };
        let roundtrip = (inverted * projection).to_columns_array();
        let identity = Mat4::IDENTITY.to_columns_array();
        for (index, got) in roundtrip.iter().enumerate() {
            let want = identity[index];
            let aligned = close(*got, want, 1e-4);
            assert!(aligned, "element {index}");
        }
    }

    // Catches infinities from a zero determinant reaching a caller as a matrix.
    #[test]
    fn inverse_of_flat_scale_is_none() {
        let flat = Mat4::from_scale(Vec3::new(2.0, 0.0, 3.0));
        assert_eq!(flat.inverse(), None);
    }

    // A point on the plane through the eye has no projection.
    #[test]
    fn project_point_on_eye_plane_is_none() {
        let projection = Mat4::perspective(FRAC_PI_2, 1.0, 1.0, 10.0);
        assert_eq!(projection.project_point(Vec3::new(1.0, 2.0, 0.0)), None);
    }

    // Tripwire: `look_at` puts the eye at the origin looking down `-Z`.
    #[test]
    fn look_at_sends_origin_behind_eye() {
        let view = Mat4::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let moved = view * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let placed = close_vec4(moved, Vec4::new(0.0, 0.0, -5.0, 1.0), 1e-5);
        assert!(placed);
    }

    // Tripwire: perspective depth is 0 at `near` and 1 at `far`.
    #[test]
    fn perspective_maps_near_to_zero_far_to_one() {
        let projection = Mat4::perspective(FRAC_PI_2, 1.0, 1.0, 10.0);
        let near_clip = projection * Vec4::new(0.0, 0.0, -1.0, 1.0);
        let far_clip = projection * Vec4::new(0.0, 0.0, -10.0, 1.0);
        let near_depth = near_clip.z / near_clip.w;
        let far_depth = far_clip.z / far_clip.w;
        let near_placed = close(near_depth, 0.0, 1e-5);
        let far_placed = close(far_depth, 1.0, 1e-5);
        assert!(near_placed);
        assert!(far_placed);
    }

    // Tripwire: orthographic corners map to the cube corners.
    #[test]
    fn orthographic_maps_box_corners() {
        let projection = Mat4::orthographic(-1.0, 1.0, -1.0, 1.0, 1.0, 10.0);
        let near_corner = projection * Vec4::new(-1.0, -1.0, -1.0, 1.0);
        let far_corner = projection * Vec4::new(1.0, 1.0, -10.0, 1.0);
        let near_placed = close_vec4(near_corner, Vec4::new(-1.0, -1.0, 0.0, 1.0), 1e-5);
        let far_placed = close_vec4(far_corner, Vec4::new(1.0, 1.0, 1.0, 1.0), 1e-5);
        assert!(near_placed);
        assert!(far_placed);
    }
}
