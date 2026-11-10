//! What the overlay draws as one placement (ADR-0023): a unit square, drawn once for each of its instances' records,
//! each a box in the surface's pixels and its colour. The records are the panels, the frame chart's columns, and its
//! dashed budget line, in that order, as one placement's records are drawn in their order; all of them are written
//! again in one write at each `Tick` the overlay is shown, as the chart moves with time.

#[cfg(test)]
mod tests;

use crate::colour::{BRICK, LICHEN, PANEL_OPACITY, RUST, SLATE, STONE, Swatch};
use crate::frames::{Column, Slide};
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::mesh::{Attribute, VertexFormat, VertexLayout};
use aethereal_render::text::Colour;

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

/// The most records the instances have room for: the two panels, the most columns and the two sliding off the left
/// edge, and the most dashes.
pub const MOST_RECORDS: u32 = 2 + MOST_COLUMNS + 2 + MOST_DASHES;

/// The most columns the chart is cut into, whatever the window's width.
const MOST_COLUMNS: u32 = 3_000;

/// The most dashes the budget line is drawn in, whatever the window's width.
const MOST_DASHES: u32 = 1_000;

/// How many budgets the chart's height stands for: a frame of three budgets reaches its top, and a longer one is
/// drawn to it.
const BUDGETS_TALL: f32 = 3.0;

/// How long each dash of the budget line is, and the gap after it, in logical pixels.
const DASH_PIXELS: f32 = 6.0;
const DASH_GAP_PIXELS: f32 = 4.0;

/// How thick the budget line is, in logical pixels.
const LINE_PIXELS: f32 = 1.0;

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

/// How many bytes the records of [`records`] take on `layout` at most: the two panels, a column for each the chart is
/// cut into and the two sliding off its left edge, and the dashes of the budget line.
#[must_use]
pub fn records_bytes(layout: Layout) -> usize {
    let dashes =
        (layout.chart().width() / (layout.line_pixels(DASH_PIXELS) + layout.line_pixels(DASH_GAP_PIXELS))).ceil();
    // A count of dashes is at most some thousands, which a u32 holds; a negative width holds none.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let dashes = (dashes.max(0.0) as u32).min(MOST_DASHES);
    let count = 2 + column_count(layout) + 2 + dashes;

    count as usize * RECORD_BYTES as usize
}

/// Appends to `records` what the overlay draws at `stop` on `layout`: the band's panel, the console's beneath it when
/// the stop shows it, each of `columns` from the chart's foot, shaded by its longest cycle against `budget_nanos`, and
/// the dashed budget line. A record is appended as [`record_layout`] lays it out.
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
    budget_nanos: u64,
) {
    let panel = SLATE.at(PANEL_OPACITY);
    push(records, layout.band(), panel);
    if stop.console() {
        push(records, layout.console(), panel);
    }

    let chart = layout.chart();
    let (column_pixels, pitch) = (layout.column_pixels(), layout.column_pitch());
    // Nanoseconds of a few seconds, which an f32 holds closely enough to draw.
    #[allow(clippy::cast_precision_loss)]
    let tallest_nanos = budget_nanos as f32 * BUDGETS_TALL;
    for column in columns {
        // Columns back from the newest, at most a few thousand, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let (back, longest) = (slide.newest.saturating_sub(column.span) as f32, column.longest_nanos as f32);
        let left = (back + slide.through).mul_add(-pitch, chart.max.x);
        let (from, to) = (left.max(chart.min.x), (left + column_pixels).min(chart.max.x));
        let shows = from < to;
        if !shows {
            continue;
        }
        let tall = (longest / tallest_nanos).min(1.0) * chart.height();
        let bar = Rect2 { min: Vec2::new(from, chart.max.y - tall.max(1.0)), max: Vec2::new(to, chart.max.y) };
        push(records, bar, shade(column.longest_nanos, budget_nanos).opaque());
    }

    let line = layout.line_pixels(LINE_PIXELS);
    let budget_y = (chart.max.y - chart.height() / BUDGETS_TALL).round();
    let (dash, gap) = (layout.line_pixels(DASH_PIXELS), layout.line_pixels(DASH_GAP_PIXELS));
    let mut left = chart.min.x;
    for _ in 0..MOST_DASHES {
        let ends = left >= chart.max.x;
        if ends {
            break;
        }
        let right = (left + dash).min(chart.max.x);
        let piece = Rect2 { min: Vec2::new(left, budget_y), max: Vec2::new(right, budget_y + line) };
        push(records, piece, STONE.opaque());
        left = right + gap;
    }
}

/// The colour of a column whose longest cycle took `nanos` against `budget_nanos`: lichen within the budget, rust
/// over it, and brick over twice it.
fn shade(nanos: u64, budget_nanos: u64) -> Swatch {
    let over = nanos > budget_nanos;
    let twice = nanos > budget_nanos.saturating_mul(2);
    match (over, twice) {
        (_, true) => BRICK,
        (true, false) => RUST,
        (false, false) => LICHEN,
    }
}

/// Appends the record of `area` drawn in `colour`.
fn push(records: &mut Vec<u8>, area: Rect2, colour: Colour) {
    let [red, green, blue, alpha] = colour.channels();
    let values = [area.min.x, area.min.y, area.width(), area.height(), red, green, blue, alpha];
    records.extend(values.iter().flat_map(|value| value.to_ne_bytes()));
}
