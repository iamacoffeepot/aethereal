//! How big the overlay draws (ADR-0053: The three stops). The widget library lays the band and the console out from the
//! overlay's styles (`overlay::view`), which name the sizes kept here; what this module answers is what the overlay's
//! own drawing and its view need of them:
//!
//! - [`Scale`]: how many of the window's pixels make a logical one, and so how wide a chart's column is drawn, how far
//!   apart columns stand, how many fit a box the library laid out, and how thick a line is, as the primitives draw into
//!   the boxes the library gave them;
//! - [`Layout`]: the window's size, from which the view works out how tall the frame chart and the GPU's graph stand.
//!
//! How wide the charts and the meters' graphs are, and so how many columns they are cut into, and how many rows the
//! scrollback holds, are read from the boxes the library last laid them out in (`Screen::laid`), never worked out here
//! again from the font's metrics or the library's own sizes.
//!
//! ```text
//! the band:    padding │ left column, 60%: chart │ gutter          │ rule │ right column: meter graph │ gutter │ padding
//! the console: a third of the window, or as dragged: grip, scrollback or transcript, asks, rule, prompt, tabs
//! ```

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

/// The space after an owner's line's tag and after the prompt's mark, and between a meter's graph and its gutter, in
/// logical pixels: about a character of the overlay's text, so what follows reads as the next word.
pub const WORD_GAP_PIXELS: f32 = 8.0;

/// How far a meter's graph or bar stands in from its row's top and foot, in logical pixels, so the rows read apart.
pub const METER_INSET_PIXELS: f32 = 6.0;

/// The space between one section of the band's left column and the next, and between the graphs and the gutter, in
/// logical pixels.
pub const SECTION_PIXELS: f32 = 8.0;

/// How far a row's bars stand in from its top and foot, in logical pixels, so the rows read apart.
const ROW_INSET_PIXELS: f32 = 3.0;

/// The share of the window's height the console's panel takes, beneath the band, until its grip is dragged.
pub const CONSOLE_SHARE: f32 = 1.0 / 3.0;

/// The least and the most share of the window's height the console's panel is dragged to.
pub const LEAST_CONSOLE_SHARE: f32 = 0.15;
pub const MOST_CONSOLE_SHARE: f32 = 0.85;

/// How tall the console's grip is, the seam between the band and the console, in logical pixels: its shadow along its
/// top, its handle in its middle, and room around them to take hold of.
pub const GRIP_PIXELS: f32 = 7.0;

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

/// How thick the lines between the scrollback and the prompt's strip, between the prompt's strip and the tabs', between
/// the band's two columns, between the left column's sections, and between the tabs are, and the grip's shadow, in
/// logical pixels.
pub const DIVIDER_PIXELS: f32 = 1.0;

/// How wide the grip's handle is, centred across it, in logical pixels: a mark to take hold of, while the whole grip
/// takes the drag.
pub const HANDLE_WIDTH_PIXELS: f32 = 28.0;

/// How tall the grip's handle is, in logical pixels.
pub const HANDLE_PIXELS: f32 = 3.0;

/// The space above and below a tab's title inside the tab, in logical pixels; [`TAB_SIDE_PIXELS`] stand either side.
/// The tabs fill their strip's height, which has no padding of its own, so this is the strip's space above and below
/// too.
pub const TAB_PADDING_PIXELS: f32 = 7.0;

/// The space either side of a tab's title, and of its status icon before it, inside the tab, in logical pixels: room
/// enough that each tab reads as a button of its own.
pub const TAB_SIDE_PIXELS: f32 = 18.0;

/// The space between two tabs, and either side of the line that parts them, in logical pixels.
pub const TAB_GAP_PIXELS: f32 = 4.0;

/// How wide and tall the slot of a session tab's status icon is, in logical pixels: fixed, whatever the icon, so the
/// title after it never moves as the session's status changes.
pub const TAB_ICON_PIXELS: f32 = 8.0;

/// The space between a session tab's status icon and its title, in logical pixels.
pub const TAB_ICON_GAP_PIXELS: f32 = 7.0;

/// How far in from either side of the open tab the line along its bottom edge stops, in logical pixels, so the lines of
/// two tabs side by side never read as one.
pub const TAB_ACCENT_INSET_PIXELS: f32 = 10.0;

/// How thick the line along the open tab's bottom edge is, in logical pixels: over the tab's own space beneath its
/// title, so an open tab stands as tall as the others.
pub const TAB_ACCENT_PIXELS: f32 = 2.0;

/// How tall the divider between two sessions' tabs is, in logical pixels: shorter than the strip, so it stands in from
/// its top and its foot.
pub const TAB_DIVIDER_PIXELS: f32 = 12.0;

/// How tall the break between the console's own tab and the sessions' is, in logical pixels: taller than a divider
/// between sessions, with [`PADDING_PIXELS`] of space either side, so the engine's tab stands apart from Claude's.
pub const TAB_BREAK_PIXELS: f32 = 18.0;

/// The size of the overlay's text, in logical pixels to the em: what reads on a window 1080 pixels tall.
pub const TEXT_PIXELS: f32 = 14.0;

/// How wide each of the chart's columns is drawn, in logical pixels, and the gap after it: at 1080, 226 columns of
/// about a quarter of a second each.
const COLUMN_PIXELS: f32 = 4.0;
const COLUMN_GAP_PIXELS: f32 = 1.0;

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

/// The window the overlay lays itself out on: its height in its pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    tall: f32,
}

impl Layout {
    /// The layout on `viewport`, the screen's size.
    #[must_use]
    pub const fn of(viewport: Viewport) -> Self {
        Self { tall: viewport.size.y }
    }

    /// How tall the frame chart stands, in the window's pixels: a share of the window's height. Its row stretches it to
    /// the two lines of the gutter beside it where they stand taller, on a screen of many pixels to the logical one.
    #[must_use]
    pub fn chart_pixels_tall(self) -> f32 {
        (self.tall * CHART_SHARE).round()
    }

    /// How tall the GPU's graph stands, in the window's pixels: two thirds of the frame chart, and at least a pixel.
    #[must_use]
    pub fn gpu_pixels_tall(self) -> f32 {
        (self.chart_pixels_tall() * GPU_SHARE).round().max(1.0)
    }
}
