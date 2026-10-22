//! How the disc is seen: tilted away from facing the screen and turned on it, the same for the stars the GPU draws and
//! for the pointer the disc maps back onto the disc's plane.

use crate::bytes::floats;

/// How far the disc is tilted away from facing the screen, in radians.
const TILT: f32 = 1.02;

/// How far the disc's axes are turned on the screen, in radians, counter-clockwise.
const ROLL: f32 = -0.38;

/// The clip units each of the disc's units spans up the screen.
const ZOOM: f32 = 1.0;

/// A view of the disc on a surface of `width` by `height` pixels.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    width: f32,
    height: f32,
}

/// The pixels of a side of a surface, as a float: a canvas's side is far below what sixteen bits count.
fn pixels(side: u32) -> f32 {
    f32::from(u16::try_from(side).unwrap_or(u16::MAX).max(1))
}

impl Camera {
    /// A view of the disc on a surface of `width` by `height` pixels.
    pub fn of(width: u32, height: u32) -> Self {
        Self { width: pixels(width), height: pixels(height) }
    }

    /// The clip units of one of the screen's units, across and up: the same up as across on the screen.
    fn scale(self) -> [f32; 2] {
        [ZOOM * self.height / self.width, ZOOM]
    }

    /// The bytes of the uniforms the stars are drawn with on this surface: where the disc's axes and its up land on
    /// the screen, the clip units of the screen's, half a sprite of `sprite` pixels, and the light `gain` each gives.
    pub fn view(self, sprite: f32, gain: f32) -> Vec<u8> {
        let (tilt_sin, tilt_cos) = TILT.sin_cos();
        let (roll_sin, roll_cos) = ROLL.sin_cos();
        let across = [roll_cos, roll_sin];
        let along = [-roll_sin * tilt_cos, roll_cos * tilt_cos];
        let up = [-roll_sin * tilt_sin, roll_cos * tilt_sin];
        let half = [sprite * 2.0 / self.width, sprite * 2.0 / self.height];

        floats([across, along, up, self.scale(), half].into_iter().flatten().chain([gain, 0.0]))
    }

    /// Where on the disc's plane the pixel at `x`, `y` of the surface, from its top left, shows.
    pub fn on_disc(self, x: f32, y: f32) -> [f32; 2] {
        let [scale_across, scale_up] = self.scale();
        let screen_x = (2.0 * x / self.width - 1.0) / scale_across;
        let screen_y = (1.0 - 2.0 * y / self.height) / scale_up;
        let (roll_sin, roll_cos) = ROLL.sin_cos();
        let unrolled_x = screen_x.mul_add(roll_cos, screen_y * roll_sin);
        let unrolled_y = screen_y.mul_add(roll_cos, -screen_x * roll_sin);

        [unrolled_x, unrolled_y / TILT.cos()]
    }
}
