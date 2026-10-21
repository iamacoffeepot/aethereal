//! Chance, as an actor keeps it: a generator whose whole state is one number in the actor's own state, so the same
//! seed plays the same match again.

use crate::court::Side;
use aethereal_data::Plain;

/// A xorshift generator: each roll moves its state on and reads a number from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Dice {
    state: u64,
}

impl Dice {
    /// Dice that roll from `seed`. Xorshift never leaves nought, so a seed of nought rolls from one.
    #[must_use]
    pub const fn seeded(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    /// Dice of their own for another actor, seeded from these without rolling them.
    #[must_use]
    pub const fn split(self) -> Self {
        Self::seeded(self.state.rotate_left(29) ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// A number from nought up to one, not reaching it.
    pub fn roll(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        let high = u16::try_from(self.state >> 48).unwrap_or(0);

        f32::from(high) / 65_536.0
    }

    /// A number from `low` up to `high`.
    pub fn between(&mut self, low: f32, high: f32) -> f32 {
        self.roll().mul_add(high - low, low)
    }

    /// One end of the court or the other, evenly.
    pub fn side(&mut self) -> Side {
        let is_left = self.roll() < 0.5;
        if is_left {
            Side::Left
        } else {
            Side::Right
        }
    }
}
