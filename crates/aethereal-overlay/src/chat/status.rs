//! What a session's tab marks it as (ADR-0057: The console is the chat): one status, worked out from the hub's row of
//! the session, drawn as a small shape in a slot of its own before the tab's title, so the title never moves as the
//! status changes.
//!
//! ```text
//! an ask waits ──> Waiting, over everything else
//! the hub's container state: Starting ──> Starting;  Lost ──> Lost;  Unstarted ──> Unstarted
//! the hub's state: Working, Waiting ──> Working;  Compacting ──> Compacting;  Idle ──> Idle;
//!                  Hibernating, Archived ──> Hibernating
//! ```
//!
//! Every fact is the hub's: it lists a session compacting for every compaction Claude Code runs, the wind-down's, the
//! owner's `/compact`, or one Claude Code began by itself, so the engine works out nothing from the transcript.

use aethereal_sessions::{Connection, State};

/// What a session's tab marks it as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Its container is coming up.
    Starting,
    /// It runs, and no turn does.
    Idle,
    /// A turn runs, or a background task of its.
    Working,
    /// It is being compacted.
    Compacting,
    /// An ask of its waits on the owner.
    Waiting,
    /// It has no container: the hub wound it down, and the next line, or `wake_session`, wakes it.
    Hibernating,
    /// The hub lists its container lost: it ended without the hub ending it, and the next line wakes it.
    Lost,
    /// The hub lists its last start failed: it hibernates, and the next line, or `wake_session`, starts it again.
    Unstarted,
}

impl Status {
    /// The status of a session the hub lists in `state`, its container `connection`, with `asking` asks waiting. An
    /// ask waiting wins over every other, as it is the owner's to answer; then a container starting, lost, or whose
    /// start failed, which the hub's state does not show.
    pub const fn of(state: State, connection: &Connection, asking: u32) -> Self {
        if asking > 0 {
            return Self::Waiting;
        }

        match (state, connection) {
            (_, Connection::Starting) => Self::Starting,
            (_, Connection::Lost) => Self::Lost,
            (_, Connection::Unstarted { .. }) => Self::Unstarted,
            (State::Working | State::Waiting, _) => Self::Working,
            (State::Compacting, _) => Self::Compacting,
            (State::Idle, _) => Self::Idle,
            (State::Hibernating | State::Archived, Connection::None | Connection::Attached) => Self::Hibernating,
        }
    }

    /// Whether its mark pulses: only while its container comes up or a turn runs, so a tab that moves is one that is
    /// busy.
    pub const fn pulses(self) -> bool {
        match self {
            Self::Starting | Self::Working => true,
            Self::Idle | Self::Compacting | Self::Waiting | Self::Hibernating | Self::Lost | Self::Unstarted => false,
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
