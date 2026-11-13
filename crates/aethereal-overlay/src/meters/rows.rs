//! What each meter's row draws left of its gutter (ADR-0053: The three stops, Memory): its last minute where it has no
//! limit, process, guests, and GPU, or its bar against its limit, blobs and backlog, nothing of those before the
//! engine's figures are read; each drawn by the row's primitive (`widget`) between the same two edges, so the graphs
//! stand aligned with each other as the left column's do.

use super::widget::MeterDrawn;
use super::{EngineMemory, Meter, MeterHistories};
use crate::frames::Slide;

/// What `meter`'s row draws: its history in `histories` cut into `count` columns placed at `slide`, or its bar against
/// its limit in `memory`.
pub fn drawn(meter: Meter, memory: &EngineMemory, histories: &MeterHistories, slide: Slide, count: u32) -> MeterDrawn {
    let history = |kept: &super::history::History| MeterDrawn::History { columns: kept.columns(slide, count), slide };
    match (meter, memory) {
        (Meter::Process, _) => history(&histories.process),
        (Meter::Guests, _) => history(&histories.guests),
        (Meter::Gpu, _) => history(&histories.gpu),
        (Meter::Blobs, EngineMemory::Read(figures)) => MeterDrawn::Bar(figures.blobs),
        (Meter::Backlog, EngineMemory::Read(figures)) => MeterDrawn::Bar(figures.backlog),
        (Meter::Blobs | Meter::Backlog, EngineMemory::Unread) => MeterDrawn::Nothing,
    }
}
