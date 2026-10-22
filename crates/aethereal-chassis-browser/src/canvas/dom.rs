//! What the page's pointer and wheel events read as: where the pointer is in the canvas's pixels, which button the
//! page numbered, and how far the wheel turned, in the canvas's pixels too; and how many pixels the canvas is drawn
//! at, from the size the page lays it out at.

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

    /// The event of the wheel turned by `delta_x`, `delta_y`, counted as the page's `deltaMode` `mode` says, with the
    /// pointer `from_left`, `from_top` CSS pixels from the canvas's top left: the place and the turn each in the
    /// canvas's pixels, scaled on each axis as [`pixel`](Self::pixel) scales a point, and each delta's sign as the page
    /// gives it, positive down and right. None for a mode the page has no name for.
    pub fn wheel(&self, (from_left, from_top): (f64, f64), (delta_x, delta_y): (f64, f64), mode: u32) -> Option<Event> {
        let (x, y) = self.pixel(from_left, from_top);
        let (delta_x, delta_y) = self.pixel(css_pixels(delta_x, mode)?, css_pixels(delta_y, mode)?);

        Some(Event::Wheel { x, y, delta_x, delta_y })
    }
}

/// The CSS pixels a line of the wheel's turn is taken as, for a page that counts the wheel in lines.
pub const LINE_CSS_PIXELS: f64 = 40.0;

/// The CSS pixels a page of the wheel's turn is taken as, for a page that counts the wheel in pages.
pub const PAGE_CSS_PIXELS: f64 = 800.0;

/// `delta` of the wheel's turn, counted as the page's `WheelEvent.deltaMode` `mode` says, in CSS pixels: as it is for
/// pixels (`0`), by [`LINE_CSS_PIXELS`] for lines (`1`), and by [`PAGE_CSS_PIXELS`] for pages (`2`). None for a mode
/// the page has no name for.
pub fn css_pixels(delta: f64, mode: u32) -> Option<f64> {
    let per = match mode {
        0 => 1.0,
        1 => LINE_CSS_PIXELS,
        2 => PAGE_CSS_PIXELS,
        _ => return None,
    };

    Some(delta * per)
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

/// How sharply the canvas is drawn: as many of its pixels to each of the page's CSS pixels as the screen has device
/// pixels, but at most `most`. A page past two device pixels to each CSS pixel may draw at two, which is no less sharp
/// to the eye and a quarter of the pixels of four.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Density {
    most: f64,
}

impl Density {
    /// The screen's device pixels to each CSS pixel, but at most `most`: below one, fewer pixels than the page lays
    /// the canvas out at. A `most` of no pixels, or one that is no number, is the screen's.
    #[must_use]
    pub const fn at_most(most: f64) -> Self {
        Self { most }
    }

    /// The canvas's pixels to each CSS pixel, on a screen of `device_pixel_ratio` device pixels to each: the screen's,
    /// at most this density's most. A screen that gives no positive ratio is taken as one device pixel to each.
    #[must_use]
    pub fn scale(self, device_pixel_ratio: f64) -> f64 {
        let screen = screen(device_pixel_ratio);
        let has_most = self.most > 0.0;

        if has_most {
            screen.min(self.most)
        } else {
            screen
        }
    }
}

/// The size the page lays the canvas out at, as it measures it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measured {
    /// In the screen's device pixels, as a page that gives `devicePixelContentBoxSize` does: whole pixels, snapped as
    /// the page shows them.
    DevicePixels { width: f64, height: f64 },
    /// In CSS pixels, as every page gives the box of its content.
    CssPixels { width: f64, height: f64 },
}

impl Measured {
    /// The canvas's pixels for this size, `width` by `height`, drawn at `density` on a screen of `device_pixel_ratio`
    /// device pixels to each CSS pixel: each side rounded to the nearest pixel, at least one, as a surface of none is
    /// not drawn, and at most `longest`, the longest side of a texture the GPU makes.
    #[must_use]
    pub fn pixels(self, density: Density, device_pixel_ratio: f64, longest: u32) -> (u32, u32) {
        let scale = density.scale(device_pixel_ratio);
        let (width, height, by) = match self {
            Self::DevicePixels { width, height } => (width, height, scale / screen(device_pixel_ratio)),
            Self::CssPixels { width, height } => (width, height, scale),
        };
        let side = |length: f64| {
            // Saturates past a `u32`, and gives none for no number, which the clamp makes one.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pixels = (length * by).round() as u32;

            pixels.clamp(1, longest)
        };

        (side(width), side(height))
    }
}

/// The screen's device pixels to each CSS pixel, as the page gives them, `device_pixel_ratio`: one when it gives no
/// positive ratio.
fn screen(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}
