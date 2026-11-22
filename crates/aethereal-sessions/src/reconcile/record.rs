//! A session's record as the reconcile reads and writes it: what the session should be, which of its run's containers
//! may exist and whether the hub is stopping it and why, how its latest run ended, whether Claude's conversation has
//! begun, and how far its lines are held. It holds no line and counts none: the transcript owns lines.

use super::transcript::Role;

/// A session's record, written whole, before the effect it allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Record {
    pub desired: Desired,
    /// Its latest run, numbered from 1: 0 before its first.
    pub run: u32,
    pub phase: Phase,
    pub last: Last,
    pub begun: Begun,
    /// The number of the transcript entry its lines are held through: a line said at or before it waits for a wake,
    /// and an owner's line after it wakes a hibernating session.
    pub held_through: u64,
}

impl Record {
    /// The record of a new session: awake, its first run to start, its conversation not begun.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            desired: Desired::Awake,
            run: 0,
            phase: Phase::Off,
            last: Last::Unrun,
            begun: Begun::No,
            held_through: 0,
        }
    }
}

impl Default for Record {
    fn default() -> Self {
        Self::new()
    }
}

/// What the session should be; the reconcile makes Docker match it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Desired {
    /// Running, its Claude Code container attached.
    Awake,
    /// Hibernating, its last container removed, as soon as `When` allows.
    Hibernating(When),
    /// Archived: it never wakes again.
    Archived,
}

/// When a session asked to hibernate stops its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum When {
    /// Once no turn or task runs in it, after its wind-down (D8).
    AfterTurn,
    /// At once, whatever runs in it: `stop_session` (D5).
    Now,
}

/// Which container of its run a session may have, and where that container is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// None: no container of it exists, nor may.
    Off,
    /// Its run's bootstrap container.
    Bootstrap(Run),
    /// Its run's Claude Code container.
    Claude(Run),
}

impl Phase {
    /// The container it names, and where that container is: none when it is off.
    #[must_use]
    pub const fn container(&self) -> Option<(Role, Run)> {
        match self {
            Self::Off => None,
            Self::Bootstrap(state) => Some((Role::Bootstrap, *state)),
            Self::Claude(state) => Some((Role::Claude, *state)),
        }
    }
}

/// Where the container a phase names is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Run {
    /// It may be made, started, and running.
    Running,
    /// The hub is stopping it, for this reason: written before the stop is asked.
    Stopping(Why),
    /// It exited, and how is in the record: written before it is removed.
    Exited,
}

/// Why the hub stops a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Why {
    /// The session hibernates.
    Hibernate,
    /// The session is archived.
    Archive,
    /// The owner stopped it (D5).
    Owner,
    /// It ran past a deadline.
    Overdue(Overdue),
    /// The hub lost its attach to the container's stdin, which may hold a line cut short: the next run writes the rest.
    Reattach,
}

/// Which deadline a container ran past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overdue {
    /// The bootstrap's.
    Bootstrap,
    /// A turn's, or a background task's.
    Turn,
}

/// How the latest finished run of a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Last {
    /// No run has ended.
    Unrun,
    /// Run `run` ended so.
    Ended { run: u32, how: How },
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum How {
    /// The hub stopped it, for this reason.
    Stopped(Why),
    /// Its Claude Code container ended without the hub asking.
    Lost,
    /// It never got as far as Claude Code reading a line, for this reason.
    Unstarted(Reason),
}

impl How {
    /// Whether it is a failure: the session hibernates, and nothing starts it again but a wake or a newer line.
    #[must_use]
    pub const fn fails(self) -> bool {
        matches!(self, Self::Lost | Self::Unstarted(_) | Self::Stopped(Why::Overdue(_)))
    }
}

/// Why a run never got as far as Claude Code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The hub serves no repository for it to clone.
    Repository,
    /// Docker refused to make or start a container.
    Refused(Refusal),
    /// The bootstrap exited with this code.
    Bootstrap { code: i32 },
    /// The bootstrap was killed for its memory.
    BootstrapOutOfMemory,
    /// The bootstrap ran past its deadline.
    BootstrapOverdue,
    /// The bootstrap container was gone when the hub came back to it.
    Gone,
    /// `--resume` found no conversation (D7).
    Unresumable,
    /// `--session-id` found the conversation already begun (D2).
    InUse,
}

/// Docker's typed refusal of an ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The session image is missing.
    NoImage,
    /// The container could not be made.
    Create,
    /// The container could not be started.
    Start,
}

/// Whether Claude's conversation has begun, as Claude's own answers say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Begun {
    /// Not yet: the next Claude Code container starts it with `--session-id`.
    No,
    /// It has: the next resumes it with `--resume`.
    Yes,
}
