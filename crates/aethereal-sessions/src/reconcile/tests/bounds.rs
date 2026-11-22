//! How far the explorer goes.

use crate::reconcile::Limits;
use core::time::Duration;

/// The bounds of one exploration.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bounds {
    pub sessions: usize,
    /// The most sessions awake at once.
    pub cap: usize,
    /// The most lines the owner says to each session.
    pub lines: u32,
    /// The highest run a session reaches.
    pub runs: u32,
    pub hub_crashes: u8,
    /// The most faults of every kind together.
    pub faults: u8,
    /// Which kinds of fault may happen, one bit each, each at most once.
    pub kinds: u32,
    /// The most background tasks Claude starts.
    pub tasks: u8,
    /// Which asks the owner makes of each session besides lines, each at most once, one bit each: [`HIBERNATE`],
    /// [`WAKE`], [`INTERRUPT`], [`STOP`], [`ARCHIVE`].
    pub asks: u8,
    /// Whether every state reached is also stopped and archived by the owner, and shown to settle.
    pub sleeps: bool,
    pub limits: Limits,
}

pub(super) const HIBERNATE: u8 = 1;
pub(super) const WAKE: u8 = 2;
pub(super) const INTERRUPT: u8 = 4;
pub(super) const STOP: u8 = 8;
pub(super) const ARCHIVE: u8 = 16;

/// A kind of fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Fault {
    /// Claude Code exits by itself.
    ClaudeCrash,
    /// Claude Code is killed for its memory.
    ClaudeOutOfMemory,
    /// Docker restarts: every running container exits, every attach ends.
    Daemon,
    /// The hub's attach to a session's stdin ends.
    Drop,
    /// A session's container is removed by hand, its unread output with it.
    Remove,
    /// The session image goes missing, for good.
    NoImage,
    /// Docker refuses to start a container.
    StartRefused,
    /// A session's home loses Claude's conversation.
    Wipe,
    /// The hub stops serving its repository, for good.
    Unserve,
    /// The bootstrap exits 78: a step failed.
    BootstrapFailed,
    /// The bootstrap exits 75: a step ran past its limit.
    BootstrapSlow,
    /// The bootstrap is killed for its memory.
    BootstrapOutOfMemory,
}

impl Fault {
    pub const ALL: [Self; 12] = [
        Self::ClaudeCrash,
        Self::ClaudeOutOfMemory,
        Self::Daemon,
        Self::Drop,
        Self::Remove,
        Self::NoImage,
        Self::StartRefused,
        Self::Wipe,
        Self::Unserve,
        Self::BootstrapFailed,
        Self::BootstrapSlow,
        Self::BootstrapOutOfMemory,
    ];

    pub const fn bit(self) -> u32 {
        1 << self as u32
    }
}

const fn ticks(count: u64) -> Duration {
    Duration::from_nanos(count)
}

/// Every kind of fault.
pub(super) const ALL_FAULTS: u32 = (1 << Fault::ALL.len()) - 1;

impl Bounds {
    /// Every limit one tick: a deadline has passed once the clock has moved since it began.
    pub const SHORT: Limits = Limits {
        bootstrap: ticks(1),
        turn: ticks(1),
        grace: ticks(1),
        notice: ticks(1),
        compaction: ticks(1),
        idle: ticks(1),
    };

    /// One session, one line said to it, two runs, the clock, and nothing else: each test adds what it explores.
    pub const LINE: Self = Self {
        sessions: 1,
        cap: 1,
        lines: 1,
        runs: 2,
        hub_crashes: 0,
        faults: 0,
        kinds: ALL_FAULTS,
        tasks: 0,
        asks: 0,
        sleeps: false,
        limits: Self::SHORT,
    };

    /// The longest limit, in ticks.
    pub fn longest(&self) -> u64 {
        let limits = self.limits;
        let all = [limits.bootstrap, limits.turn, limits.grace, limits.notice, limits.compaction, limits.idle];

        all.iter().map(|limit| u64::try_from(limit.as_nanos()).unwrap_or(u64::MAX)).max().unwrap_or(1)
    }
}
