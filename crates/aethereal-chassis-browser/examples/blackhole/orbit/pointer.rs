//! The pointer as the orbit follows it: up, or held down at a place on the surface, and how far each move drags it.

use aethereal_data::Plain;
use aethereal_window::{Button, Event};

/// The pointer, as the window last said.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub enum Pointer {
    /// Its main button is up: it drags nothing.
    Up,
    /// Its main button is held down at `x`, `y` of the surface, in its pixels from the top left.
    Held { x: f32, y: f32 },
}

/// How far the pointer dragged on one event, in the surface's pixels, rightward and downward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dragged {
    pub right: f32,
    pub down: f32,
}

impl Dragged {
    /// No drag at all.
    const NONE: Self = Self { right: 0.0, down: 0.0 };
}

impl Pointer {
    /// The pointer once the window heard `event`, and how far it dragged: held where its main button went down and
    /// wherever it then moves, dragging by each move while held, and up once the button comes up or the window loses
    /// the keyboard.
    pub const fn heard(self, event: &Event) -> (Self, Dragged) {
        match (self, event) {
            (_, Event::PointerDown { x, y, button: Button::Main }) => (Self::Held { x: *x, y: *y }, Dragged::NONE),
            (Self::Held { x: from_x, y: from_y }, Event::PointerMoved { x, y }) => {
                (Self::Held { x: *x, y: *y }, Dragged { right: *x - from_x, down: *y - from_y })
            }
            (_, Event::PointerUp { button: Button::Main, .. } | Event::FocusLost) => (Self::Up, Dragged::NONE),
            _ => (self, Dragged::NONE),
        }
    }

    /// Whether the pointer is held down, so the camera follows it rather than drifting.
    pub const fn held(self) -> bool {
        matches!(self, Self::Held { .. })
    }
}
