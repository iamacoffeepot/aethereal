//! The bars of the meters that have a limit, blobs and backlog (ADR-0053: Memory, Colours): each a trough of faint
//! stone after the meter's label, and over it a fill from its left, as wide a share of the trough as the bytes held are
//! of their most, at least a pixel where they are not nothing: lichen while under 75% of the most, rust from 75%, and
//! brick from 90%. With no most, as when no actor holds bytes unsent, the trough is drawn empty.

use super::{Backlog, EngineMemory, Held, Meter};
use crate::chart::push;
use crate::colour::{BRICK, LICHEN, RUST, STONE, Swatch};
use crate::layout::Layout;
use aethereal_math::{Rect2, Vec2};

/// The most records the bars take: a trough and a fill for each of the two meters with a limit.
pub const MOST_BAR_RECORDS: u32 = 4;

/// How opaque a bar's trough is drawn, in stone: faint, as the fill over it carries the reading.
const TROUGH_OPACITY: f32 = 0.08;

/// The percentages of the most from which a fill is rust, and then brick.
const RUST_PERCENT: u64 = 75;
const BRICK_PERCENT: u64 = 90;

/// Appends to `records` the bars of the meters with a limit, on `layout`, from `memory`: nothing before the engine's
/// figures are read.
pub fn bars(records: &mut Vec<u8>, layout: Layout, memory: &EngineMemory) {
    let EngineMemory::Read(figures) = memory else {
        return;
    };
    let backlog = match &figures.backlog {
        Backlog::Empty => Held { bytes: 0, most: 0 },
        Backlog::Held { unsent, .. } => *unsent,
    };
    bar(records, layout.meter_bar(Meter::Blobs.index()), figures.blobs);
    bar(records, layout.meter_bar(Meter::Backlog.index()), backlog);
}

/// Appends the trough `trough` and, where `held` has a most and is not nothing, its fill.
fn bar(records: &mut Vec<u8>, trough: Rect2, held: Held) {
    push(records, trough, STONE.at(TROUGH_OPACITY));
    let fills = held.most > 0 && held.bytes > 0;
    if !fills {
        return;
    }
    // Bytes far under an f64's exact range, and a share of the trough from nothing to one.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    let share = (held.bytes as f64 / held.most as f64).min(1.0) as f32;
    let width = (share * trough.width()).max(1.0);
    let fill = Rect2 { min: trough.min, max: Vec2::new(trough.min.x + width, trough.max.y) };
    push(records, fill, fill_swatch(held).opaque());
}

/// The colour of `held`'s fill: lichen under 75% of its most, rust from 75%, and brick from 90%.
pub(super) fn fill_swatch(held: Held) -> Swatch {
    let percent = held.bytes.saturating_mul(100) / held.most.max(1);
    match percent {
        BRICK_PERCENT.. => BRICK,
        RUST_PERCENT.. => RUST,
        _ => LICHEN,
    }
}
