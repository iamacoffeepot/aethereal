//! The transcript as the reconcile reads it: every entry the hub appends, with its time.

use aethereal_actor::Time;

/// One entry of a session's transcript, as the hub appends it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry {
    /// When it happened: Docker's time for a line of a container's output, the hub's for the rest.
    pub at: Time,
    pub what: What,
}

/// What one entry of a transcript says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum What {
    /// A line said to the session, written to Claude's stdin with its id as its uuid.
    Said(LineId),
    /// A line of the output of `run`'s container `role`, copied from Docker's log.
    Output { run: u32, role: Role, event: Event },
    /// From here on the hub may write to the stdin of `run`'s Claude Code container: appended before the first write of
    /// each attach, so a hub that finds it and holds no attach of its own knows a write of another may have been cut
    /// short.
    Attached { run: u32 },
    /// The hub's interrupt of the turn in `run` that the entry numbered `turn` began, appended before it is written, so
    /// it is written once and its grace runs from here.
    Interrupted { run: u32, turn: u64 },
    /// `run`'s container `role` was gone when the hub came back to it: what it wrote after the entries before this one
    /// is lost.
    Gone { run: u32, role: Role },
}

/// The id of a line said to a session: the uuid it is written to Claude with, which Claude echoes in its receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LineId {
    /// A line a caller said, by the id it carries from where it was typed, so a caller that says it again is answered
    /// with the same entry (D1).
    Owner(u128),
    /// The hub's line telling Claude that its container of `run` ended without warning.
    Lost { run: u32 },
    /// The hub's notice of the `nth` wind-down in `run`: it is about to be compacted and hibernated.
    Notice { run: u32, nth: u32 },
    /// The hub's `/compact` of the `nth` wind-down in `run`.
    Compact { run: u32, nth: u32 },
}

/// Which of a run's two containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The bootstrap container, which readies the session's work tree and exits.
    Bootstrap,
    /// Claude Code's container.
    Claude,
}

/// A line of a container's output as Docker's log holds it: when Docker took it, and the event of Claude's it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Output {
    pub at: Time,
    pub event: Event,
}

/// The events of Claude's stream-json output the reconcile reads; every other line is kept for people alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Event {
    /// Claude read the line: its echo with `isReplay`. The line is in Claude's conversation by the time it prints.
    Receipt(LineId),
    /// A turn began: `system` `init`.
    Init,
    /// A turn ended, however: done, failed, or interrupted.
    Result,
    /// A background task began.
    TaskStarted(TaskId),
    /// A background task ended: its `task_notification`.
    TaskEnded(TaskId),
    /// Claude's status is compacting.
    Compacting,
    /// A compaction finished: `compact_boundary`.
    Compacted,
    /// The typed result `error_during_execution`, before any receipt, of a `--resume` that found no conversation.
    ResumeFailed,
}

/// The id of a background task of Claude's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub String);
