//! Input performed on a window: what a person does at it, asked of the window's actor, which sends it to its
//! subscribers as though its host had heard it (ADR-0046).

use crate::event::{Button, Event};
use aethereal_data::Plain;

/// The most inputs one request performs: far more than a script's step needs, and a bound on one turn's sends. An
/// engine reads a piece of mail within its `read_limit`, 1024 bytes by default, so a request of more than a few dozen
/// is read only by an engine set to read more; `send_input` asks one input a request (ADR-0046).
pub const MOST_INPUTS: u32 = 1024;

/// The most bytes a key's `code` or `key` is: far longer than any the W3C UI Events name, so one input is always read
/// within an engine's default `read_limit`.
pub const MOST_NAME_BYTES: u32 = 64;

/// One thing a person does at a window: an [`Event`] that changes nothing about how the window stands, named and
/// measured as the event of the same name is.
#[derive(Debug, Clone, PartialEq, Plain)]
pub enum Input {
    PointerMoved { x: f32, y: f32 },
    PointerDown { x: f32, y: f32, button: Button },
    PointerUp { x: f32, y: f32, button: Button },
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    KeyDown { code: String, key: String, repeat: bool },
    KeyUp { code: String, key: String },
}

impl Input {
    /// Whether every position and delta it carries is finite, as every one a host sends is.
    fn is_finite(&self) -> bool {
        match self {
            Self::PointerMoved { x, y } | Self::PointerDown { x, y, .. } | Self::PointerUp { x, y, .. } => {
                x.is_finite() && y.is_finite()
            }
            Self::Wheel { x, y, delta_x, delta_y } => {
                x.is_finite() && y.is_finite() && delta_x.is_finite() && delta_y.is_finite()
            }
            Self::KeyDown { .. } | Self::KeyUp { .. } => true,
        }
    }

    /// Whether a key's names are each at most [`MOST_NAME_BYTES`].
    fn names_fit(&self) -> bool {
        let most = MOST_NAME_BYTES as usize;
        match self {
            Self::KeyDown { code, key, .. } | Self::KeyUp { code, key } => code.len() <= most && key.len() <= most,
            Self::PointerMoved { .. } | Self::PointerDown { .. } | Self::PointerUp { .. } | Self::Wheel { .. } => true,
        }
    }
}

impl From<Input> for Event {
    fn from(input: Input) -> Self {
        match input {
            Input::PointerMoved { x, y } => Self::PointerMoved { x, y },
            Input::PointerDown { x, y, button } => Self::PointerDown { x, y, button },
            Input::PointerUp { x, y, button } => Self::PointerUp { x, y, button },
            Input::Wheel { x, y, delta_x, delta_y } => Self::Wheel { x, y, delta_x, delta_y },
            Input::KeyDown { code, key, repeat } => Self::KeyDown { code, key, repeat },
            Input::KeyUp { code, key } => Self::KeyUp { code, key },
        }
    }
}

/// Asks a window's actor, at `path::<Window>(window)`, to send its subscribers `inputs`, in order, as its host would
/// have, whether or not the window has the keyboard: answered with [`Performed`] once it has, or would not.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.window.perform")]
pub struct Perform {
    pub inputs: Vec<Input>,
}

/// What became of input asked to be performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.performed")]
pub enum Performed {
    /// Every input was sent to the window's subscribers, before this answer.
    Performed,
    /// None was sent, for this reason.
    Refused(Unperformed),
}

/// Why input asked to be performed was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Unperformed {
    /// No window's actor is at the path: the window never opened, or it has closed. `send_input` answers this.
    NotOpen,
    /// Input `at`, counted from 0, has a position or a delta that is not finite, which no host sends.
    NotFinite { at: u32 },
    /// Input `at`, counted from 0, is a key whose `code` or `key` is longer than `most` bytes.
    NameTooLong { at: u32, most: u32 },
    /// More inputs than the most one request performs.
    TooMany { most: u32 },
}

/// Why `inputs` cannot be performed, if they cannot.
pub fn unperformable(inputs: &[Input]) -> Option<Unperformed> {
    let most = MOST_INPUTS as usize;
    if inputs.len() > most {
        return Some(Unperformed::TooMany { most: MOST_INPUTS });
    }
    for (input, at) in inputs.iter().zip(0..) {
        if !input.is_finite() {
            return Some(Unperformed::NotFinite { at });
        }
        if !input.names_fit() {
            return Some(Unperformed::NameTooLong { at, most: MOST_NAME_BYTES });
        }
    }

    None
}
