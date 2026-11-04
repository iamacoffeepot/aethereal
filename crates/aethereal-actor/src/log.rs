//! What an actor logged, as the engine's log view keeps it and a turn reads it (ADR-0009, ADR-0011, ADR-0044).

use crate::{Fault, Level, Unreadable};
use alloc::collections::VecDeque;
use alloc::string::String;

/// One entry in an actor's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A line the actor logged.
    Line { level: Level, text: String },
    /// A turn of the actor's returned this fault, and the actor ended.
    Failed(Fault),
    /// The actor's closing turn returned this fault.
    EndFailed(Fault),
    /// A resource of this name, given by the actor or given to it, was dropped on its way: no mail carried it, its
    /// mail did not arrive, or the actor did not take it in the turn of the mail it came with (ADR-0010).
    Dropped(&'static str),
    /// What a source brought the actor, of the kind this names, could not be written as mail, and was dropped
    /// (ADR-0010).
    Unbrought(&'static str),
    /// A job the actor ran returned this fault, or its host lost it to a panic, and the actor was told it went
    /// unanswered (ADR-0018).
    JobFaulted(Fault),
    /// Mail the actor was handed did not read as its kind, for this reason, so no handler ran: it was dropped, or, a
    /// request, answered `Unreadable` (ADR-0017).
    Unread(Unreadable),
}

/// One actor's log as the view keeps it: its last entries, oldest first, each numbered within the actor from zero in
/// the order it made them (ADR-0044).
#[derive(Debug, Default)]
pub struct KeptLog {
    entries: VecDeque<Entry>,
    /// The number the actor's next entry takes: how many it has made.
    next: u64,
}

impl KeptLog {
    /// Keeps the entry `make` makes, given the oldest entry when keeping it would hold more than `most`, which then
    /// leaves, so a log kept full reuses what leaves. With `most` zero, nothing is kept, and the entry is still
    /// numbered.
    pub fn keep(&mut self, most: usize, make: impl FnOnce(Option<Entry>) -> Entry) {
        let is_full = self.entries.len() >= most;
        let left = if is_full {
            self.entries.pop_front()
        } else {
            None
        };
        let entry = make(left);
        if most > 0 {
            self.entries.push_back(entry);
        }
        self.next += 1;
    }

    /// The entries kept, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// The number of the oldest entry kept, or [`Self::next`] when none is.
    #[must_use]
    pub fn first(&self) -> u64 {
        self.next - self.entries.len() as u64
    }

    /// The number the actor's next entry takes.
    #[must_use]
    pub const fn next(&self) -> u64 {
        self.next
    }
}

/// An actor's log as a turn reads it, as the view stood when the step began (ADR-0044).
#[derive(Clone, Copy)]
pub enum Logged<'a> {
    /// The actor has logged nothing the view holds: none, or its seat has since been taken by another that logged.
    Nothing,
    /// What the view holds of its log.
    Kept(&'a KeptLog),
}

impl<'a> Logged<'a> {
    /// The entries numbered `since` and after, oldest first, each with its number.
    pub fn since(self, since: u64) -> impl Iterator<Item = (u64, &'a Entry)> {
        let kept = match self {
            Self::Nothing => None,
            Self::Kept(kept) => Some(kept),
        };
        let numbered = kept.into_iter().flat_map(|kept| (kept.first()..).zip(kept.entries()));

        numbered.filter(move |(number, _)| *number >= since)
    }

    /// The number of the oldest entry the view holds.
    #[must_use]
    pub fn first(self) -> u64 {
        match self {
            Self::Nothing => 0,
            Self::Kept(kept) => kept.first(),
        }
    }

    /// The number the actor's next entry takes.
    #[must_use]
    pub const fn next(self) -> u64 {
        match self {
            Self::Nothing => 0,
            Self::Kept(kept) => kept.next(),
        }
    }
}
