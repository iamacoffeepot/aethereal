//! What the page's events read as: where the pointer is in the canvas's pixels, which button the page numbered, and
//! what a control holds.

use super::{Button, Event, Value};

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
    /// `button`, which a move does not read.
    pub const fn event(self, (x, y): (f32, f32), button: i16) -> Event {
        match self {
            Self::Moved => Event::PointerMoved { x, y },
            Self::Down => Event::PointerDown { x, y, button: button_of(button) },
            Self::Up => Event::PointerUp { x, y, button: button_of(button) },
        }
    }
}

/// The button the page numbers `number`.
pub const fn button_of(number: i16) -> Button {
    match number {
        0 => Button::Main,
        1 => Button::Middle,
        2 => Button::Secondary,
        3 => Button::Back,
        4 => Button::Forward,
        other => Button::Other(other),
    }
}

/// What an input element of type `kind` holds, its value `value` and whether it is `checked`: a checkbox or radio
/// button by whether it is checked, a range or number input by its number when its value reads as a finite one, and
/// any other by its value as written.
pub fn value_of(kind: &str, value: String, checked: bool) -> Value {
    match kind {
        "checkbox" | "radio" => Value::Checked(checked),
        "range" | "number" => {
            let number = value.parse::<f64>().ok().filter(|number| number.is_finite());
            number.map_or(Value::Text(value), Value::Number)
        }
        _ => Value::Text(value),
    }
}
