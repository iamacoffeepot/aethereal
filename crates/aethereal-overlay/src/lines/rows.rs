//! How the lines' rows are drawn (ADR-0053: The three stops): each line a row beneath the GPU's graph, over the same
//! columns, each column a thin stone bar from the row's foot to the most its frames came to, on a scale linear from
//! nothing to the most any column shown came to, so the row's tallest bar is the most of the window. The memory meters
//! with no limit draw their minute of history the same way (`row`), each in its own row of the right column.

use super::{Line, Values};
use crate::binned::Spanned;
use crate::chart::{left_from, push};
use crate::colour::STONE;
use crate::frames::Slide;
use crate::layout::Layout;
use aethereal_math::{Rect2, Vec2};

/// How opaque a row's bars are drawn, in stone: quieter than the graphs above, as the rows are a glance.
const BAR_OPACITY: f32 = 0.6;

/// Appends to `records` each line's row on `layout`, over `columns` placed at `slide` as the frame chart's columns of
/// the same spans are (`row`).
pub fn rows(
    records: &mut Vec<u8>,
    layout: Layout,
    slide: Slide,
    columns: &(impl Iterator<Item = Spanned<Values>> + Clone),
) {
    for (index, line) in (0_u8..).zip(Line::ALL) {
        let values =
            columns.clone().map(move |spanned| Spanned { span: spanned.span, column: spanned.column.of_line(line) });
        row(records, layout, layout.row(index), slide, &values);
    }
}

/// Appends to `records` the row `area` of `values`: each placed at `slide` with its span's column at the row's right
/// edge and each older one a column's pitch further left, cut to the row's left and right edges, a bar of its value
/// against the most of the values, at least a pixel tall where it is not nothing. A row whose values are all nothing
/// draws nothing.
pub fn row(
    records: &mut Vec<u8>,
    layout: Layout,
    area: Rect2,
    slide: Slide,
    values: &(impl Iterator<Item = Spanned<u64>> + Clone),
) {
    let (width, pitch) = (layout.column_pixels(), layout.column_pitch());
    let most = values.clone().map(|spanned| spanned.column).max().unwrap_or_default();
    for spanned in values.clone() {
        let value = spanned.column;
        let left = left_from(area.max.x, pitch, slide, spanned.span);
        let (from, to) = (left.max(area.min.x), (left + width).min(area.max.x));
        let shows = value > 0 && from < to;
        if !shows {
            continue;
        }
        // Counts and bytes far under an f64's exact range, and a share of the row from nothing to one.
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let share = (value as f64 / most as f64) as f32;
        let tall = (share * area.height()).max(1.0);
        let bar = Rect2 { min: Vec2::new(from, area.max.y - tall), max: Vec2::new(to, area.max.y) };
        push(records, bar, STONE.at(BAR_OPACITY));
    }
}
