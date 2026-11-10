//! Where the overlay draws, worked out from the window's surface each time it is needed (ADR-0053: The three stops):
//! the band across the top, a tenth of the window's height, in two columns; the chart's figures on one line at the
//! foot of the left column, and the frame chart filling the column above them, with the count of warnings and errors
//! at the right end of that line; and the console's panel beneath the band, a third of the window's height, cut into
//! two strips told apart by their tints and a faint line between: the scrollback above, its scroll bar in the padding
//! at its right edge, and the prompt's strip along the panel's foot, a line of text tall, with what is running at its
//! right end. All of it is in the surface's pixels, from its top left, as the renderer places texts (ADR-0052).

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

/// The space between the chart and its figures, and between the scrollback's last row and the prompt's strip, in
/// logical pixels.
const GAP_PIXELS: f32 = 4.0;

/// The space above and below the prompt's line inside its strip, in logical pixels.
const STRIP_PADDING_PIXELS: f32 = 6.0;

/// How thick the lines between the band and the console, and between the scrollback and the prompt's strip, are, in
/// logical pixels.
const DIVIDER_PIXELS: f32 = 1.0;

/// The size of the overlay's text, in logical pixels to the em: what reads on a window 1080 pixels tall.
const TEXT_PIXELS: f32 = 14.0;

/// How tall one line of the overlay's text is, in ems: IBM Plex Mono's line height, its ascender and descender with no
/// gap between lines, 1.3 em.
const LINE_EMS: f32 = 1.3;

/// How far each character of the overlay's text advances, in ems: every glyph of IBM Plex Mono is 600 of its 1,000
/// units wide.
const ADVANCE_EMS: f32 = 0.6;

/// How wide the console's cursor is, in logical pixels.
const CURSOR_PIXELS: f32 = 2.0;

/// How wide the console's scroll bar is, in logical pixels, how far in from the panel's right edge, and the space
/// between it and the console's text, so the bar never lies over the text or crowds it.
const SCROLLBAR_PIXELS: f32 = 4.0;
const SCROLLBAR_INSET_PIXELS: f32 = 3.0;
const SCROLLBAR_GAP_PIXELS: f32 = 12.0;

/// How far one notch of the wheel turns, in logical pixels: a line, as the window capability takes a host's that
/// counts the wheel in lines (ADR-0024).
const NOTCH_PIXELS: f32 = 40.0;

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

    /// The console's scrollback strip: the console's panel above the prompt's strip.
    pub fn console_body(self) -> Rect2 {
        let console = self.console();

        Rect2 { min: console.min, max: Vec2::new(console.max.x, self.prompt_strip().min.y) }
    }

    /// The prompt's strip: the whole width of the console's panel, along its foot, a line of text tall and its
    /// padding above and below. No taller than the panel.
    pub fn prompt_strip(self) -> Rect2 {
        let console = self.console();
        let tall = self.pixels(STRIP_PADDING_PIXELS).mul_add(2.0, self.line_pixels_tall().ceil());
        let top = (console.max.y - tall).max(console.min.y);

        Rect2 { min: Vec2::new(console.min.x, top), max: console.max }
    }

    /// The line between the band and the console: along the top of the console's panel, inside it.
    pub fn band_divider(self) -> Rect2 {
        let console = self.console();
        let bottom = (console.min.y + self.line_pixels(DIVIDER_PIXELS)).min(console.max.y);

        Rect2 { min: console.min, max: Vec2::new(console.max.x, bottom) }
    }

    /// The line between the scrollback and the prompt's strip: along the top of the strip, inside it.
    pub fn prompt_divider(self) -> Rect2 {
        let strip = self.prompt_strip();
        let bottom = (strip.min.y + self.line_pixels(DIVIDER_PIXELS)).min(strip.max.y);

        Rect2 { min: strip.min, max: Vec2::new(strip.max.x, bottom) }
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

    /// The console's panel inside its padding, its foot the prompt's strip's padding, and left of its scroll bar by a
    /// gap: what its texts are clipped to.
    pub fn console_inside(self) -> Rect2 {
        let padding = self.pixels(PADDING_PIXELS);
        let console = self.console();
        let right = self.scrollbar_left() - self.pixels(SCROLLBAR_GAP_PIXELS);
        let bottom = console.max.y - self.pixels(STRIP_PADDING_PIXELS);
        let max = Vec2::new(right.max(padding), bottom.max(console.min.y + padding));

        Rect2 { min: Vec2::new(padding, console.min.y + padding), max }
    }

    /// How many characters wide the console's rows are: as many of IBM Plex Mono's as fit inside its panel, at least
    /// one.
    #[must_use]
    pub fn console_columns(self) -> usize {
        let fit = (self.console_inside().width() / self.advance()).floor();
        // A count of characters across a window is some hundreds, which a usize holds; a negative width fits none.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let columns = fit.max(1.0) as usize;

        columns
    }

    /// Where the prompt's line is placed: the top left of the line inside the prompt's strip, below its padding.
    pub fn prompt_at(self) -> Vec2 {
        let inside = self.console_inside();
        let top = self.prompt_strip().min.y + self.pixels(STRIP_PADDING_PIXELS);

        Vec2::new(inside.min.x, top.max(inside.min.y))
    }

    /// Where the line of what is running, `characters` wide, is placed: on the prompt's line, ending at the right of
    /// the console's text.
    pub fn running_at(self, characters: usize) -> Vec2 {
        let inside = self.console_inside();
        // A count of characters is some tens, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let width = characters as f32 * self.advance();

        Vec2::new((inside.max.x - width).max(inside.min.x), self.prompt_at().y)
    }

    /// Where the scrollback is placed: the top left inside the console's panel.
    pub fn scrollback_at(self) -> Vec2 {
        self.console_inside().min
    }

    /// How many rows of the scrollback are in view: as many lines as fit between the panel's top and a gap above the
    /// prompt's strip, at least one.
    #[must_use]
    pub fn scrollback_rows(self) -> usize {
        let room = self.prompt_strip().min.y - self.pixels(GAP_PIXELS) - self.scrollback_at().y;
        let fit = (room / self.line_pixels_tall()).floor();
        // A count of lines down a window is some tens, which a usize holds; a negative room fits none.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = fit.max(1.0) as usize;

        rows
    }

    /// The track of the console's scroll bar: a strip down the panel's right edge, inside its padding, beside the
    /// scrollback's rows from the top of the first to the foot of the last.
    pub fn scrollbar(self) -> Rect2 {
        let right = self.console().max.x - self.pixels(SCROLLBAR_INSET_PIXELS);
        let left = self.scrollbar_left();
        let top = self.scrollback_at().y;
        // A count of lines down a window is some tens, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let bottom = (self.scrollback_rows() as f32).mul_add(self.line_pixels_tall(), top);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, bottom) }
    }

    /// The left edge of the console's scroll bar's track, in the surface's pixels.
    fn scrollbar_left(self) -> f32 {
        self.console().max.x - self.pixels(SCROLLBAR_INSET_PIXELS) - self.line_pixels(SCROLLBAR_PIXELS)
    }

    /// How far one notch of the wheel turns, in the surface's pixels, as its turns are counted.
    #[must_use]
    pub fn notch_pixels(self) -> f32 {
        NOTCH_PIXELS * self.scale
    }

    /// Where the count of warnings and errors `characters` wide is placed: on the figures' line, ending at the band's
    /// right padding.
    pub fn alerts_at(self, characters: usize) -> Vec2 {
        let padding = self.pixels(PADDING_PIXELS);
        // A count of characters is some tens, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let width = characters as f32 * self.advance();

        Vec2::new((self.band().max.x - padding - width).max(padding), self.figures_at().y)
    }

    /// The console's cursor before the character `column` of the prompt's line: a bar the height of the line.
    pub fn cursor(self, column: usize) -> Rect2 {
        let at = self.prompt_at();
        // A column is some hundreds of characters at most, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let left = (column as f32).mul_add(self.advance(), at.x).round();
        let width = self.line_pixels(CURSOR_PIXELS);

        Rect2 { min: Vec2::new(left, at.y), max: Vec2::new(left + width, at.y + self.line_pixels_tall()) }
    }

    /// How far each character of the overlay's text advances, in the surface's pixels.
    #[must_use]
    pub fn advance(self) -> f32 {
        self.text_pixels() * ADVANCE_EMS
    }

    /// How tall one line of the overlay's text is, in the surface's pixels.
    #[must_use]
    pub fn line_pixels_tall(self) -> f32 {
        self.text_pixels() * LINE_EMS
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
