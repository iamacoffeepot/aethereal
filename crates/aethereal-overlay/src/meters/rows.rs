//! What each meter's row draws left of its gutter (ADR-0053: The three stops, Memory): its graph or bar, from the right
//! column's left to a character's space before the gutter, the same two edges for every row, so the graphs stand
//! aligned with each other as the left column's do.

use super::bars::bar;
use super::{EngineMemory, Meter, MeterHistories};
use crate::frames::Slide;
use crate::layout::Layout;

/// Appends to `records` each meter's graph or bar on `layout`: the history of process, guests, and GPU in
/// `histories`, each placed at the slide `slide_of` gives for its count of columns; and the bars of blobs and backlog
/// against their limits in `memory`, nothing of those before the engine's figures are read.
pub fn rows(
    records: &mut Vec<u8>,
    layout: Layout,
    memory: &EngineMemory,
    histories: &mut MeterHistories,
    slide_of: impl Fn(u32) -> Slide,
) {
    for meter in Meter::ALL {
        let area = layout.meter_graph(meter.index());
        match (meter, memory) {
            (Meter::Process, _) => histories.process.graph(records, layout, area, &slide_of),
            (Meter::Guests, _) => histories.guests.graph(records, layout, area, &slide_of),
            (Meter::Gpu, _) => histories.gpu.graph(records, layout, area, &slide_of),
            (Meter::Blobs, EngineMemory::Read(figures)) => bar(records, area, figures.blobs),
            (Meter::Backlog, EngineMemory::Read(figures)) => bar(records, area, figures.backlog),
            (Meter::Blobs | Meter::Backlog, EngineMemory::Unread) => {}
        }
    }
}
