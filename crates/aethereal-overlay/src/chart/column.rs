//! How the chart draws one column (ADR-0053: The three stops): a bar to its median cycle, and a line from the bar up
//! to its slowest when that stands well above it, each on the chart's scale (`scale`).
//!
//! ```text
//!   │      line: the bar's top to the slowest cycle, a line, shaded by the slowest; only past 1.15 × the median
//!  ███     bar: the chart's foot to the median, the column's width, shaded by the median
//!  ███
//! ```
//!
//! So one spike in a column of healthy cycles is a rust line over a lichen bar, a stretch of slow cycles is a rust
//! bar, and a column of even cycles is a plain bar.

use super::scale::height_fraction;
use super::{push, shade};
use crate::frames::Column;
use aethereal_math::{Rect2, Vec2};

/// The most records one column is drawn in: its bar and its line.
pub const RECORDS_PER_COLUMN: u32 = 2;

/// How far above its median, in hundredths of it, a column's slowest cycle must stand for the line to it to be drawn:
/// more than 15% above, so a column of even cycles stays a plain bar.
const LINE_PAST_MEDIAN_PERCENT: u64 = 115;

/// Where one column is drawn: its left edge and width, before it is cut to the chart; the chart; the budget it is
/// scaled and shaded against; and how thick a line is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub left: f32,
    pub width: f32,
    pub chart: Rect2,
    pub budget_nanos: u64,
    pub line: f32,
}

impl Placed {
    /// How far down the chart a cycle `nanos` long stands, on the chart's scale.
    fn height_of(self, nanos: u64) -> f32 {
        height_fraction(nanos, self.budget_nanos).mul_add(-self.chart.height(), self.chart.max.y)
    }

    /// The rows from the cycle `high_nanos` down to `bottom`, from `left` to `right`, at least a pixel tall and inside
    /// the chart.
    fn down_to(self, left: f32, right: f32, high_nanos: u64, bottom: f32) -> Rect2 {
        let top = self.height_of(high_nanos);
        let bottom = bottom.max(top + 1.0).min(self.chart.max.y);
        let top = top.min(bottom - 1.0);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, bottom) }
    }
}

/// Whether a column's slowest cycle stands far enough above its median for the line to it to be drawn.
fn reaches_past_median(column: Column) -> bool {
    let slowest = u128::from(column.slowest_nanos) * 100;
    let threshold = u128::from(column.median_nanos) * u128::from(LINE_PAST_MEDIAN_PERCENT);

    slowest > threshold
}

/// Appends to `records` `column` drawn at `placed`, cut to the chart's left and right edges: its bar, then its line
/// when its slowest cycle reaches far enough past its median.
pub fn column(records: &mut Vec<u8>, placed: Placed, column: Column) {
    let chart = placed.chart;
    let (from, to) = (placed.left.max(chart.min.x), (placed.left + placed.width).min(chart.max.x));
    let bar = placed.down_to(from, to, column.median_nanos, chart.max.y);
    push(records, bar, shade(column.median_nanos, placed.budget_nanos).opaque());

    let line_left = placed.left + ((placed.width - placed.line) / 2.0).floor();
    let (line_from, line_to) = (line_left.max(chart.min.x), (line_left + placed.line).min(chart.max.x));
    let shows = line_from < line_to && reaches_past_median(column);
    if shows {
        let line = placed.down_to(line_from, line_to, column.slowest_nanos, bar.min.y);
        push(records, line, shade(column.slowest_nanos, placed.budget_nanos).opaque());
    }
}
