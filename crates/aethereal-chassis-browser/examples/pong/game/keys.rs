//! The keys the player holds, as the window names them (ADR-0024): the arrows, or W and S, move the player's paddle,
//! and Space or Enter starts a match once one is won.

use aethereal_data::Plain;

/// The keys that move the paddle that are held down, a bit each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Keys {
    held: u8,
}

/// The bits of the keys that move the paddle up.
const UP: u8 = 0b0011;

/// The bits of the keys that move the paddle down.
const DOWN: u8 = 0b1100;

/// The bit of the key `code` names, or none for a key that does not move the paddle.
fn bit(code: &str) -> u8 {
    match code {
        "ArrowUp" => 0b0001,
        "KeyW" => 0b0010,
        "ArrowDown" => 0b0100,
        "KeyS" => 0b1000,
        _ => 0,
    }
}

impl Keys {
    /// No key held.
    pub const NONE: Self = Self { held: 0 };

    /// The keys held once the key `code` names went down.
    #[must_use]
    pub fn pressed(self, code: &str) -> Self {
        Self { held: self.held | bit(code) }
    }

    /// The keys held once the key `code` names came up.
    #[must_use]
    pub fn released(self, code: &str) -> Self {
        Self { held: self.held & !bit(code) }
    }

    /// Which way the held keys move the paddle: one up, minus one down, and nought for neither or both.
    #[must_use]
    pub const fn direction(self) -> f32 {
        let up = self.held & UP != 0;
        let down = self.held & DOWN != 0;
        match (up, down) {
            (true, false) => 1.0,
            (false, true) => -1.0,
            _ => 0.0,
        }
    }
}

/// Whether the key `code` names starts a match: Space or Enter.
#[must_use]
pub fn starts(code: &str) -> bool {
    matches!(code, "Space" | "Enter" | "NumpadEnter")
}
