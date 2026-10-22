//! The camera's place on its orbit about the hole, and the uniforms it is traced from: its longitude and latitude, its
//! distance, the disk's time, and the picture's shape.

use super::pointer::Dragged;
use crate::bytes::floats;
use aethereal_data::Plain;
use core::f32::consts::{FRAC_PI_2, PI};

/// The camera's distance from the hole, in the hole's masses: about 36 M, far enough to see the disk whole, near enough
/// for the shadow to fill a good part of the picture.
const DISTANCE: f32 = 36.0;

/// The tangent of half the vertical field of view, 26 degrees, on a picture at least as wide as it is high; on a
/// narrower one it is widened so the field across stays the same.
const TAN_HALF_FOV: f32 = 0.4877;

/// The camera's latitude above the disk's plane as it begins, in radians: 7 degrees, so the disk is seen nearly edge
/// on and its far side bends up over the shadow.
const PITCH: f32 = 0.122;

/// How far the camera's latitude may be dragged from the plane, in radians: a little short of either pole.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.1;

/// How fast the camera drifts round the hole when nothing drags it, in radians a second.
const DRIFT: f32 = 0.035;

/// How fast the disk's time runs, in masses of light travel time each second of the page's: the innermost orbits take
/// about ten seconds, the outer ones a minute.
const MASSES_PER_SECOND: f32 = 10.0;

/// The light's gain before the tone map.
const EXPOSURE: f32 = 1.0;

/// The camera's place and the disk's time.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct View {
    yaw: f32,
    pitch: f32,
    time: f32,
}

impl View {
    /// The view as it begins: just above the disk's plane, at the disk's time zero.
    pub const fn begun() -> Self {
        Self { yaw: 0.6, pitch: PITCH, time: 0.0 }
    }

    /// The view after the pointer was `dragged` over a surface `height` pixels high: a drag the height of the surface
    /// turns the camera half way round, and dragging down raises it, its latitude held short of the poles.
    pub fn dragged(self, dragged: Dragged, height: f32) -> Self {
        let turning = PI / height.max(1.0);
        let yaw = (-dragged.right).mul_add(turning, self.yaw);
        let pitch = dragged.down.mul_add(turning, self.pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);

        Self { yaw, pitch, ..self }
    }

    /// The view `seconds` later: the disk turned on, and the camera drifted round the hole unless `held`.
    pub fn later(self, seconds: f32, held: bool) -> Self {
        let drift = if held {
            0.0
        } else {
            DRIFT
        };
        let yaw = drift.mul_add(seconds, self.yaw) % (2.0 * PI);
        let time = MASSES_PER_SECOND.mul_add(seconds, self.time);

        Self { yaw, time, ..self }
    }

    /// The bytes of the uniforms the photons are traced from, for a picture `aspect` times as wide as it is high.
    pub fn uniforms(self, aspect: f32) -> Vec<u8> {
        let tan_half_fov = TAN_HALF_FOV / aspect.min(1.0);

        floats([self.yaw, self.pitch, DISTANCE, self.time, aspect, tan_half_fov, EXPOSURE, 0.0])
    }
}
