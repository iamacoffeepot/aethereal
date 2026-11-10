//! How the GPU's graph is drawn (ADR-0053: The three stops): beneath the frame chart, over the same columns, each a
//! stack of its passes from the graph's foot, each pass at its median over the column's frames, in its pass's colour.
//! The scale is linear, as stacked times are summed: nothing at the foot, twice the budget at the top, and the budget,
//! dashed as the frame chart's, half way up. A stack over the top is cut there.
//!
//! ```text
//!   - - - - - -   the budget, half way up
//!   ███           a third pass, mauve
//!   ███           a second pass, muted violet
//!   ███           the first pass heard, slate blue, at the foot
//! ```

use super::Stack;
use crate::binned::Spanned;
use crate::chart::{dashes, left_of, push};
use crate::colour::PASS_SWATCHES;
use crate::frames::Slide;
use crate::layout::Layout;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::stats::FrameBudget;

/// Appends to `records` the GPU's graph on `layout`: each of `columns` placed at `slide` as the frame chart's column of
/// its span is, cut to the graph's left and right edges, its passes stacked from the foot in their order, and then
/// the budget's dashed line half way up.
pub fn graph(
    records: &mut Vec<u8>,
    layout: Layout,
    slide: Slide,
    budget: FrameBudget,
    columns: impl IntoIterator<Item = Spanned<Stack>>,
) {
    let area = layout.gpu_graph();
    let width = layout.column_pixels();
    let top_nanos = budget.nanos().saturating_mul(2).max(1);
    // Nanoseconds of at most some minutes, which an f64 holds closely enough to draw; and a share of the graph from
    // nothing to one, which an f32 holds closely enough too.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let height_of =
        |nanos: u64| ((nanos as f64 / top_nanos as f64).min(1.0) as f32).mul_add(-area.height(), area.max.y);

    for spanned in columns {
        let left = left_of(layout, slide, spanned.span);
        let (from, to) = (left.max(area.min.x), (left + width).min(area.max.x));
        let shows = from < to;
        if !shows {
            continue;
        }
        let mut beneath_nanos: u64 = 0;
        for (index, nanos) in spanned.column.nanos.into_iter().enumerate() {
            let bottom = height_of(beneath_nanos);
            beneath_nanos = beneath_nanos.saturating_add(nanos);
            let top = height_of(beneath_nanos);
            let stands = top < bottom;
            if !stands {
                continue;
            }
            let segment = Rect2 { min: Vec2::new(from, top), max: Vec2::new(to, bottom) };
            push(records, segment, PASS_SWATCHES[index % PASS_SWATCHES.len()].opaque());
        }
    }

    dashes(records, layout, height_of(budget.nanos()).round(), budget);
}
