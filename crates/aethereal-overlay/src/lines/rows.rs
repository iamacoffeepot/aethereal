//! How the lines' rows are drawn (ADR-0053: The three stops): each line a row beneath the GPU's graph, over the same
//! columns, each column a thin stone bar from the row's foot to the most its frames came to, on a scale linear from
//! nothing to the most any column shown came to, so the row's tallest bar is the most of the window.

use super::{Line, Values};
use crate::binned::Spanned;
use crate::chart::{left_of, push};
use crate::colour::STONE;
use crate::frames::Slide;
use crate::layout::Layout;
use aethereal_math::{Rect2, Vec2};

/// How opaque a row's bars are drawn, in stone: quieter than the graphs above, as the rows are a glance.
const BAR_OPACITY: f32 = 0.6;

/// Appends to `records` each line's row on `layout`: each of `columns` placed at `slide` as the frame chart's column of
/// its span is, cut to the row's left and right edges, a bar of its value against the most of the columns, at least a
/// pixel tall where it is not nothing. A row whose columns are all nothing draws nothing.
pub fn rows(
    records: &mut Vec<u8>,
    layout: Layout,
    slide: Slide,
    columns: &(impl Iterator<Item = Spanned<Values>> + Clone),
) {
    let width = layout.column_pixels();
    for (index, line) in (0_u8..).zip(Line::ALL) {
        let row = layout.row(index);
        let most = columns.clone().map(|spanned| spanned.column.of_line(line)).max().unwrap_or_default();
        for spanned in columns.clone() {
            let value = spanned.column.of_line(line);
            let left = left_of(layout, slide, spanned.span);
            let (from, to) = (left.max(row.min.x), (left + width).min(row.max.x));
            let shows = value > 0 && from < to;
            if !shows {
                continue;
            }
            // Counts and bytes far under an f64's exact range, and a share of the row from nothing to one.
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let share = (value as f64 / most as f64) as f32;
            let tall = (share * row.height()).max(1.0);
            let bar = Rect2 { min: Vec2::new(from, row.max.y - tall), max: Vec2::new(to, row.max.y) };
            push(records, bar, STONE.at(BAR_OPACITY));
        }
    }
}
