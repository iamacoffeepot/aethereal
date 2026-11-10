//! Where the overlay draws, worked out from the window's surface each time it is needed (ADR-0053: The three stops):
//! the band across the top, a tenth of the window's height, in two columns; the chart's figures on one line at the
//! foot of the left column, and the frame chart filling the column above them; and the console's panel beneath the
//! band, a third of the window's height. All of it is in the surface's pixels, from its top left, as the renderer
//! places texts (ADR-0052).

#[cfg(test)]
mod tests;

use aethereal_math::{Rect2, Vec2};
use aethereal_window::Surface;

/// The share of the window's height the band takes.
const BAND_SHARE: f32 = 0.1;

/// The share of the window's height the console's panel takes, beneath the band.
const CONSOLE_SHARE: f32 = 1.0 / 3.0;

/// The share of the window's width the band's left column takes: the chart's column. The right column, the memory
/// meters' and the adapter's, takes the rest.
const LEFT_SHARE: f32 = 0.6;

/// The space between the band's edges and what it holds, in logical pixels.
const PADDING_PIXELS: f32 = 10.0;

/// The space between the chart and its figures, in logical pixels.
const GAP_PIXELS: f32 = 4.0;

/// The size of the overlay's text, in logical pixels to the em: what reads on a window 1080 pixels tall.
const TEXT_PIXELS: f32 = 14.0;

/// How tall one line of the overlay's text is, in ems: IBM Plex Mono's line height, its ascender and descender with no
/// gap between lines, 1.3 em.
const LINE_EMS: f32 = 1.3;

/// How wide each of the chart's columns is drawn, in logical pixels, and the gap after it: at 1080, 226 columns of
/// about a quarter of a second each.
const COLUMN_PIXELS: f32 = 4.0;
const COLUMN_GAP_PIXELS: f32 = 1.0;

/// The window's surface as the overlay lays itself out on it: its width and height in its pixels, and how many of
/// them make one of the host's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    width: f32,
    height: f32,
    scale: f32,
}

impl Layout {
    /// The layout on `surface`.
    #[must_use]
    pub fn of(surface: Surface) -> Self {
        // A surface is some thousands of pixels across, which an f32 holds exactly, and a screen's scale a small
        // number that an f32 holds closely enough to lay out by.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let (width, height, scale) = (surface.width as f32, surface.height as f32, surface.scale as f32);

        Self { width, height, scale }
    }

    /// The surface's width and height, in its pixels.
    pub const fn size(self) -> Vec2 {
        Vec2::new(self.width, self.height)
    }

    /// `logical` logical pixels, in the surface's, rounded to a whole one.
    fn pixels(self, logical: f32) -> f32 {
        (logical * self.scale).round()
    }

    /// The band: the whole width of the window, a tenth of its height, from its top.
    pub fn band(self) -> Rect2 {
        Rect2 { min: Vec2::ZERO, max: Vec2::new(self.width, (self.height * BAND_SHARE).round()) }
    }

    /// The console's panel: the whole width of the window, a third of its height, beneath the band.
    pub fn console(self) -> Rect2 {
        let top = self.band().max.y;

        Rect2 { min: Vec2::new(0.0, top), max: Vec2::new(self.width, top + (self.height * CONSOLE_SHARE).round()) }
    }

    /// The frame chart: the band's left column, inside its padding, from its top down to a gap above the figures. At
    /// least a pixel tall, on a window too short to hold it.
    pub fn chart(self) -> Rect2 {
        let padding = self.pixels(PADDING_PIXELS);
        let right = ((self.width * LEFT_SHARE).round() - padding).max(padding);
        let bottom = (self.figures_at().y - self.pixels(GAP_PIXELS)).max(padding + 1.0);

        Rect2 { min: Vec2::new(padding, padding), max: Vec2::new(right, bottom) }
    }

    /// Where the chart's figures are placed: the top left of their one line, which sits on the band's foot, inside its
    /// padding.
    pub fn figures_at(self) -> Vec2 {
        let padding = self.pixels(PADDING_PIXELS);
        let line = (self.text_pixels() * LINE_EMS).ceil();

        Vec2::new(padding, self.band().max.y - padding - line)
    }

    /// The size of the overlay's text, in the surface's pixels to the em.
    #[must_use]
    pub fn text_pixels(self) -> f32 {
        TEXT_PIXELS * self.scale
    }

    /// How wide each of the chart's columns is, in the surface's pixels: at least one.
    #[must_use]
    pub fn column_pixels(self) -> f32 {
        self.pixels(COLUMN_PIXELS).max(1.0)
    }

    /// How far one column is from the next, in the surface's pixels: its width and the gap after it, at least one.
    #[must_use]
    pub fn column_pitch(self) -> f32 {
        self.column_pixels() + self.pixels(COLUMN_GAP_PIXELS).max(1.0)
    }

    /// `logical` logical pixels, in the surface's, at least one: how thick a line is drawn.
    #[must_use]
    pub fn line_pixels(self, logical: f32) -> f32 {
        self.pixels(logical).max(1.0)
    }
}
