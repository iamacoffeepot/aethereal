//! What each meter's row draws between its label and its value (ADR-0053: The three stops, Memory): its graph or bar,
//! from a character's space after its label to one before what its row writes at its right, which is as wide as the
//! row's text says, so the graph fills the room the text leaves.

use super::bars::bar;
use super::{Backlog, EngineMemory, Held, Meter, MeterHistories};
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
    let columns = layout.right_columns();
    for meter in Meter::ALL {
        let tail = memory.tail_characters(meter, histories.gpu_shown(), columns);
        let area = layout.meter_graph(meter.index(), tail);
        match (meter, memory) {
            (Meter::Process, _) => histories.process.graph(records, layout, area, &slide_of),
            (Meter::Guests, _) => histories.guests.graph(records, layout, area, &slide_of),
            (Meter::Gpu, _) => histories.gpu.graph(records, layout, area, &slide_of),
            (Meter::Blobs, EngineMemory::Read(figures)) => bar(records, area, figures.blobs),
            (Meter::Backlog, EngineMemory::Read(figures)) => {
                let held = match &figures.backlog {
                    Backlog::Empty => Held { bytes: 0, most: 0 },
                    Backlog::Held { unsent, .. } => *unsent,
                };
                bar(records, area, held);
            }
            (Meter::Blobs | Meter::Backlog, EngineMemory::Unread) => {}
        }
    }
}
