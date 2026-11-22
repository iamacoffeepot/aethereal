//! A session as it is listed (ADR-0057: A session is a record, and its container the connection): its conversation's
//! state, its container's state beneath it, when its transcript last grew, and how many of its asks wait. The same row
//! is what the hub's tools list and what a trusted engine is shown.

#[cfg(test)]
mod tests;

use aethereal_actor::Time;
use aethereal_data::Plain;

/// One session as it is listed.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.session_row")]
pub struct SessionRow {
    /// Its number, which every ask names it by.
    pub session: u32,
    pub name: String,
    pub state: State,
    /// Its container's state: a detail beneath `state`.
    pub container: Connection,
    pub grew: Grew,
    /// How many of its asks wait for the owner's answer.
    pub asking: u32,
}

/// Where a session's conversation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.state")]
pub enum State {
    /// Nothing runs in it, and it waits for a line.
    Idle,
    /// A turn runs, the notice's turn as it is wound down among them, or a background task of its does.
    Working,
    /// A turn waits on the owner's answer to an ask.
    Waiting,
    /// It is being compacted.
    Compacting,
    /// It has no container, and is ready to be picked up again.
    Hibernating,
    /// It never wakes again, and its record stays readable.
    Archived,
}

/// Where a session's container is: a detail beneath its conversation's state. A session is awake from its container's
/// `Starting` until Docker has removed it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.connection")]
pub enum Connection {
    /// It has no container.
    None,
    /// Its record saying so is written, then its volumes are made, then its bootstrap container is made and runs until
    /// it ends, and, once it exited 0, its Claude Code container is made and started.
    Starting,
    /// Its container is there, and runs until the hub has stopped and removed it.
    Attached,
    /// Its last container ended without the hub ending it: its process exited, the kernel killed it for its memory, or
    /// it was gone when the hub came back to it. It shows so until it wakes, so the owner knows its last turn may have
    /// been cut short.
    Lost,
    /// It has no container, as its last start failed before Claude Code ran, as `why` says, and it holds `holding`
    /// lines that waited for that start, which its next is written first. It starts again only as the owner asks, as a
    /// line or a wake does.
    Unstarted { why: String, holding: u32 },
}

/// When a session's transcript last grew, on the clock of whoever holds the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.grew")]
pub enum Grew {
    /// Not since the hub began: its transcript is as the hub read it, or empty.
    Unseen,
    /// At this moment.
    At(Time),
}

impl Grew {
    /// This time, held at `from` on one clock, as the moment as long before `to` on another: how a row's time crosses
    /// from the hub's clock to an engine's, as two clocks are their own (ADR-0041).
    #[must_use]
    pub fn rebased(self, from: Time, to: Time) -> Self {
        match self {
            Self::Unseen => Self::Unseen,
            Self::At(at) => Self::At(to.saturating_sub(from.saturating_duration_since(at))),
        }
    }
}
