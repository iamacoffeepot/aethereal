use aethereal_data::Plain;

use crate::{Mat4, Vec2, Vec3};

/// A half-line with an origin and a direction.
#[derive(Plain, Debug, Clone, Copy, PartialEq)]
#[must_use]
pub struct Ray {
    /// Where the ray starts.
    pub origin: Vec3,
    /// Where the ray goes. Need not be a unit vector.
    pub direction: Vec3,
}

impl Ray {
    /// Returns `origin + direction * distance`.
    #[inline]
    pub fn at(self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }

    /// Returns the ray through `ndc` (x and y from -1 to 1, y up) from the near plane toward the
    /// far plane, for the inverse of a `projection * view`. Its direction is a unit vector. `None`
    /// when either end cannot be projected or the two coincide.
    #[inline]
    #[must_use]
    pub fn unproject(inverse_view_projection: Mat4, ndc: Vec2) -> Option<Self> {
        let start = Vec3::new(ndc.x, ndc.y, 0.0);
        let end = Vec3::new(ndc.x, ndc.y, 1.0);
        let near = inverse_view_projection.project_point(start)?;
        let far = inverse_view_projection.project_point(end)?;
        let span = far - near;
        let separated = span.length_squared() > 0.0;
        if separated {
            Some(Self { origin: near, direction: span.normalize() })
        } else {
            None
        }
    }

    /// Returns the distance along the ray to the plane through `point` with `normal`. `None` when
    /// the ray is parallel to the plane (`along * along <= 1e-10 * direction.length_squared() *
    /// normal.length_squared()`) or the plane is behind the origin.
    #[inline]
    #[must_use]
    pub fn plane_hit(self, point: Vec3, normal: Vec3) -> Option<f32> {
        let along = self.direction.dot(normal);
        let direction_squared = self.direction.length_squared();
        let normal_squared = normal.length_squared();
        let scale = direction_squared * normal_squared;
        let threshold = 1e-10 * scale;
        let parallel = along * along <= threshold;
        if parallel {
            return None;
        }
        let offset = point - self.origin;
        let numerator = offset.dot(normal);
        let distance = numerator / along;
        let ahead = distance >= 0.0;
        if ahead {
            Some(distance)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx::{close, close_vec3};
    use core::f32::consts::FRAC_PI_2;

    // The centre of the screen runs from the near plane straight at the target.
    #[test]
    fn centre_ray_runs_from_near_plane_to_target() {
        let view = Mat4::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Y);
        let projection = Mat4::perspective(FRAC_PI_2, 1.0, 1.0, 10.0);
        let Some(inverse) = (projection * view).inverse() else {
            panic!("a view-projection inverts");
        };
        let Some(ray) = Ray::unproject(inverse, Vec2::new(0.0, 0.0)) else {
            panic!("the centre unprojects");
        };
        let starts_near = close_vec3(ray.origin, Vec3::new(0.0, 0.0, 4.0), 1e-5);
        let points_forward = close_vec3(ray.direction, Vec3::new(0.0, 0.0, -1.0), 1e-5);
        let reaches_target = close_vec3(ray.at(4.0), Vec3::ZERO, 1e-5);
        assert!(starts_near);
        assert!(points_forward);
        assert!(reaches_target);
    }

    // An orthographic projection casts parallel rays offset by the screen position.
    #[test]
    fn orthographic_rays_are_parallel_and_offset() {
        let projection = Mat4::orthographic(-2.0, 2.0, -2.0, 2.0, 0.1, 10.0);
        let Some(inverse) = (projection * Mat4::IDENTITY).inverse() else {
            panic!("an orthographic projection inverts");
        };
        let Some(left) = Ray::unproject(inverse, Vec2::new(-0.5, 0.0)) else {
            panic!("the left ray unprojects");
        };
        let Some(right) = Ray::unproject(inverse, Vec2::new(0.5, 0.0)) else {
            panic!("the right ray unprojects");
        };
        let forward = Vec3::new(0.0, 0.0, -1.0);
        let left_forward = close_vec3(left.direction, forward, 1e-5);
        let parallel = close_vec3(right.direction, left.direction, 1e-5);
        let left_placed = close_vec3(left.origin, Vec3::new(-1.0, 0.0, -0.1), 1e-5);
        let right_placed = close_vec3(right.origin, Vec3::new(1.0, 0.0, -0.1), 1e-5);
        assert!(left_forward);
        assert!(parallel);
        assert!(left_placed);
        assert!(right_placed);
    }

    // A matrix that flattens depth leaves no span to normalize.
    #[test]
    fn unproject_through_flat_depth_is_none() {
        let flattened = Mat4::from_scale(Vec3::new(1.0, 1.0, 0.0));
        assert_eq!(Ray::unproject(flattened, Vec2::new(0.0, 0.0)), None);
    }

    // A tilted plane meets the ray two units down, at the origin.
    #[test]
    fn plane_hit_measures_tilted_plane() {
        let ray = Ray { origin: Vec3::new(0.0, 2.0, 0.0), direction: Vec3::new(0.0, -1.0, 0.0) };
        let Some(distance) = ray.plane_hit(Vec3::ZERO, Vec3::new(0.0, 1.0, 1.0)) else {
            panic!("the ray crosses the plane");
        };
        let pinned = close(distance, 2.0, 1e-5);
        let at_origin = close_vec3(ray.at(distance), Vec3::ZERO, 1e-5);
        assert!(pinned);
        assert!(at_origin);
    }

    // Scaling the plane normal scales both sides of the divide away.
    #[test]
    fn plane_hit_ignores_normal_length() {
        let ray = Ray { origin: Vec3::new(0.0, 2.0, 0.0), direction: Vec3::new(0.0, -1.0, 0.0) };
        let unit = Vec3::new(0.0, 1.0, 1.0).normalize();
        let Some(short) = ray.plane_hit(Vec3::ZERO, unit) else {
            panic!("the unit normal hits");
        };
        let Some(long) = ray.plane_hit(Vec3::ZERO, Vec3::new(0.0, 8.0, 8.0)) else {
            panic!("the long normal hits");
        };
        let same = close(short, long, 1e-5);
        assert!(same);
    }

    // A ray across the plane never meets it.
    #[test]
    fn plane_hit_parallel_is_none() {
        let ray = Ray { origin: Vec3::new(0.0, 2.0, 0.0), direction: Vec3::X };
        assert_eq!(ray.plane_hit(Vec3::ZERO, Vec3::Y), None);
    }

    // A plane behind the origin is not ahead.
    #[test]
    fn plane_hit_behind_is_none() {
        let ray = Ray { origin: Vec3::new(0.0, 2.0, 0.0), direction: Vec3::Y };
        assert_eq!(ray.plane_hit(Vec3::ZERO, Vec3::Y), None);
    }
}
