//! The last minute of the meters with no limit, process, guests, and GPU (ADR-0053: The three stops, Memory): each
//! reading kept by the program's time it was read at and cut into columns over the same 60 seconds as the left
//! column's, as many as the meter's graph holds, each the most of the readings in its span; drawn over a trough of
//! faint stone, as the bars of the meters with a limit are (`widget`), so a minute of nothing reads as a graph at nothing, and
//! as the lines' rows are, a stone bar a column, linear from nothing to the most any column shown came to. A column no
//! reading fell in carries the reading before it, for up to [`CARRIED_NANOS`], as the figures' reads, four times a
//! second, are about a column apart and now and then miss one: a steady meter draws a steady line, and a pause in the
//! readings longer than that, as while the overlay is hidden, stays a gap. Process and guests are read with the
//! engine's figures, four times a second, and GPU from each frame's statistics: every kind's bytes, summed.

use super::{EngineMemory, MemoryFigures, ProcessMemory};
use crate::binned::{Accrue, Binned, Spanned};
use crate::frames::Slide;
use aethereal_data::Plain;
use aethereal_render::stats::{FrameStats, GpuMemory};

/// The most columns a meter's graph is cut into, whatever the window's width: two thirds of the frame chart's most, as
/// the right column is two thirds as wide as the left, and a graph narrower than the column.
pub const MOST_METER_COLUMNS: u32 = 2_000;

/// How long a reading stands for the columns after it that no reading fell in: about four of the figures' reads.
const CARRIED_NANOS: u64 = 1_000_000_000;

/// A column that is the most of the readings in its span.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Most {
    most: u64,
}

impl Accrue for Most {
    type Sample = u64;
    type Column = u64;

    fn first(reading: u64) -> Self {
        Self { most: reading }
    }

    fn take(&mut self, reading: u64) {
        self.most = self.most.max(reading);
    }

    fn column(&self) -> u64 {
        self.most
    }
}

/// One meter's readings over the last minute and more, binned into its graph's columns.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct History {
    binned: Binned<Most>,
}

impl History {
    /// Takes in `reading`, read at the program's time `elapsed_nanos`.
    pub fn push(&mut self, elapsed_nanos: u64, reading: u64) {
        self.binned.push(elapsed_nanos, reading);
    }

    /// Cuts the history into the columns of a graph of `count`, as the left column's are cut.
    pub fn cut(&mut self, count: u32) {
        self.binned.cut(count);
    }

    /// The columns of the graph as it was last cut into `count`, placed at `slide`: a column no reading fell in
    /// carrying the reading before it ([`carried`]).
    pub fn columns(&self, slide: Slide, count: u32) -> Vec<Spanned<u64>> {
        carried(self.binned.newest_first(), slide, count)
    }

    /// The newest reading: nothing while none has been taken in.
    fn newest(&self) -> u64 {
        self.binned.newest().unwrap_or_default()
    }
}

/// The columns of a graph cut into `count`, placed at `slide`, from its newest span back over the `count + 2` it draws,
/// out of `columns`, newest first: each span's own column, or where no reading fell in it, the newest column before
/// it, while that is no more than [`CARRIED_NANOS`] older; a span with neither is left out.
fn carried(columns: impl Iterator<Item = Spanned<u64>>, slide: Slide, count: u32) -> Vec<Spanned<u64>> {
    let carried_spans = (CARRIED_NANOS / slide.span_nanos.max(1)).max(1);
    let oldest = slide.newest.saturating_sub(u64::from(count) + 1);
    let mut read = columns.peekable();
    let mut drawn = Vec::new();
    for span in (oldest..=slide.newest).rev() {
        while read.next_if(|column| column.span > span).is_some() {}
        let Some(before) = read.peek() else {
            break;
        };
        let near = span - before.span <= carried_spans;
        if near {
            drawn.push(Spanned { span, column: before.column });
        }
    }

    drawn
}

/// The histories of the meters with no limit.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct MeterHistories {
    pub process: History,
    pub guests: History,
    pub gpu: History,
}

impl MeterHistories {
    /// Takes in the GPU memory of the frame `stats` tell of.
    pub fn framed(&mut self, stats: &FrameStats) {
        self.gpu.push(stats.elapsed_nanos, gpu_bytes(stats.memory));
    }

    /// Takes in the engine's memory `memory`, read at the program's time `elapsed_nanos`: process where the chassis
    /// reads a figure, and guests. Nothing before the engine's figures are read.
    pub fn read(&mut self, elapsed_nanos: u64, memory: &EngineMemory) {
        let EngineMemory::Read(MemoryFigures { process, guest_bytes, .. }) = memory else {
            return;
        };
        match process {
            ProcessMemory::Resident { bytes } | ProcessMemory::Heap { bytes } => {
                self.process.push(elapsed_nanos, *bytes);
            }
            ProcessMemory::Unknown => {}
        }
        self.guests.push(elapsed_nanos, *guest_bytes);
    }

    /// Cuts each history into the columns of a graph of `count`.
    pub fn cut(&mut self, count: u32) {
        for history in [&mut self.process, &mut self.guests, &mut self.gpu] {
            history.cut(count);
        }
    }

    /// The GPU's memory the meters' text shows: the newest frame's.
    pub fn gpu_shown(&self) -> u64 {
        self.gpu.newest()
    }
}

/// Every kind of GPU memory in `memory`, summed.
pub fn gpu_bytes(memory: GpuMemory) -> u64 {
    let GpuMemory { meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes } = memory;

    [meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes].into_iter().fold(0, u64::saturating_add)
}
