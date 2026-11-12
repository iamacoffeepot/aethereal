//! The wheel at the console (ADR-0053, ADR-0055): a turn, in the window's surface pixels, taken as rows of the
//! scrollback, three to a notch, and what a turn comes to short of a whole row carried to the next, so a trackpad's
//! many small turns scroll as smoothly as a wheel's notches.

use aethereal_data::Plain;

/// How many rows one notch of the wheel scrolls.
const ROWS_PER_NOTCH: f32 = 3.0;

/// The share of a row the wheel has turned through since it last scrolled a whole one: down when positive, up when
/// negative.
#[derive(Plain, Debug, Clone, Copy, Default, PartialEq)]
pub struct Wheel {
    carried_rows: f32,
}

impl Wheel {
    /// Takes in a turn of `delta_pixels`, down when positive, on a window whose notch is `notch_pixels`, and answers
    /// the whole rows the view scrolls down, up when negative. What is left of a row is carried to the next turn. A
    /// turn that is not a number scrolls nothing.
    pub fn turned(&mut self, delta_pixels: f32, notch_pixels: f32) -> i64 {
        let rows = (delta_pixels / notch_pixels.max(1.0)).mul_add(ROWS_PER_NOTCH, self.carried_rows);
        if !rows.is_finite() {
            return 0;
        }
        let whole = rows.trunc();
        self.carried_rows = rows - whole;
        // A turn is some thousands of pixels at most, so some hundreds of rows, which an i64 holds exactly.
        #[allow(clippy::cast_possible_truncation)]
        let whole = whole as i64;

        whole
    }

    /// Lets go of what was carried: the view stopped at an end, and a turn on past it must not be owed back.
    pub const fn stopped(&mut self) {
        self.carried_rows = 0.0;
    }
}
