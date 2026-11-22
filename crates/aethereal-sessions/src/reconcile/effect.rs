//! What the reconcile returns: the one next effect, each a single write.

use super::record::Record;
use super::transcript::{Entry, LineId, Role};
use aethereal_actor::Time;

/// The one next thing the hub does for a session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Write the record whole.
    Record(Record),
    /// Append one entry to the transcript.
    Append(Entry),
    /// Ask Docker.
    Docker(Ask),
    /// Write one item to the stdin of the run's Claude Code container, through the hub's attach.
    Stdin(Write),
    /// Nothing until Docker, Claude, the session's callers, or the hub's memory say something new, or the time comes.
    Wait(Until),
}

/// An ask of Docker about a run's container, named by the session, the run, and the role, so it can be repeated: each
/// counts as done when what it asks for already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ask {
    /// Make the container; a Claude Code container begins or resumes the conversation, as `conversation` says.
    Create { run: u32, role: Role, conversation: Begin },
    /// Start it.
    Start { run: u32, role: Role },
    /// Stop it: Docker's grace, then a kill.
    Stop { run: u32, role: Role },
    /// Remove it.
    Remove { run: u32, role: Role },
    /// Attach to the stdin of `run`'s Claude Code container.
    Attach { run: u32 },
}

/// How a Claude Code container takes its conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Begin {
    /// `--session-id`: it begins it.
    Fresh,
    /// `--resume`: it carries it on.
    Resume,
}

/// One item written to Claude's stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Write {
    /// A line, as a stream-json user line whose uuid is its id.
    Line(LineId),
    /// An interrupt of the turn in flight, as a stream-json control request.
    Interrupt,
}

/// Until when nothing needs doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Until {
    /// Until something new is heard.
    Event,
    /// Until something new is heard, or this time, when a deadline passes.
    Time(Time),
}
