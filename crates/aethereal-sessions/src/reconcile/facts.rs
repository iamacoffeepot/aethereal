//! What the reconcile is given: the record, the transcript as its ledger reads it, Docker's state of the one container
//! the record names, what the hub holds in memory of that container's stdin, whether the hub's repository is served,
//! the time, and the deadlines.

use super::effect::Write;
use super::ledger::Ledger;
use super::record::{Record, Refusal};
use super::transcript::Output;
use aethereal_actor::Time;
use core::time::Duration;

/// Everything one call of the reconcile reads.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub record: &'a Record,
    pub ledger: &'a Ledger,
    /// The container the record's phase names, as Docker shows it: absent when it names none.
    pub container: Container<'a>,
    pub attachment: &'a Attachment,
    /// Whether the hub serves a repository for the session to clone.
    pub served: bool,
    pub now: Time,
    pub limits: &'a Limits,
}

/// The one container a record's phase names, as Docker shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Container<'a> {
    pub seen: Seen,
    /// The first line of its log that the transcript does not hold yet.
    pub unread: Unread<'a>,
}

/// A container's state, as Docker answered the hub's last ask or inspect of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Seen {
    /// No container of that name exists.
    Absent,
    /// It was made and never started.
    Created,
    /// It runs, since `started`, Docker's own time.
    Running { started: Time },
    /// It exited with `code`, killed for its memory when `oom`, at `finished`, Docker's own time.
    Exited { code: i32, oom: bool, finished: Time },
    /// Docker refused the hub's last ask of it, to make or to start it, so: nothing has been asked of it since.
    Refused(Refusal),
}

/// Whether a container's log holds a line the transcript does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread<'a> {
    /// The transcript holds all of it.
    Nothing,
    /// This line is the first it does not hold.
    Next(&'a Output),
}

/// What the hub holds in memory of a Claude Code container's stdin: lost when the hub stops, or when its attach ends.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Attachment {
    /// No attach.
    Detached,
    /// The hub appended the marker numbered `marker` for `run`, and attaches next.
    Marked { run: u32, marker: u64 },
    /// The hub is attached to `run`'s stdin, under the marker numbered `marker`, and has written `wrote` through it.
    Open { run: u32, marker: u64, wrote: Vec<Write> },
}

/// How long each wait may last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// A bootstrap container, from its start.
    pub bootstrap: Duration,
    /// A turn, or a background task, from its start.
    pub turn: Duration,
    /// A turn past its deadline, from its interrupt.
    pub grace: Duration,
    /// The turn that answers a wind-down's notice, from the notice.
    pub notice: Duration,
    /// A wind-down's compaction, from its `/compact`.
    pub compaction: Duration,
    /// An awake session that nothing runs in, until it winds down.
    pub idle: Duration,
}
