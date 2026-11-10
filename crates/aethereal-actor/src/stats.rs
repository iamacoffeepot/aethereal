//! The engine's memory against its limits, as a turn reads it: the program's, the blobs', the guests', and the bytes
//! the actors that send hold unsent, with the largest guests and backlogs by name (ADR-0053).

use crate::ActorName;
use alloc::vec::Vec;

/// How many of the largest guests, and of the longest backlogs, the engine names: the eight with the most bytes.
pub const LARGEST: usize = 8;

/// What a turn reads of the engine's memory, as it stood when the step began (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStats {
    /// The table the turn read through reads no engine: a guest's, as it reads no log.
    Unread,
    /// The engine's figures.
    Read(Figures),
}

/// The engine's memory against its limits, and the largest of what it is made of, each named by its actor (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figures {
    /// The memory the program holds, as its chassis reads it.
    pub process: Process,
    /// The bytes the blob store holds, against the most it may (ADR-0014).
    pub blobs: Against,
    /// The bytes of every guest's memory, added up: none when no guest runs (ADR-0015).
    pub guest_bytes: u64,
    /// The guests with the most memory, the most first, at most [`LARGEST`].
    pub largest_guests: Vec<GuestMemory>,
    /// The actors holding the most bytes unsent, the most first, at most [`LARGEST`]: the first is the engine's
    /// backlog meter.
    pub longest_backlogs: Vec<Backlog>,
}

/// The memory the program holds, as the operating system or the browser reports it (ADR-0053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Process {
    /// The bytes of the program resident in memory, as the operating system counts them.
    Resident { bytes: u64 },
    /// The bytes of the script heap a browser reports for the page, where it reports one.
    Heap { bytes: u64 },
    /// The chassis reads no figure: a browser that reports none, or a host that has no way to ask.
    Unknown,
}

/// Bytes held against the most that may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Against {
    pub bytes: u64,
    pub most: u64,
}

/// One guest's memory: an instance of one blob's code in one part of the engine, which the actors of that blob seated
/// there share, named by the first of them (ADR-0015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestMemory {
    /// The first actor that runs in it.
    pub actor: ActorName,
    /// The bytes of its memory.
    pub bytes: u64,
    /// How many actors run in it.
    pub actors: u32,
}

/// The bytes one actor holds unsent, against the most it holds, as it last told the engine (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlog {
    pub actor: ActorName,
    pub unsent: Against,
}

impl Figures {
    /// The engine's backlog: what the actor holding the most bytes unsent holds, against its most; nothing against
    /// nothing when no actor holds any.
    #[must_use]
    pub fn backlog(&self) -> Against {
        self.longest_backlogs.first().map_or(Against { bytes: 0, most: 0 }, |longest| longest.unsent)
    }
}
