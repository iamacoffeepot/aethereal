//! The pointer as the disc follows it: up, or held down at a place on the surface, and the step the simulation is
//! handed each frame, which carries where the held pointer pulls on the disc's plane.

use crate::bytes::floats;
use crate::camera::Camera;
use aethereal_data::Plain;
use aethereal_window::{Button, Event};

/// The pointer, as the window last said.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub enum Pointer {
    /// Its main button is up: it pulls nothing.
    Up,
    /// Its main button is held down at `x`, `y` of the surface, in its pixels from the top left.
    Held { x: f32, y: f32 },
}

impl Pointer {
    /// The pointer once the window heard `event`: held where its main button went down and wherever it then moves,
    /// and up once the button comes up or the window loses the keyboard.
    pub const fn heard(self, event: &Event) -> Self {
        match (self, event) {
            (_, Event::PointerDown { x, y, button: Button::Main })
            | (Self::Held { .. }, Event::PointerMoved { x, y }) => Self::Held { x: *x, y: *y },
            (_, Event::PointerUp { button: Button::Main, .. } | Event::FocusLost) => Self::Up,
            _ => self,
        }
    }

    /// The bytes of the step's uniforms for a step of `delta` seconds, the arms turned by `turned` radians at its end
    /// and turning `turning` radians a second, the pointer seen through `camera`.
    pub fn step(self, camera: Camera, delta: f32, turned: f32, turning: f32) -> Vec<u8> {
        let (at, pull) = match self {
            Self::Up => ([0.0, 0.0], 0.0),
            Self::Held { x, y } => (camera.on_disc(x, y), 1.0),
        };

        floats(at.into_iter().chain([pull, delta, turned, turning, 0.0, 0.0]))
    }
}
