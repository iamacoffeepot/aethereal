//! The last minute of the meters with no limit, process, guests, and GPU (ADR-0053: The three stops, Memory): each
//! reading kept by the program's time it was read at and cut into columns over the same 60 seconds as the left
//! column's, as many as the meter's graph holds, each the most of the readings in its span; drawn over a trough of
//! faint stone, as the bars of the meters with a limit are, so a minute of nothing reads as a graph at nothing, and
//! as the lines' rows are, a stone bar a column, linear from nothing to the most any column shown came to. Process and guests are read
//! with the engine's figures, four times a second, and GPU from each frame's statistics: every kind's bytes, summed.

use super::bars::TROUGH_OPACITY;
use super::{EngineMemory, MemoryFigures, ProcessMemory};
use crate::binned::{Accrue, Binned};
use crate::chart::{columns_across, push};
use crate::colour::STONE;
use crate::frames::Slide;
use crate::layout::Layout;
use crate::lines;
use aethereal_data::Plain;
use aethereal_math::Rect2;
use aethereal_render::stats::{FrameStats, GpuMemory};

/// How many meters keep a history: process, guests, and GPU.
pub const HISTORIES: u32 = 3;

/// The most columns a meter's graph is cut into, whatever the window's width: two thirds of the frame chart's most, as
/// the right column is two thirds as wide as the left, and a graph narrower than the column.
pub const MOST_METER_COLUMNS: u32 = 2_000;

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

    /// Appends to `records` the graph `area` on `layout`: its trough, and over it its columns, cut into as many columns as fit across it, each placed at
    /// the slide `slide_of` gives for that count, as the left column's are at its own.
    pub fn graph(&mut self, records: &mut Vec<u8>, layout: Layout, area: Rect2, slide_of: impl Fn(u32) -> Slide) {
        let count = columns_across(layout, area.width(), MOST_METER_COLUMNS);
        self.binned.cut(count);
        push(records, area, STONE.at(TROUGH_OPACITY));

        lines::row(records, layout, area, slide_of(count), &self.binned.newest_first());
    }

    /// The newest reading: nothing while none has been taken in.
    fn newest(&self) -> u64 {
        self.binned.newest().unwrap_or_default()
    }
}

/// The histories of the meters with no limit, and the GPU's memory the meters' text shows.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct MeterHistories {
    pub process: History,
    pub guests: History,
    pub gpu: History,
    /// The newest frame's GPU memory as the engine's figures were last read, which the GPU meter's text shows and its
    /// row is laid out by until the next read, so its graph ends where its text begins.
    gpu_shown: u64,
}

impl MeterHistories {
    /// Takes in the GPU memory of the frame `stats` tell of.
    pub fn framed(&mut self, stats: &FrameStats) {
        self.gpu.push(stats.elapsed_nanos, gpu_bytes(stats.memory));
    }

    /// Takes in the engine's memory `memory`, read at the program's time `elapsed_nanos`: process where the chassis
    /// reads a figure, and guests; and the GPU's newest memory for the text. Nothing of the engine's before its figures
    /// are read.
    pub fn read(&mut self, elapsed_nanos: u64, memory: &EngineMemory) {
        self.gpu_shown = self.gpu.newest();
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

    /// The GPU's memory the meters' text shows.
    pub const fn gpu_shown(&self) -> u64 {
        self.gpu_shown
    }
}

/// Every kind of GPU memory in `memory`, summed.
pub fn gpu_bytes(memory: GpuMemory) -> u64 {
    let GpuMemory { meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes } = memory;

    [meshes_bytes, textures_bytes, targets_bytes, buffers_bytes, atlas_bytes].into_iter().fold(0, u64::saturating_add)
}
