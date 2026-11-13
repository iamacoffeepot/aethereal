//! The band's right column (ADR-0053: The three stops, Memory): one meter a row for each limit the engine can hit,
//! process, blobs, backlog, guests, and GPU, read from the engine's figures four times a second and from each frame's
//! statistics, the five rows sharing the column's height; in each row, as the left column's rows are, a graph across
//! the column to a gutter at its right, every row's graph between the same two edges (`rows`), drawn by the row's
//! primitive (`widget`): a bar against its limit where the meter has one (`bars`), or else the meter's last minute
//! (`history`); and in the gutter each meter's label and value as text (`figures`).

mod bars;
mod figures;
mod history;
mod rows;
#[cfg(test)]
mod tests;
mod widget;

pub use history::{MOST_METER_COLUMNS, MeterHistories};
pub use rows::drawn;
pub use widget::{MeterDrawn, MeterGraph};

use aethereal_actor::{Against, EngineStats, Figures, Process};
use aethereal_data::Plain;

/// How many meters the right column holds, a row each.
const METERS: usize = 5;

/// Which meter, in the order they stand from the top of the right column.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Meter {
    /// The memory the program holds, as its chassis reads it.
    Process,
    /// The bytes the blob store holds, against its most.
    Blobs,
    /// The most bytes any one actor holds unsent, against its most.
    Backlog,
    /// Every guest's memory, added up.
    Guests,
    /// The GPU memory of everything the renderer made, as the newest frame's statistics tell it.
    Gpu,
}

impl Meter {
    /// Every meter, in the order they stand from the top.
    pub const ALL: [Self; METERS] = [Self::Process, Self::Blobs, Self::Backlog, Self::Guests, Self::Gpu];

    /// Its row from the top of the right column.
    pub const fn index(self) -> u8 {
        self as u8
    }
}

/// Bytes held against the most that may be: what a meter with a limit draws its bar from.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    pub bytes: u64,
    pub most: u64,
}

impl From<Against> for Held {
    fn from(against: Against) -> Self {
        Self { bytes: against.bytes, most: against.most }
    }
}

/// The memory the program holds, as its chassis reads it.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMemory {
    /// The bytes of the program resident in memory.
    Resident { bytes: u64 },
    /// The bytes of the script heap a browser reports for the page.
    Heap { bytes: u64 },
    /// The chassis reads no figure.
    Unknown,
}

/// The engine's memory as the meters show it.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct MemoryFigures {
    pub process: ProcessMemory,
    pub blobs: Held,
    /// What the actor holding the most bytes unsent holds, against its most: nothing against nothing when no actor
    /// holds any. Which actor it is, the `memory` tool names.
    pub backlog: Held,
    pub guest_bytes: u64,
}

/// The engine's memory as the overlay last read it.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum EngineMemory {
    /// Not read since the overlay began, or read where no engine's figures are: the meters say so.
    Unread,
    /// The engine's figures as the step it was read in began.
    Read(MemoryFigures),
}

impl EngineMemory {
    /// What `stats` tell of the engine's memory.
    pub fn of(stats: &EngineStats) -> Self {
        match stats {
            EngineStats::Unread => Self::Unread,
            EngineStats::Read(figures) => Self::Read(MemoryFigures::of(figures)),
        }
    }
}

impl MemoryFigures {
    /// `figures` as the meters show them.
    fn of(figures: &Figures) -> Self {
        let process = match figures.process {
            Process::Resident { bytes } => ProcessMemory::Resident { bytes },
            Process::Heap { bytes } => ProcessMemory::Heap { bytes },
            Process::Unknown => ProcessMemory::Unknown,
        };

        Self {
            process,
            blobs: Held::from(figures.blobs),
            backlog: Held::from(figures.backlog()),
            guest_bytes: figures.guest_bytes,
        }
    }
}
