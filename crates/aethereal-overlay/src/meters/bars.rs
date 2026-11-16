//! The bars of the meters that have a limit, blobs and backlog (ADR-0053: Memory, Colours): each a trough of faint
//! dim across its row's graph, left of its gutter, as tall as the other meters' graphs, and over it a
//! fill from its left, as wide a share of the trough as the bytes held are of their most, at least a pixel where they
//! are not nothing: lichen while under 75% of the most, the warning's ink from 75%, and the error's from 90%. With no most, as when no
//! actor holds bytes unsent, the trough is drawn empty.

use super::Held;
use crate::chart::{Fill, push};
use crate::colour::{Ink, Palette};
use aethereal_math::{Rect2, Vec2};

/// How opaque a bar's trough, and a history's, is drawn, dim: faint, as the fill or bars over it carry the
/// reading.
pub(super) const TROUGH_OPACITY: f32 = 0.08;

/// The percentages of the most from which a fill is a warning, and then an error.
const WARN_PERCENT: u64 = 75;
const ERROR_PERCENT: u64 = 90;

/// Appends the trough `trough` and, where `held` has a most and is not nothing, its fill.
pub(super) fn bar(fills: &mut Vec<Fill>, palette: &Palette, trough: Rect2, held: Held) {
    push(fills, trough, palette.faded(Ink::Dim, TROUGH_OPACITY));
    let filled = held.most > 0 && held.bytes > 0;
    if !filled {
        return;
    }
    // Bytes far under an f64's exact range, and a share of the trough from nothing to one.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let share = (held.bytes as f64 / held.most as f64).min(1.0) as f32;
    let width = (share * trough.width()).max(1.0);
    let fill = Rect2 { min: trough.min, max: Vec2::new(trough.min.x + width, trough.max.y) };
    push(fills, fill, palette.ink(fill_ink(held)));
}

/// The ink of `held`'s fill: lichen under 75% of its most, the warning's from 75%, and the error's from 90%.
pub(super) fn fill_ink(held: Held) -> Ink {
    let percent = held.bytes.saturating_mul(100) / held.most.max(1);
    match percent {
        ERROR_PERCENT.. => Ink::Error,
        WARN_PERCENT.. => Ink::Warn,
        _ => Ink::Lichen,
    }
}
