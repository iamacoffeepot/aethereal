//! Vectors, a matrix, a rotation, boxes, and a ray (ADR-0005).
//!
//! Space is right-handed with `Y` up and forward `-Z`. Angles are radians.
//! `Mat4` is column-major and multiplies a column vector on its right, so
//! `a * b` applies `b` first. Euler angles have the one order `YXZ`.
//! Clip depth runs from `0` to `1`.

#![no_std]
#![forbid(unsafe_code)]

mod aabb;
#[cfg(test)]
mod approx;
mod mat4;
mod quat;
mod ray;
mod rect2;
mod vector;

pub use aabb::Aabb;
pub use mat4::Mat4;
pub use quat::Quat;
pub use ray::Ray;
pub use rect2::Rect2;
pub use vector::{Vec2, Vec3, Vec4};
