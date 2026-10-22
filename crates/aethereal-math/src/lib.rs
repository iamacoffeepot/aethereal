//! Vectors, a matrix, a rotation, boxes, and a ray, and curves, polylines, and polygons in a
//! plane (ADR-0005).
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
#[cfg(any(test, feature = "cases"))]
pub mod cases;
mod curve;
mod mat4;
mod polygon;
mod polyline;
mod quat;
mod ray;
mod rect2;
mod segment;
mod vector;

pub use aabb::Aabb;
pub use curve::{CatmullRom, CatmullRomSamples};
pub use mat4::Mat4;
pub use polygon::Polygon;
pub use polyline::{Ends, NearestSegment, Polyline, Segments};
pub use quat::Quat;
pub use ray::Ray;
pub use rect2::Rect2;
pub use segment::{Segment, Side};
pub use vector::{Vec2, Vec3, Vec4};
