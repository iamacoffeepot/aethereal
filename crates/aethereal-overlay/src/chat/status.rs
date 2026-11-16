//! What a session's tab marks it as (ADR-0057: The console is the chat): one status, worked out from what the engine
//! knows of the session, drawn as a small shape in a slot of its own before the tab's title, so the title never moves
//! as the status changes.
//!
//! ```text
//! an ask waits ──> Waiting, over everything else
//! a compaction runs, as the hub lists it or its transcript shows it ──> Compacting
//! the hub's state: Starting ──> Starting;  Working, Waiting ──> Working;  Idle ──> Idle;
//!                  Stopping, Stopped, Archived ──> Lost when the transcript's last word of its container is that it is
//!                  gone, and Stopped otherwise
//! ```

use super::read::Container;
use super::transcript::Compaction;
use aethereal_sessions::State;

/// What a session's tab marks it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Its container is coming up.
    Starting,
    /// It runs, and no turn does.
    Idle,
    /// A turn runs, or a background task of its.
    Working,
    /// It is being compacted, or told it is about to be.
    Compacting,
    /// An ask of its waits on the owner.
    Waiting,
    /// The hub wound it down, as it was idle: the next line resumes it.
    Stopped,
    /// Its container is gone by no word of the hub's: it exited, ran out of memory, was lost, or never started.
    Lost,
}

impl Status {
    /// The status of a session the hub lists in `state` with `asking` asks waiting, whose transcript, as the engine
    /// holds it, shows `compaction` and says last of its container `container`. An ask waiting wins over every other,
    /// as it is the owner's to answer; then a compaction, which runs as a turn does, whichever of the hub and the
    /// transcript says so first.
    pub const fn of(state: State, asking: u32, compaction: Compaction, container: Container) -> Self {
        if asking > 0 {
            return Self::Waiting;
        }
        if matches!(compaction, Compaction::Compacting) {
            return Self::Compacting;
        }

        match (state, container) {
            (State::Starting, _) => Self::Starting,
            (State::Working | State::Waiting, _) => Self::Working,
            (State::Noticed | State::Compacting, _) => Self::Compacting,
            (State::Idle, _) => Self::Idle,
            (State::Stopping | State::Stopped | State::Archived, Container::Gone) => Self::Lost,
            (
                State::Stopping | State::Stopped | State::Archived,
                Container::Unsaid | Container::Started | Container::WoundDown,
            ) => Self::Stopped,
        }
    }

    /// Whether its mark pulses: only while its container comes up or a turn runs, so a tab that moves is one that is
    /// busy.
    pub const fn pulses(self) -> bool {
        match self {
            Self::Starting | Self::Working => true,
            Self::Idle | Self::Compacting | Self::Waiting | Self::Stopped | Self::Lost => false,
        }
    }
}

/// What stands before a tab's title: nothing for the console's own tab, which is the engine's and has no status, and
/// a session's status for each of theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Unmarked,
    Marked(Status),
}

impl Icon {
    /// Whether the tab has a status to mark, so its icon's slot shows.
    pub const fn marks(self) -> bool {
        matches!(self, Self::Marked(_))
    }
}
