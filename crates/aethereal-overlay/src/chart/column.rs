//! How the chart draws one column (ADR-0053: The three stops): as a candle, by default, or as a bar of its slowest
//! cycle, as the chart was first drawn, so the two can be judged side by side.
//!
//! ```text
//!   │      wick: fastest to slowest, a line, shaded by the slowest
//!  ███     body: the 25th to the 75th percentile, the column's width, shaded by the 75th
//!  ▀▀▀     median: a slate line across the body
//!  ███
//!   │
//! ```
//!
//! So one spike in a column of healthy cycles is a rust wick over a lichen body, and a stretch of slow cycles is a
//! rust body.

use super::{push, shade};
use crate::colour::SLATE;
use crate::frames::Candle;
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};

/// How the frame chart draws each column.
#[derive(Plain, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ChartStyle {
    /// As a candle: a wick from its fastest cycle to its slowest, a body from its 25th to its 75th percentile, and a
    /// line across the body at its median.
    #[default]
    Candles,
    /// As a bar from the chart's foot to its slowest cycle.
    Bars,
}

/// The most records one column is drawn in: a candle's wick, body, and median.
pub const RECORDS_PER_COLUMN: u32 = 3;

/// How many lines thick a candle's body must be for its median to be drawn across it: a thinner body is its median.
const MEDIAN_ROOM_LINES: f32 = 3.0;

/// Where one column is drawn: its left edge and width, before it is cut to the chart; the chart; how long a cycle
/// reaches the chart's top; the budget it is shaded against; and how thick a line is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub left: f32,
    pub width: f32,
    pub chart: Rect2,
    pub tallest_nanos: f32,
    pub budget_nanos: u64,
    pub line: f32,
}

impl Placed {
    /// How far down the chart a cycle `nanos` long stands: the chart's foot for none, its top for one of
    /// `tallest_nanos` or longer.
    fn height_of(self, nanos: u64) -> f32 {
        // Nanoseconds of a few seconds, which an f32 holds closely enough to draw.
        #[allow(clippy::cast_precision_loss)]
        let share = (nanos as f32 / self.tallest_nanos).min(1.0);

        share.mul_add(-self.chart.height(), self.chart.max.y)
    }

    /// The rows from the cycle `high_nanos` down to the cycle `low_nanos`, from `left` to `right`, at least a pixel
    /// tall and inside the chart.
    fn between(self, left: f32, right: f32, high_nanos: u64, low_nanos: u64) -> Rect2 {
        let top = self.height_of(high_nanos);
        let bottom = self.height_of(low_nanos).max(top + 1.0).min(self.chart.max.y);
        let top = top.min(bottom - 1.0);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, bottom) }
    }
}

/// Appends to `records` the column of `candle` drawn in `style` at `placed`, cut to the chart's left and right edges.
pub fn column(records: &mut Vec<u8>, style: ChartStyle, placed: Placed, candle: Candle) {
    let chart = placed.chart;
    let (from, to) = (placed.left.max(chart.min.x), (placed.left + placed.width).min(chart.max.x));
    match style {
        ChartStyle::Bars => {
            let bar = placed.between(from, to, candle.slowest, 0);
            push(records, bar, shade(candle.slowest, placed.budget_nanos).opaque());
        }
        ChartStyle::Candles => {
            let wick_left = placed.left + ((placed.width - placed.line) / 2.0).floor();
            let (wick_from, wick_to) = (wick_left.max(chart.min.x), (wick_left + placed.line).min(chart.max.x));
            if wick_from < wick_to {
                let wick = placed.between(wick_from, wick_to, candle.slowest, candle.fastest);
                push(records, wick, shade(candle.slowest, placed.budget_nanos).opaque());
            }

            let body = placed.between(from, to, candle.upper, candle.lower);
            push(records, body, shade(candle.upper, placed.budget_nanos).opaque());

            let roomy = body.height() >= placed.line * MEDIAN_ROOM_LINES;
            if roomy {
                let middle = placed.height_of(candle.median);
                let top = placed.line.mul_add(-0.5, middle).round().clamp(body.min.y, body.max.y - placed.line);
                let median = Rect2 { min: Vec2::new(from, top), max: Vec2::new(to, top + placed.line) };
                push(records, median, SLATE.opaque());
            }
        }
    }
}
