//! How the chart draws one column (ADR-0053: The three stops): three opaque bars the column's width, each from the
//! chart's foot up to one of its cycles on the chart's scale (`scale`), drawn back to front, so each shorter bar stands
//! in front of the taller one behind it.
//!
//! ```text
//!   ███    slowest: sand, the tallest, at the back
//!   ███    median: lichen, in the middle
//!   ███    fastest: slate-teal, the shortest, in front
//!   ███
//! ```
//!
//! Each layer is the same colour whatever the budget, so over or under is read from the bars' height against the
//! budget's dashed line alone.

use super::scale::height_fraction;
use super::{Fill, push};
use crate::colour::{LICHEN, SAND, SLATE_TEAL};
use crate::frames::Column;
use aethereal_math::{Rect2, Vec2};

/// Where one column is drawn: its left edge and width, before it is cut to the chart; the chart; and the budget it is
/// scaled against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub left: f32,
    pub width: f32,
    pub chart: Rect2,
    pub budget_nanos: u64,
}

impl Placed {
    /// The rows from the cycle `nanos` down to the chart's foot, from `left` to `right`, at least a pixel tall and
    /// inside the chart.
    fn bar(self, left: f32, right: f32, nanos: u64) -> Rect2 {
        let bottom = self.chart.max.y;
        let top = height_fraction(nanos, self.budget_nanos).mul_add(-self.chart.height(), bottom).min(bottom - 1.0);

        Rect2 { min: Vec2::new(left, top), max: Vec2::new(right, bottom) }
    }
}

/// Appends to `fills` `column` drawn at `placed`, cut to the chart's left and right edges: its slowest bar in sand,
/// its median in lichen, and its fastest in slate-teal, in that order, so they are drawn back to front.
pub fn column(fills: &mut Vec<Fill>, placed: Placed, column: Column) {
    let chart = placed.chart;
    let (from, to) = (placed.left.max(chart.min.x), (placed.left + placed.width).min(chart.max.x));
    let layers = [(column.slowest_nanos, SAND), (column.median_nanos, LICHEN), (column.fastest_nanos, SLATE_TEAL)];
    for (nanos, swatch) in layers {
        push(fills, placed.bar(from, to, nanos), swatch.opaque());
    }
}
