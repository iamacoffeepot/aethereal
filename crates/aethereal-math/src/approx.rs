use crate::{Vec2, Vec3, Vec4};

/// Returns true when `left` and `right` differ by less than `tolerance`.
pub fn close(left: f32, right: f32, tolerance: f32) -> bool {
    (left - right).abs() < tolerance
}

/// Returns true when every component pair of `left` and `right` is `close`.
pub fn close_vec2(left: Vec2, right: Vec2, tolerance: f32) -> bool {
    let close_x = close(left.x, right.x, tolerance);
    let close_y = close(left.y, right.y, tolerance);
    close_x && close_y
}

/// Returns true when every component pair of `left` and `right` is `close`.
pub fn close_vec3(left: Vec3, right: Vec3, tolerance: f32) -> bool {
    let close_x = close(left.x, right.x, tolerance);
    let close_y = close(left.y, right.y, tolerance);
    let close_z = close(left.z, right.z, tolerance);
    close_x && close_y && close_z
}

/// Returns true when every component pair of `left` and `right` is `close`.
pub fn close_vec4(left: Vec4, right: Vec4, tolerance: f32) -> bool {
    let close_x = close(left.x, right.x, tolerance);
    let close_y = close(left.y, right.y, tolerance);
    let close_z = close(left.z, right.z, tolerance);
    let close_w = close(left.w, right.w, tolerance);
    close_x && close_y && close_z && close_w
}
