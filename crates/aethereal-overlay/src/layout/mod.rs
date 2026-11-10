//! Where the overlay draws, worked out from the window's surface each time it is needed (ADR-0053: The three stops):
//! the band across the top, in two columns. Its left column holds, top to bottom on one axis of time, the frame chart
//! and its figures' line, the GPU's graph and its figures' line, and the three lines' rows, each graph ending at a
//! gutter on the column's right: beside the frame chart its latest median and, under it, the count of warnings and
//! errors; beside the GPU's graph its top; and beside each row its label and latest value. Its right column holds the
//! memory meters, a row each, the five sharing its height evenly from the frame chart's top to the left column's foot,
//! laid out as the left column's rows are: each its graph or bar from the column's left to a gutter at its right, every
//! graph between the same two edges, and in the gutter its label and its value, ending at the band's right padding. A
//! faint line parts the two columns, from the band's content's top to its foot, and two more part the left column's
//! three sections, across its graphs and their gutter. Beneath the band is the console's panel, a third of the
//! window's height, cut into two strips told apart by their tints and a faint line between: the scrollback above, its
//! scroll bar in the padding at its right edge, and the prompt's strip along the panel's foot, a line of text tall,
//! with what is running at its right end. All of it is in the surface's pixels, from its top left, as the renderer
//! places texts (ADR-0052).

#[cfg(test)]
mod tests;

use crate::meters::METERS;
use aethereal_math::{Rect2, Vec2};
use aethereal_window::Surface;

/// The share of the window's height the frame chart takes: at 1080, 65 pixels.
const CHART_SHARE: f32 = 0.06;

/// How tall the GPU's graph is, as a share of the frame chart's height.
const GPU_SHARE: f32 = 2.0 / 3.0;

/// How many rows the lines beneath the GPU's graph take: draws, triangles, and bytes uploaded.
const ROWS: u8 = 3;

/// How many characters wide the gutter at the right of each column's graphs is: a row's label and its latest value,
/// `triangles   12.3M`, `process  412.0 MiB`.
pub const GUTTER_CHARACTERS: usize = 20;

/// How many characters of a gutter's line its label is given, its value the rest, flush right.
pub const GUTTER_LABEL_CHARACTERS: usize = 10;

/// How many characters' space stand between a meter's graph or bar and the right column's gutter.
pub const METER_GAP_CHARACTERS: usize = 1;

/// How far a meter's graph or bar stands in from its row's top and foot, in logical pixels, so the rows read apart.
const METER_INSET_PIXELS: f32 = 6.0;

/// The space between one section of the band's left column and the next, and between the graphs and the gutter, in
/// logical pixels.
const SECTION_PIXELS: f32 = 8.0;

/// How far a row's bars stand in from its top and foot, in logical pixels, so the rows read apart.
const ROW_INSET_PIXELS: f32 = 3.0;

/// The share of the window's height the console's panel takes, beneath the band.
const CONSOLE_SHARE: f32 = 1.0 / 3.0;

/// The share of the window's width the band's left column takes: the chart's column. The right column, the memory
/// meters', takes the rest.
const LEFT_SHARE: f32 = 0.6;

/// The space between the band's edges and what it holds, in logical pixels.
const PADDING_PIXELS: f32 = 10.0;

/// The space between the chart and its figures, and between the scrollback's last row and the prompt's strip, in
/// logical pixels.
const GAP_PIXELS: f32 = 4.0;

/// The space above and below the prompt's line inside its strip, in logical pixels.
const STRIP_PADDING_PIXELS: f32 = 6.0;

/// How thick the lines between the band and the console, between the scrollback and the prompt's strip, between the
/// band's two columns, and between the left column's sections are, in logical pixels.
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

    /// The band: the whole width of the window, from its top down to its left column's last row and the padding
    /// beneath it.
    pub fn band(self) -> Rect2 {
        let bottom = (self.rows().max.y + self.pixels(PADDING_PIXELS)).round();

        Rect2 { min: Vec2::ZERO, max: Vec2::new(self.width, bottom) }
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

    /// The frame chart: the band's left column inside its padding, left of the gutter, from its top, a share of the
    /// window's height tall, and no less than the two lines of the gutter beside it, its median and the count of
    /// warnings and errors, so they stand beside it on a screen of many pixels to the logical one. At least a pixel
    /// wide. The GPU's graph and the
    /// rows beneath it span the same pixels across, so a column of each stands under the frame chart's.
    pub fn chart(self) -> Rect2 {
        let padding = self.pixels(PADDING_PIXELS);
        let right = (self.gutter_right() - self.gutter_pixels()).max(padding + 1.0);
        let gutter_lines = (2.0 * self.line_pixels_tall()).ceil();
        let bottom = padding + (self.height * CHART_SHARE).round().max(gutter_lines);

        Rect2 { min: Vec2::new(padding, padding), max: Vec2::new(right, bottom) }
    }

    /// The right edge of the gutter right of the graphs, where its lines end: the left column's end, inside its
    /// padding.
    fn gutter_right(self) -> f32 {
        (self.width * LEFT_SHARE).round() - self.pixels(PADDING_PIXELS)
    }

    /// The line between the band's two columns: from the frame chart's top to the rows' foot, the band's content,
    /// centred in the space between the gutter's right edge and the right column's left, so it parts the graphs and
    /// their gutter from the meters and touches neither.
    pub fn column_divider(self) -> Rect2 {
        let thick = self.line_pixels(DIVIDER_PIXELS);
        let left = ((self.gutter_right() + self.right_column().min.x - thick) / 2.0).round();
        let (top, bottom) = (self.chart().min.y, self.rows().max.y);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(left + thick, bottom) }
    }

    /// The lines between the left column's three sections, the frame chart and its figures, the GPU's graph and its
    /// figures, and the rows: across the column from the frame chart's left to the gutter's right edge, each centred
    /// in the section's space between one section's figures and the next section's top.
    pub fn section_dividers(self) -> [Rect2; 2] {
        let thick = self.line_pixels(DIVIDER_PIXELS);
        let (left, right) = (self.chart().min.x, self.gutter_right());
        let between = |foot: f32, top: f32| {
            let at = ((foot + top - thick) / 2.0).round();
            Rect2 { min: Vec2::new(left, at), max: Vec2::new(right, at + thick) }
        };
        let figures_foot = self.figures_at().y + self.figures_line_pixels();
        let gpu_figures_foot = self.gpu_figures_at().y + self.figures_line_pixels();

        [between(figures_foot, self.gpu_graph().min.y), between(gpu_figures_foot, self.rows().min.y)]
    }

    /// How wide the gutter right of the graphs is, in the surface's pixels: a section's space and the gutter's
    /// characters.
    fn gutter_pixels(self) -> f32 {
        // A count of characters is some tens, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let characters = GUTTER_CHARACTERS as f32;

        self.pixels(SECTION_PIXELS) + (characters * self.advance()).round()
    }

    /// How tall a line of the band's figures is given, in whole pixels.
    fn figures_line_pixels(self) -> f32 {
        self.line_pixels_tall().ceil()
    }

    /// Where the frame chart's figures are placed: the top left of their one line, a gap beneath the chart.
    pub fn figures_at(self) -> Vec2 {
        Vec2::new(self.pixels(PADDING_PIXELS), self.chart().max.y + self.pixels(GAP_PIXELS))
    }

    /// The GPU's graph: across the frame chart's pixels, a section's space beneath the frame chart's figures, two
    /// thirds of the frame chart's height and at least a pixel.
    pub fn gpu_graph(self) -> Rect2 {
        let chart = self.chart();
        let top = self.figures_at().y + self.figures_line_pixels() + self.pixels(SECTION_PIXELS);
        let tall = (chart.height() * GPU_SHARE).round().max(1.0);

        Rect2 { min: Vec2::new(chart.min.x, top), max: Vec2::new(chart.max.x, top + tall) }
    }

    /// The left of the gutter right of the graphs, where its lines are written: a section's space right of them.
    fn gutter_left(self) -> f32 {
        self.chart().max.x + self.pixels(SECTION_PIXELS)
    }

    /// Where the frame chart's latest median is written: the gutter beside the chart, level with its top.
    pub fn frame_median_at(self) -> Vec2 {
        Vec2::new(self.gutter_left(), self.chart().min.y)
    }

    /// Where the count of warnings and errors is written: the gutter beside the frame chart, the line beneath its
    /// latest median.
    pub fn alerts_at(self) -> Vec2 {
        let at = self.frame_median_at();

        Vec2::new(at.x, at.y + self.line_pixels_tall())
    }

    /// Where the GPU graph's top is written: the gutter beside the graph, level with its top.
    pub fn gpu_top_at(self) -> Vec2 {
        Vec2::new(self.gutter_left(), self.gpu_graph().min.y)
    }

    /// Where the GPU's figures are placed: the top left of their one line, a gap beneath the GPU's graph.
    pub fn gpu_figures_at(self) -> Vec2 {
        Vec2::new(self.pixels(PADDING_PIXELS), self.gpu_graph().max.y + self.pixels(GAP_PIXELS))
    }

    /// The lines' rows together: across the frame chart's pixels, a section's space beneath the GPU's figures, each
    /// row a line of text tall, so the text of their labels and values beside them lines up with them.
    pub fn rows(self) -> Rect2 {
        let chart = self.chart();
        let top = self.gpu_figures_at().y + self.figures_line_pixels() + self.pixels(SECTION_PIXELS);
        let tall = f32::from(ROWS) * self.line_pixels_tall();

        Rect2 { min: Vec2::new(chart.min.x, top), max: Vec2::new(chart.max.x, top + tall) }
    }

    /// Where the bars of the row `index`, from the top, stand: its line of the rows, inset from its top and foot, and
    /// at least a pixel tall.
    pub fn row(self, index: u8) -> Rect2 {
        let rows = self.rows();
        let tall = self.line_pixels_tall();
        let top = f32::from(index).mul_add(tall, rows.min.y);
        let inset = self.pixels(ROW_INSET_PIXELS).min((tall - 1.0) / 2.0).max(0.0);

        Rect2 { min: Vec2::new(rows.min.x, top + inset), max: Vec2::new(rows.max.x, top + tall - inset) }
    }

    /// Where the rows' labels and values are placed: the top left of the gutter beside the first row.
    pub fn lines_at(self) -> Vec2 {
        Vec2::new(self.gutter_left(), self.rows().min.y)
    }

    /// The band's right column, which the memory meters' rows share and their text is clipped to: from the frame
    /// chart's top, level with it, down to the foot of the left column's rows, and ending at the band's right padding;
    /// its left as near the left column's end, by its padding and the band's, as a whole number of characters across
    /// lets it stand, so a line of the column's characters ends at the band's right padding. At least a character wide
    /// and a pixel tall, on a window too small to hold it.
    pub fn right_column(self) -> Rect2 {
        let right = self.width - self.pixels(PADDING_PIXELS);
        let left = self.right_characters().mul_add(-self.advance(), right);
        let (top, bottom) = (self.chart().min.y, self.rows().max.y);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, bottom.max(top + 1.0)) }
    }

    /// How many characters fit across the room right of the left column, by its padding and the band's, to the band's
    /// right padding: at least one.
    fn right_characters(self) -> f32 {
        let padding = self.pixels(PADDING_PIXELS);
        let room = self.width - padding - ((self.width * LEFT_SHARE).round() + padding);

        (room / self.advance()).floor().max(1.0)
    }

    /// How many characters wide the right column is: as many of IBM Plex Mono's as fit across it, at least one; the
    /// tests fit a meter's row across it.
    #[cfg(test)]
    #[must_use]
    pub fn right_columns(self) -> usize {
        // A count of characters across a window is some hundreds, which a usize holds.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let columns = self.right_characters() as usize;

        columns
    }

    /// The row of the meter `index`, from the top: across the right column, a fifth of its height, the first's top the
    /// column's top and the last's foot its foot, so the meters fill the column with no space above, between, or
    /// below them.
    pub fn meter_row(self, index: u8) -> Rect2 {
        let column = self.right_column();
        // The count of meters, five, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let pitch = column.height() / METERS as f32;
        let top = f32::from(index).mul_add(pitch, column.min.y);
        let foot = (f32::from(index) + 1.0).mul_add(pitch, column.min.y).min(column.max.y);

        Rect2 { min: Vec2::new(column.min.x, top), max: Vec2::new(column.max.x, foot) }
    }

    /// The left of the right column's gutter, where the meters' lines are written: as many characters before the
    /// column's right edge, the band's right padding, as the gutter is wide, so each line ends there.
    fn meter_gutter_left(self) -> f32 {
        // A count of characters is some tens, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let characters = GUTTER_CHARACTERS as f32;

        characters.mul_add(-self.advance(), self.right_column().max.x)
    }

    /// Where the text of the meter `index` is placed: the top left of its one line, at the right column's gutter, the
    /// line centred down its row.
    pub fn meter_at(self, index: u8) -> Vec2 {
        let row = self.meter_row(index);
        let top = ((row.height() - self.line_pixels_tall()) / 2.0 + row.min.y).round();

        Vec2::new(self.meter_gutter_left(), top)
    }

    /// The graph or bar of the meter `index`: from the right column's left to a character's space before its gutter,
    /// the same for every meter, inset from its row's top and foot, and at least a pixel wide and tall.
    pub fn meter_graph(self, index: u8) -> Rect2 {
        let row = self.meter_row(index);
        // A count of characters, one, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let gap = METER_GAP_CHARACTERS as f32;
        let left = row.min.x.round();
        let right = gap.mul_add(-self.advance(), self.meter_gutter_left()).round().max(left + 1.0);
        let inset = self.pixels(METER_INSET_PIXELS).min((row.height() - 1.0) / 2.0).max(0.0);
        let (top, foot) = ((row.min.y + inset).round(), (row.max.y - inset).round());

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, foot.max(top + 1.0)) }
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
