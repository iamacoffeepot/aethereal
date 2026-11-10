//! What the overlay draws as one placement (ADR-0023): a unit square, drawn once for each of its instances' records,
//! each a box in the surface's pixels and its colour. The records are the panels and the lines between them, the frame
//! chart's faint line at twice the budget, its columns, each three bars to its slowest, median, and fastest cycles
//! (`column`), on a scale logarithmic in a cycle's time and relative to the budget (`scale`), its dashed budget line,
//! the GPU's graph (`gpu`), the lines' rows (`lines`), the memory meters' bars (`meters`), the marks' ticks, the console's cursor, and its scroll bar, in
//! that order, as one placement's records are drawn in their order; all of them are written again in one write at
//! each `Tick` the overlay is shown, as the chart moves with time.

mod column;
mod scale;
#[cfg(test)]
pub mod tests;

use crate::colour::{BONE, LICHEN, OCHRE, PANEL_OPACITY, SLATE, SLATE_DARKER, SLATE_LIGHTER, STONE};
use crate::frames::{Column, Slide};
use crate::gpu::MOST_PASSES;
use crate::layout::Layout;
use crate::lines::LINES;
use crate::marks::MOST_MARKS;
use crate::meters::MOST_BAR_RECORDS;
use crate::scrollbar::{Bar, Standing};
use crate::stop::Stop;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::mesh::{Attribute, VertexFormat, VertexLayout};
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Colour;
use column::{Placed, RECORDS_PER_COLUMN};
use scale::height_fraction;

/// The program that draws each record: the unit square's corner placed in the record's box, turned from the surface's
/// pixels, from its top left, into clip space by the surface's size in the uniform, and shaded the record's colour.
pub const SHADE: &str = "
@group(0) @binding(0) var<uniform> surface: vec4<f32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>, @location(1) area: vec4<f32>, @location(2) colour: vec4<f32>) -> Between {
    let pixel = area.xy + corner * area.zw;
    let clip = vec2(pixel.x / surface.x * 2.0 - 1.0, 1.0 - pixel.y / surface.y * 2.0);
    return Between(vec4(clip, 0.0, 1.0), colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// How many bytes a record takes: a box of four floats and a colour of four.
pub const RECORD_BYTES: u32 = 32;

/// The most records the instances have room for: the panels and the lines between them, the line at twice the budget,
/// the most columns and the two sliding off the left edge each in a column's records of the frame chart, of the GPU's
/// graph, and of the lines' rows, the most dashes of the two budget lines, the meters' bars, the most marks, the
/// cursor, and the scroll bar's track and thumb.
pub const MOST_RECORDS: u32 =
    PANEL_RECORDS + 1 + (MOST_COLUMNS + 2) * RECORDS_ACROSS + 2 * MOST_DASHES + MOST_BAR_RECORDS + MOST_MARKS + 1 + 2;

/// How many records one column takes down the band's left column at most: the frame chart's, a segment for each pass
/// of the GPU's graph, and a bar in each line's row.
const RECORDS_ACROSS: u32 = RECORDS_PER_COLUMN + MOST_PASSES + LINES;

/// How many records the panels take at most: the band's, the scrollback's, and the prompt's strip, and the lines
/// between the band and the console and above the prompt's strip.
const PANEL_RECORDS: u32 = 5;

/// The most columns the chart is cut into, whatever the window's width.
const MOST_COLUMNS: u32 = 3_000;

/// The most dashes the budget line is drawn in, whatever the window's width.
const MOST_DASHES: u32 = 1_000;

/// How long each dash of a budget line that is a limit is, and the gap after it, in logical pixels.
const DASH_PIXELS: f32 = 6.0;
const DASH_GAP_PIXELS: f32 = 4.0;

/// How long each dash of a budget line that is a reference is, and the gap after it, in logical pixels: a lighter
/// dash, so a budget the frames are not held to does not read as one they are (ADR-0053).
const REFERENCE_DASH_PIXELS: f32 = 2.0;
const REFERENCE_GAP_PIXELS: f32 = 6.0;

/// How opaque the budget line is drawn, in bone: bright, as the bars' height against it is what says over or under.
const BUDGET_OPACITY: f32 = 0.7;

/// How much fainter than a limit a budget line that is a reference is drawn.
const REFERENCE_OPACITY: f32 = 0.5;

/// How thick the budget line and the line at twice it are, in logical pixels.
const LINE_PIXELS: f32 = 1.0;

/// How wide a mark's tick is, in logical pixels.
const TICK_PIXELS: f32 = 2.0;

/// How opaque the line at twice the budget is drawn, in stone: faint, a guide to a doubling over the budget, as
/// the figures beneath the chart carry the numbers.
const TWICE_BUDGET_OPACITY: f32 = 0.15;

/// How opaque the lines between the panels are drawn, in stone: faint, so they part the panels without drawing the
/// eye.
const DIVIDER_OPACITY: f32 = 0.28;

/// How opaque the scroll bar's track is drawn, in stone.
const TRACK_OPACITY: f32 = 0.2;

/// How opaque the scroll bar's thumb is drawn in stone, on the newest rows; scrolled up from them it is lichen.
const THUMB_OPACITY: f32 = 0.6;

/// How the square's corners are laid out: a place of two floats.
#[must_use]
pub fn corner_layout() -> VertexLayout {
    VertexLayout { stride: 8, attributes: vec![Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 }] }
}

/// The unit square's corners, from its top left, clockwise on screen.
#[must_use]
pub fn corners() -> Vec<u8> {
    [0.0_f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0].iter().flat_map(|corner| corner.to_ne_bytes()).collect()
}

/// The square's two triangles, indexing its corners.
#[must_use]
pub fn triangles() -> Vec<u8> {
    [0_u32, 1, 2, 0, 2, 3].iter().flat_map(|index| index.to_ne_bytes()).collect()
}

/// How a record is laid out: its box, its left, top, width, and height, then its colour.
#[must_use]
pub fn record_layout() -> VertexLayout {
    let area = Attribute { location: 1, format: VertexFormat::Float32x4, offset: 0 };
    let colour = Attribute { location: 2, format: VertexFormat::Float32x4, offset: 16 };

    VertexLayout { stride: RECORD_BYTES, attributes: vec![area, colour] }
}

/// The uniform the program reads: the surface's width and height, then two floats it does not read.
#[must_use]
pub fn surface(layout: Layout) -> Vec<u8> {
    let size = layout.size();

    [size.x, size.y, 0.0, 0.0].iter().flat_map(|value| value.to_ne_bytes()).collect()
}

/// How many columns the chart is cut into on `layout`: as many as fit its width, at most [`MOST_COLUMNS`].
#[must_use]
pub fn column_count(layout: Layout) -> u32 {
    let fit = (layout.chart().width() / layout.column_pitch()).floor();
    // A count of columns is at most some thousands, which a u32 holds; a negative width fits none.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = fit.max(0.0) as u32;

    count.min(MOST_COLUMNS)
}

/// How many bytes the records of [`records`], the GPU's graph, the lines' rows, the meters' bars, [`marks`], [`cursor`],
/// and [`scrollbar`] take on `layout` at most: the panels and the lines between them, the line at twice the budget, a
/// column's records down the left column for each column the chart is cut into and the two sliding off its left edge,
/// the dashes of the two budget lines, the meters' bars, the most marks, the cursor, and the scroll bar's track and thumb.
#[must_use]
pub fn records_bytes(layout: Layout) -> usize {
    // A reference's dashes are the closer, so the more.
    let pitch = layout.line_pixels(REFERENCE_DASH_PIXELS.min(DASH_PIXELS))
        + layout.line_pixels(REFERENCE_GAP_PIXELS.min(DASH_GAP_PIXELS));
    let dashes = (layout.chart().width() / pitch).ceil();
    // A count of dashes is at most some thousands, which a u32 holds; a negative width holds none.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let dashes = (dashes.max(0.0) as u32).min(MOST_DASHES);
    let count = PANEL_RECORDS
        + 1
        + (column_count(layout) + 2) * RECORDS_ACROSS
        + 2 * dashes
        + MOST_BAR_RECORDS
        + MOST_MARKS
        + 1
        + 2;

    count as usize * RECORD_BYTES as usize
}

/// Appends to `records` what the overlay draws at `stop` on `layout`: the band's panel; when the stop shows the
/// console, its scrollback's panel a step lighter and the prompt's strip a step darker, with a faint stone line along
/// the top of each; a faint stone line at twice `budget`; each of `columns`, scaled against it; and the dashed budget
/// line, bone, drawn lighter when the budget is a reference. A record is appended as [`record_layout`]
/// lays it out.
///
/// The newest column's span is at the chart's right edge, and each older one a column's pitch further left, all moved
/// left by the share of a pitch `slide` has come through the newest span, so the chart slides smoothly left as the
/// program's time passes: the newest column grows out of the right edge, and the oldest is cut at the left edge as it
/// slides off.
pub fn records(
    records: &mut Vec<u8>,
    layout: Layout,
    stop: Stop,
    columns: impl IntoIterator<Item = Column>,
    slide: Slide,
    budget: FrameBudget,
) {
    let budget_nanos = budget.nanos();
    push(records, layout.band(), SLATE.at(PANEL_OPACITY));
    if stop.console() {
        let divider = STONE.at(DIVIDER_OPACITY);
        push(records, layout.console_body(), SLATE_LIGHTER.at(PANEL_OPACITY));
        push(records, layout.prompt_strip(), SLATE_DARKER.at(PANEL_OPACITY));
        push(records, layout.band_divider(), divider);
        push(records, layout.prompt_divider(), divider);
    }

    let chart = layout.chart();
    let (width, line) = (layout.column_pixels(), layout.line_pixels(LINE_PIXELS));
    let height_of = |nanos: u64| height_fraction(nanos, budget_nanos).mul_add(-chart.height(), chart.max.y).round();
    let twice_y = height_of(budget_nanos.saturating_mul(2));
    let twice = Rect2 { min: Vec2::new(chart.min.x, twice_y), max: Vec2::new(chart.max.x, twice_y + line) };
    push(records, twice, STONE.at(TWICE_BUDGET_OPACITY));

    for column in columns {
        let left = left_of(layout, slide, column.span);
        let shows = left.max(chart.min.x) < (left + width).min(chart.max.x);
        if !shows {
            continue;
        }
        let placed = Placed { left, width, chart, budget_nanos };
        column::column(records, placed, column);
    }

    dashes(records, layout, height_of(budget_nanos), budget);
}

/// Where the left edge of the column of `span` is drawn at `slide`, before it is cut to the chart: the newest's span
/// at the chart's right edge, each older one a column's pitch further left, all moved left by the share of a pitch
/// `slide` has come through the newest span. The GPU's graph and the lines' rows place their columns here too, so
/// each stands under the frame chart's column of the same span.
pub fn left_of(layout: Layout, slide: Slide, span: u64) -> f32 {
    // Columns back from the newest, at most a few thousand, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let back = slide.newest.saturating_sub(span) as f32;

    (back + slide.through).mul_add(-layout.column_pitch(), layout.chart().max.x)
}

/// Appends to `records` a budget line at `y` across the chart's pixels, dashed: bone at 70% for a limit, and a lighter,
/// sparser dash at half that for a budget that is a reference.
pub fn dashes(records: &mut Vec<u8>, layout: Layout, y: f32, budget: FrameBudget) {
    let chart = layout.chart();
    let line = layout.line_pixels(LINE_PIXELS);
    let (dash, gap, colour) = match budget {
        FrameBudget::Limit { .. } => (DASH_PIXELS, DASH_GAP_PIXELS, BONE.at(BUDGET_OPACITY)),
        FrameBudget::Reference { .. } => {
            (REFERENCE_DASH_PIXELS, REFERENCE_GAP_PIXELS, BONE.at(BUDGET_OPACITY * REFERENCE_OPACITY))
        }
    };
    let (dash, gap) = (layout.line_pixels(dash), layout.line_pixels(gap));
    let mut left = chart.min.x;
    for _ in 0..MOST_DASHES {
        let ends = left >= chart.max.x;
        if ends {
            break;
        }
        let right = (left + dash).min(chart.max.x);
        let piece = Rect2 { min: Vec2::new(left, y), max: Vec2::new(right, y + line) };
        push(records, piece, colour);
        left = right + gap;
    }
}

/// Appends to `records` a tick for each mark of `times`, the program's time it was pinned at: an ochre line the
/// chart's height, where that moment falls among the columns placed at `slide`, sliding left with them. A mark that
/// falls outside the chart is not drawn.
pub fn marks(records: &mut Vec<u8>, layout: Layout, slide: Slide, times: impl IntoIterator<Item = u64>) {
    let chart = layout.chart();
    let (pitch, width) = (layout.column_pitch(), layout.line_pixels(TICK_PIXELS));
    let span_nanos = slide.span_nanos.max(1);
    // Spans of the program's time, some thousands, and a share of one, which an f32 holds closely enough to draw.
    #[allow(clippy::cast_precision_loss)]
    let newest = slide.newest as f32 + slide.through;
    for at_nanos in times {
        #[allow(clippy::cast_precision_loss)]
        let at = (at_nanos / span_nanos) as f32 + (at_nanos % span_nanos) as f32 / span_nanos as f32;
        let left = (newest - at).mul_add(-pitch, chart.max.x);
        let within = left >= chart.min.x && left + width <= chart.max.x;
        if !within {
            continue;
        }
        let tick = Rect2 { min: Vec2::new(left, chart.min.y), max: Vec2::new(left + width, chart.max.y) };
        push(records, tick, OCHRE.opaque());
    }
}

/// Appends to `records` the console's cursor, a lichen bar the height of a line, before the character `column` of
/// the prompt's row.
pub fn cursor(records: &mut Vec<u8>, layout: Layout, column: usize) {
    push(records, layout.cursor(column), LICHEN.opaque());
}

/// Appends to `records` the console's scroll bar, unless it is hidden: its track in faint stone, and its thumb over it,
/// stone on the newest rows and lichen scrolled up from them, as newer rows are below.
pub fn scrollbar(records: &mut Vec<u8>, bar: Bar) {
    let Bar::Shown { track, thumb, standing } = bar else {
        return;
    };
    let colour = match standing {
        Standing::Newest => STONE.at(THUMB_OPACITY),
        Standing::Scrolled => LICHEN.opaque(),
    };
    push(records, track, STONE.at(TRACK_OPACITY));
    push(records, thumb, colour);
}

/// Appends the record of `area` drawn in `colour`.
pub fn push(records: &mut Vec<u8>, area: Rect2, colour: Colour) {
    let [red, green, blue, alpha] = colour.channels();
    let values = [area.min.x, area.min.y, area.width(), area.height(), red, green, blue, alpha];
    records.extend(values.iter().flat_map(|value| value.to_ne_bytes()));
}
