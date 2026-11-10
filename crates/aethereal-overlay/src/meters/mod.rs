//! The band's right column (ADR-0053: The three stops, Memory): one meter a line for each limit the engine can hit,
//! process, blobs, backlog, guests, and GPU, read from the engine's figures four times a second and from the newest
//! frame's statistics; a bar against its limit where a meter has one (`bars`), and each meter's label and value as
//! text (`figures`).

mod bars;
mod figures;
#[cfg(test)]
mod tests;

pub use bars::{MOST_BAR_RECORDS, bars};
/// How many characters a meter's line takes, which the layout's tests fit across the right column.
#[cfg(test)]
pub use figures::LINE_CHARACTERS;

use crate::tools::named;
use aethereal_actor::{Against, EngineStats, Figures, Process};
use aethereal_data::Plain;

/// Which meter, in the order they stand from the top of the right column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const ALL: [Self; 5] = [Self::Process, Self::Blobs, Self::Backlog, Self::Guests, Self::Gpu];

    /// Its line from the top of the right column.
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

/// The engine's backlog: no actor holds any bytes unsent, or the one holding the most, by name, and what it holds.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum Backlog {
    /// No actor has said it holds bytes unsent.
    Empty,
    /// The actor holding the most bytes unsent, named by its path, and what it holds against its most.
    Held { unsent: Held, actor: String },
}

/// The engine's memory as the meters show it.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct MemoryFigures {
    pub process: ProcessMemory,
    pub blobs: Held,
    pub backlog: Backlog,
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
        let held = |longest: &aethereal_actor::Backlog| Backlog::Held {
            unsent: Held::from(longest.unsent),
            actor: named(&longest.actor),
        };
        let backlog = figures.longest_backlogs.first().map_or(Backlog::Empty, held);

        Self { process, blobs: Held::from(figures.blobs), backlog, guest_bytes: figures.guest_bytes }
    }
}
