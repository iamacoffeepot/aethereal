//! How big the overlay draws (ADR-0053: The three stops). The widget library lays the band and the console out from the
//! overlay's styles (`overlay::view`), which name the sizes kept here; what this module answers is what the overlay's
//! own drawing and its view need of them:
//!
//! - [`Scale`]: how many of the window's pixels make a logical one, and so how wide a chart's column is drawn, how far
//!   apart columns stand, and how thick a line is, as the primitives draw into the boxes the library gave them;
//! - [`Layout`]: the window's size, from which the view works out what depends on it and not on the tree: how tall the
//!   frame chart and the GPU's graph stand, how many columns the charts and the meters' graphs are cut into, and how
//!   many characters across and rows down the console holds, by the same sizes the styles lay them out with.
//!
//! ```text
//! the band:    padding │ left column, 60%: chart │ gutter          │ rule │ right column: meter graph │ gutter │ padding
//! the console: a third of the window: rule, scrollback (its bar at its right edge), rule, the prompt's strip
//! ```

use aethereal_math::Vec2;
use aethereal_ui::Viewport;

/// The share of the window's height the frame chart takes: at 1080, 65 pixels.
const CHART_SHARE: f32 = 0.06;

/// How tall the GPU's graph is, as a share of the frame chart's height.
const GPU_SHARE: f32 = 2.0 / 3.0;

/// How many characters wide the gutter at the right of each column's graphs is: a row's label and its latest value,
/// `triangles   12.3M`, `process  412.0 MiB`.
pub const GUTTER_CHARACTERS: usize = 20;

/// How many characters of a gutter's line its label is given, its value the rest, flush right.
pub const GUTTER_LABEL_CHARACTERS: usize = 10;

/// How many characters' space stand between a meter's graph or bar and the right column's gutter.
pub const METER_GAP_CHARACTERS: usize = 1;

/// How far a meter's graph or bar stands in from its row's top and foot, in logical pixels, so the rows read apart.
pub const METER_INSET_PIXELS: f32 = 6.0;

/// The space between one section of the band's left column and the next, and between the graphs and the gutter, in
/// logical pixels.
pub const SECTION_PIXELS: f32 = 8.0;

/// How far a row's bars stand in from its top and foot, in logical pixels, so the rows read apart.
const ROW_INSET_PIXELS: f32 = 3.0;

/// The share of the window's height the console's panel takes, beneath the band.
pub const CONSOLE_SHARE: f32 = 1.0 / 3.0;

/// The share of the window's width the band's left column takes: the chart's column. The right column, the memory
/// meters', takes the rest.
pub const LEFT_SHARE: f32 = 0.6;

/// The space between the band's edges and what it holds, and inside the console's scrollback, in logical pixels.
pub const PADDING_PIXELS: f32 = 10.0;

/// The space between a graph and its figures, and between the scrollback's last row and the prompt's strip, in
/// logical pixels.
pub const GAP_PIXELS: f32 = 4.0;

/// The space above and below the prompt's line inside its strip, in logical pixels.
pub const STRIP_PADDING_PIXELS: f32 = 6.0;

/// How thick the lines between the band and the console, between the scrollback and the prompt's strip, between the
/// band's two columns, and between the left column's sections are, in logical pixels.
pub const DIVIDER_PIXELS: f32 = 1.0;

/// The size of the overlay's text, in logical pixels to the em: what reads on a window 1080 pixels tall.
pub const TEXT_PIXELS: f32 = 14.0;

/// How tall one line of the overlay's text is, in ems: IBM Plex Mono's line height, its ascender and descender with no
/// gap between lines, 1.3 em.
const LINE_EMS: f32 = 1.3;

/// How far each character of the overlay's text advances, in ems: every glyph of IBM Plex Mono is 600 of its 1,000
/// units wide.
const ADVANCE_EMS: f32 = 0.6;

/// How wide the library's scroll keeps its bar, in logical pixels: the console's rows end before it.
const SCROLL_BAR_PIXELS: f32 = 8.0;

/// How wide each of the chart's columns is drawn, in logical pixels, and the gap after it: at 1080, 226 columns of
/// about a quarter of a second each.
const COLUMN_PIXELS: f32 = 4.0;
const COLUMN_GAP_PIXELS: f32 = 1.0;

/// How many characters wide a gutter beside the left column's graphs is, in logical pixels: its characters and the
/// section's space before them.
#[must_use]
pub fn gutter_pixels() -> f32 {
    characters(GUTTER_CHARACTERS).mul_add(advance_pixels(), SECTION_PIXELS)
}

/// How wide the gutter of a meter's row is, in logical pixels: its characters and the character's space before them.
#[must_use]
pub fn meter_gutter_pixels() -> f32 {
    characters(GUTTER_CHARACTERS + METER_GAP_CHARACTERS) * advance_pixels()
}

/// How wide one character's space is, in logical pixels.
#[must_use]
pub fn advance_pixels() -> f32 {
    TEXT_PIXELS * ADVANCE_EMS
}

/// How tall one line of the overlay's text is, in logical pixels.
fn line_logical() -> f32 {
    TEXT_PIXELS * LINE_EMS
}

/// `count` characters, as a length is counted: some tens, which an f32 holds exactly.
fn characters(count: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let count = count as f32;

    count
}

/// How many of the window's pixels make a logical one, as the screen's UI scale says: what the overlay's own drawing
/// sizes its columns, bars, and lines by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale(pub f32);

impl Scale {
    /// `logical` logical pixels, in the window's, rounded to a whole one.
    fn pixels(self, logical: f32) -> f32 {
        (logical * self.0).round()
    }

    /// How wide each of the chart's columns is, in the window's pixels: at least one.
    #[must_use]
    pub fn column_pixels(self) -> f32 {
        self.pixels(COLUMN_PIXELS).max(1.0)
    }

    /// How far one column is from the next, in the window's pixels: its width and the gap after it, at least one.
    #[must_use]
    pub fn column_pitch(self) -> f32 {
        self.column_pixels() + self.pixels(COLUMN_GAP_PIXELS).max(1.0)
    }

    /// `logical` logical pixels, in the window's, at least one: how thick a line is drawn.
    #[must_use]
    pub fn line_pixels(self, logical: f32) -> f32 {
        self.pixels(logical).max(1.0)
    }

    /// How far a row's bars stand in from its top and foot, in the window's pixels, in a row `tall` pixels: at most
    /// what leaves it a pixel.
    #[must_use]
    pub fn row_inset(self, tall: f32) -> f32 {
        self.pixels(ROW_INSET_PIXELS).min((tall - 1.0) / 2.0).max(0.0)
    }

    /// How tall one line of the overlay's text is, in the window's pixels.
    #[must_use]
    pub fn line_pixels_tall(self) -> f32 {
        line_logical() * self.0
    }

    /// How far each character of the overlay's text advances, in the window's pixels.
    #[must_use]
    pub fn advance(self) -> f32 {
        advance_pixels() * self.0
    }

    /// How many columns, a column's pitch apart, fit across `width` pixels, at most `most`.
    #[must_use]
    pub fn columns_across(self, width: f32, most: u32) -> u32 {
        let fit = (width / self.column_pitch()).floor();
        // A count of columns is at most some thousands, which a u32 holds; a negative width fits none.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let count = fit.max(0.0) as u32;

        count.min(most)
    }
}

/// The window the overlay lays itself out on: its width and height in its pixels, and its UI scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    size: Vec2,
    scale: Scale,
}

impl Layout {
    /// The layout on `viewport`, the screen's size.
    #[must_use]
    pub const fn of(viewport: Viewport) -> Self {
        Self { size: viewport.size, scale: Scale(viewport.scale) }
    }

    /// How tall the frame chart stands, in the window's pixels: a share of the window's height, and no less than the
    /// two lines of the gutter beside it, so they stand beside it on a screen of many pixels to the logical one.
    #[must_use]
    pub fn chart_pixels_tall(self) -> f32 {
        let gutter = (2.0 * self.scale.line_pixels_tall()).ceil();

        (self.size.y * CHART_SHARE).round().max(gutter)
    }

    /// How tall the GPU's graph stands, in the window's pixels: two thirds of the frame chart, and at least a pixel.
    #[must_use]
    pub fn gpu_pixels_tall(self) -> f32 {
        (self.chart_pixels_tall() * GPU_SHARE).round().max(1.0)
    }

    /// How wide the frame chart, and the graphs beneath it, are, in the window's pixels: the left column's share of the
    /// window, inside its padding, less its gutter.
    fn chart_width(self) -> f32 {
        let taken = PADDING_PIXELS.mul_add(2.0, gutter_pixels()) * self.scale.0;

        self.size.x.mul_add(LEFT_SHARE, -taken)
    }

    /// How many columns the charts of the left column are cut into: as many as fit their width, at most `most`.
    #[must_use]
    pub fn chart_columns(self, most: u32) -> u32 {
        self.scale.columns_across(self.chart_width(), most)
    }

    /// How wide a meter's graph is, in the window's pixels: the right column, the window's width less the left
    /// column and the line between them, inside its padding, less its gutter.
    fn meter_width(self) -> f32 {
        let taken = PADDING_PIXELS.mul_add(2.0, DIVIDER_PIXELS + meter_gutter_pixels()) * self.scale.0;

        self.size.x.mul_add(1.0 - LEFT_SHARE, -taken)
    }

    /// How many columns a meter's graph is cut into: as many as fit its width, at most `most`.
    #[must_use]
    pub fn meter_columns(self, most: u32) -> u32 {
        self.scale.columns_across(self.meter_width(), most)
    }

    /// How many characters wide the console's rows are: as many of IBM Plex Mono's as fit inside its scrollback's
    /// padding, left of its bar, at least one.
    #[must_use]
    pub fn console_columns(self) -> usize {
        let room = PADDING_PIXELS.mul_add(2.0, SCROLL_BAR_PIXELS).mul_add(-self.scale.0, self.size.x);
        let fit = (room / self.scale.advance()).floor();
        // A count of characters across a window is some hundreds, which a usize holds; a negative width fits none.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let columns = fit.max(1.0) as usize;

        columns
    }

    /// How many rows of the scrollback are in view: as many lines as fit the console's third of the window less its
    /// two lines, the prompt's strip, and the scrollback's padding above and gap below, at least one.
    #[must_use]
    pub fn scrollback_rows(self) -> usize {
        let line = self.scale.line_pixels_tall();
        let console = (self.size.y * CONSOLE_SHARE).round();
        let strip = STRIP_PADDING_PIXELS.mul_add(2.0, DIVIDER_PIXELS * 2.0).mul_add(self.scale.0, line);
        let padding = (PADDING_PIXELS + GAP_PIXELS) * self.scale.0;
        let fit = ((console - strip - padding) / line).floor();
        // A count of lines down a window is some tens, which a usize holds; a negative room fits none.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = fit.max(1.0) as usize;

        rows
    }
}
