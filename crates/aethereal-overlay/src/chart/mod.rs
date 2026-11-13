//! How the frame chart is drawn (ADR-0053: The three stops), as boxes of one colour each, [`Fill`]s, in the box the
//! widget library laid the chart out at: the faint line at twice the budget, its columns, each three bars to its
//! slowest, median, and fastest cycles (`column`), on a scale logarithmic in a cycle's time and relative to the budget
//! (`scale`), its dashed budget line, and the marks' ticks, in that order, a later one over an earlier. The GPU's graph
//! (`gpu`), the lines' rows (`lines`), and the memory meters' graphs and bars (`meters`) draw the same way, each in its
//! own box; [`FrameChart`] is the primitive that draws this one into its box, as the chart moves with time.

mod column;
mod scale;
#[cfg(test)]
pub mod tests;
mod widget;

pub use widget::{ChartShown, FrameChart};

use crate::colour::{BONE, OCHRE, STONE};
use crate::frames::{Column, Slide};
use crate::layout::Scale;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameBudget;
use aethereal_render::text::Colour;
use column::Placed;
use scale::height_fraction;

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

/// One box the overlay's own drawing fills, in the window's pixels, and its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub area: Rect2,
    pub colour: Colour,
}

/// Appends to `fills` the frame chart in `area`, at `scale`: a faint stone line at twice `budget`; each of `columns`,
/// scaled against it; and the dashed budget line, bone, drawn lighter when the budget is a reference.
///
/// The newest column's span is at the chart's right edge, and each older one a column's pitch further left, all moved
/// left by the share of a pitch `slide` has come through the newest span, so the chart slides smoothly left as the
/// program's time passes: the newest column grows out of the right edge, and the oldest is cut at the left edge as it
/// slides off.
pub fn records(
    fills: &mut Vec<Fill>,
    area: Rect2,
    scale: Scale,
    columns: impl IntoIterator<Item = Column>,
    slide: Slide,
    budget: FrameBudget,
) {
    let budget_nanos = budget.nanos();
    let (width, line) = (scale.column_pixels(), scale.line_pixels(LINE_PIXELS));
    let height_of = |nanos: u64| height_fraction(nanos, budget_nanos).mul_add(-area.height(), area.max.y).round();
    let twice_y = height_of(budget_nanos.saturating_mul(2));
    let twice = Rect2 { min: Vec2::new(area.min.x, twice_y), max: Vec2::new(area.max.x, twice_y + line) };
    push(fills, twice, STONE.at(TWICE_BUDGET_OPACITY));

    for column in columns {
        let left = left_of(area, scale, slide, column.span);
        let shows = left.max(area.min.x) < (left + width).min(area.max.x);
        if !shows {
            continue;
        }
        let placed = Placed { left, width, chart: area, budget_nanos };
        column::column(fills, placed, column);
    }

    dashes(fills, area, scale, height_of(budget_nanos), budget);
}

/// Where the left edge of the column of `span` is drawn at `slide` in a graph `area`, before it is cut to it: the
/// newest's span at its right edge, each older one a column's pitch further left, all moved left by the share of a
/// pitch `slide` has come through the newest span. The GPU's graph and the lines' rows, as wide as the frame chart and
/// beneath it, place their columns here too, so each stands under the frame chart's column of the same span.
pub fn left_of(area: Rect2, scale: Scale, slide: Slide, span: u64) -> f32 {
    left_from(area.max.x, scale.column_pitch(), slide, span)
}

/// Where the left edge of the column of `span` is drawn at `slide` in a graph whose right edge is `right`, its columns
/// `pitch` pixels apart: the newest's span at `right`, each older one a pitch further left, all moved left by the share
/// of a pitch `slide` has come through the newest span.
pub fn left_from(right: f32, pitch: f32, slide: Slide, span: u64) -> f32 {
    // Columns back from the newest, at most a few thousand, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss)]
    let back = slide.newest.saturating_sub(span) as f32;

    (back + slide.through).mul_add(-pitch, right)
}

/// Appends to `fills` a budget line at `y` across `area`, dashed: bone at 70% for a limit, and a lighter, sparser dash
/// at half that for a budget that is a reference.
pub fn dashes(fills: &mut Vec<Fill>, area: Rect2, scale: Scale, y: f32, budget: FrameBudget) {
    let line = scale.line_pixels(LINE_PIXELS);
    let (dash, gap, colour) = match budget {
        FrameBudget::Limit { .. } => (DASH_PIXELS, DASH_GAP_PIXELS, BONE.at(BUDGET_OPACITY)),
        FrameBudget::Reference { .. } => {
            (REFERENCE_DASH_PIXELS, REFERENCE_GAP_PIXELS, BONE.at(BUDGET_OPACITY * REFERENCE_OPACITY))
        }
    };
    let (dash, gap) = (scale.line_pixels(dash), scale.line_pixels(gap));
    let mut left = area.min.x;
    for _ in 0..MOST_DASHES {
        let ends = left >= area.max.x;
        if ends {
            break;
        }
        let right = (left + dash).min(area.max.x);
        let piece = Rect2 { min: Vec2::new(left, y), max: Vec2::new(right, y + line) };
        push(fills, piece, colour);
        left = right + gap;
    }
}

/// Appends to `fills` a tick for each mark of `times`, the program's time it was pinned at: an ochre line `area`'s
/// height, where that moment falls among the columns placed at `slide`, sliding left with them. A mark that falls
/// outside the chart is not drawn.
pub fn marks(fills: &mut Vec<Fill>, area: Rect2, scale: Scale, slide: Slide, times: impl IntoIterator<Item = u64>) {
    let (pitch, width) = (scale.column_pitch(), scale.line_pixels(TICK_PIXELS));
    let span_nanos = slide.span_nanos.max(1);
    // Spans of the program's time, some thousands, and a share of one, which an f32 holds closely enough to draw.
    #[allow(clippy::cast_precision_loss)]
    let newest = slide.newest as f32 + slide.through;
    for at_nanos in times {
        #[allow(clippy::cast_precision_loss)]
        let at = (at_nanos / span_nanos) as f32 + (at_nanos % span_nanos) as f32 / span_nanos as f32;
        let left = (newest - at).mul_add(-pitch, area.max.x);
        let within = left >= area.min.x && left + width <= area.max.x;
        if !within {
            continue;
        }
        let tick = Rect2 { min: Vec2::new(left, area.min.y), max: Vec2::new(left + width, area.max.y) };
        push(fills, tick, OCHRE.opaque());
    }
}

/// Appends `area` filled with `colour`.
pub fn push(fills: &mut Vec<Fill>, area: Rect2, colour: Colour) {
    fills.push(Fill { area, colour });
}
