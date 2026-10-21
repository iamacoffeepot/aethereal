//! What the page's pointer events read as: where the pointer is in the canvas's pixels, and which button the page
//! numbered.

use aethereal_window::{Button, Event};

/// The canvas as the page lays it out: the size of its backing store, in its pixels, and the size it is shown at, in
/// the page's CSS pixels.
pub struct Canvas {
    width: u32,
    height: u32,
    shown_width: f64,
    shown_height: f64,
}

impl Canvas {
    /// A canvas whose backing store is `width` by `height`, shown at `shown_width` by `shown_height`.
    pub const fn new(width: u32, height: u32, shown_width: f64, shown_height: f64) -> Self {
        Self { width, height, shown_width, shown_height }
    }

    /// Where the point `from_left`, `from_top` CSS pixels from the canvas's top left falls in its pixels: each axis
    /// scaled by its backing store over its shown size, as a canvas shown larger or smaller than its store, or on a
    /// screen of more device pixels than CSS ones, is stretched. A canvas shown with no size is not scaled.
    pub fn pixel(&self, from_left: f64, from_top: f64) -> (f32, f32) {
        let scale = |stored: u32, shown: f64| {
            if shown > 0.0 {
                f64::from(stored) / shown
            } else {
                1.0
            }
        };
        let x = from_left * scale(self.width, self.shown_width);
        let y = from_top * scale(self.height, self.shown_height);

        // A canvas is at most some thousands of pixels across, which an f32 holds to a small fraction of one.
        #[allow(clippy::cast_possible_truncation)]
        let pixel = (x as f32, y as f32);

        pixel
    }
}

/// What the pointer did, as the page's event names it.
#[derive(Debug, Clone, Copy)]
pub enum Pointing {
    /// `pointermove`.
    Moved,
    /// `pointerdown`.
    Down,
    /// `pointerup`.
    Up,
}

impl Pointing {
    /// The event of the pointer doing this at `x`, `y` in the canvas's pixels, with the button the page numbered
    /// `button`, which a move does not read: none for a press or release of no button, which the page numbers below
    /// zero.
    pub fn event(self, (x, y): (f32, f32), button: i16) -> Option<Event> {
        match self {
            Self::Moved => Some(Event::PointerMoved { x, y }),
            Self::Down => button_of(button).map(|button| Event::PointerDown { x, y, button }),
            Self::Up => button_of(button).map(|button| Event::PointerUp { x, y, button }),
        }
    }
}

/// The button the page numbers `number`: none below zero, which numbers no button.
pub fn button_of(number: i16) -> Option<Button> {
    let button = match number {
        0 => Button::Main,
        1 => Button::Middle,
        2 => Button::Secondary,
        3 => Button::Back,
        4 => Button::Forward,
        other => Button::Other(u16::try_from(other).ok()?),
    };

    Some(button)
}
