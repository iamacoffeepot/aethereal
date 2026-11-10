//! What an actor logged, as the engine's log view keeps it and a turn reads it (ADR-0009, ADR-0011, ADR-0044), and
//! what every actor logged most recently, read without naming one (ADR-0053).

use crate::{Fault, Level, Unreadable};
use aethereal_data::{ActorId, ActorPath};
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

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

/// One entry of the engine's recent log, across actors: what the actor `by` logged, and the step it was folded in
/// (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    by: ActorId,
    step: u64,
    entry: Entry,
}

impl Recorded {
    /// The entry `entry`, made by the actor `by`, folded in the step `step`.
    #[must_use]
    pub const fn new(by: ActorId, step: u64, entry: Entry) -> Self {
        Self { by, step, entry }
    }

    /// The actor that made the entry.
    #[must_use]
    pub const fn by(&self) -> ActorId {
        self.by
    }

    /// The step the entry was folded in.
    #[must_use]
    pub const fn step(&self) -> u64 {
        self.step
    }

    /// What the entry records.
    #[must_use]
    pub const fn entry(&self) -> &Entry {
        &self.entry
    }
}

/// The most recent entries of one part of the log view, across the actors whose seats fall in it, oldest first: a
/// ring that keeps a set number, the oldest leaving as a newer one arrives (ADR-0053).
///
/// Each entry is numbered within the ring, and the ring remembers the number
/// of the first entry of the first step it kept one in, and of each of the
/// latest steps it kept one in, as many as it keeps entries, so a read from a
/// step learns how many entries of that step and after had left before it read
/// them. Those steps reach back at least as far as its oldest entry.
#[derive(Debug, Default)]
pub struct RecentRing {
    entries: VecDeque<Recorded>,
    /// The number of the first entry of the first step the ring kept one in, then of each of the latest steps it kept
    /// one in, at most as many as it keeps entries, oldest first.
    starts: VecDeque<StepStart>,
    /// The number the ring's next entry takes: how many it has been given.
    next: u64,
}

/// The number of the first entry a ring kept in a step.
#[derive(Debug, Clone, Copy)]
struct StepStart {
    step: u64,
    first: u64,
}

impl RecentRing {
    /// Keeps the entry `make` makes, by the actor `by`, in the step `step`, given the oldest entry when keeping it
    /// would hold more than `most`, which then leaves, so a ring kept full reuses what leaves. A ring is given its
    /// entries in the order of their steps. With `most` zero, nothing is kept, and nothing counts as left.
    pub fn keep(&mut self, most: usize, by: ActorId, step: u64, make: impl FnOnce(Option<Entry>) -> Entry) {
        let is_full = self.entries.len() >= most;
        let left = if is_full {
            self.entries.pop_front().map(|recorded| recorded.entry)
        } else {
            None
        };
        let entry = make(left);
        if most > 0 {
            self.entries.push_back(Recorded { by, step, entry });
            self.start(most, step);
        }
        self.next += 1;
    }

    /// Notes the next entry as the first of `step` when it is, forgetting the oldest step but the first past `most`
    /// latest ones.
    fn start(&mut self, most: usize, step: u64) {
        let is_same_step = self.starts.back().is_some_and(|start| start.step == step);
        if is_same_step {
            return;
        }
        if self.starts.len() > most {
            self.starts.remove(1);
        }
        self.starts.push_back(StepStart { step, first: self.next });
    }

    /// The entries kept from the step `since` on, oldest first.
    pub fn since(&self, since: u64) -> impl Iterator<Item = &Recorded> {
        let first = self.entries.partition_point(|recorded| recorded.step < since);

        self.entries.range(first..)
    }

    /// How many entries from the step `since` on had left the ring. When `since` falls between the first step the
    /// ring kept an entry in and the oldest of the latest it remembers, a reader that fell that far behind, they are
    /// counted from that oldest one: at least that many.
    #[must_use]
    pub fn left_since(&self, since: u64) -> u64 {
        let first_kept = self.next - self.entries.len() as u64;
        let started = self.starts.iter().find(|start| start.step >= since);
        let first_since = started.map_or(self.next, |start| start.first);

        first_kept.saturating_sub(first_since)
    }
}

/// What a turn reads of every actor's most recent entries from a step on, as the log view stood when the step began
/// (ADR-0053).
#[derive(Debug)]
pub struct Recent<'a> {
    /// The entries, merged by step, oldest first: within one step, by the part of the view that kept them, then in the
    /// order that part folded them, as the step's turns ran at once and nothing else orders them.
    pub entries: Vec<Named<'a>>,
    /// The step to read on from: the step the read ran in, none of whose entries the view held yet.
    pub next: u64,
    /// How many entries from the step read from had left the view before the read.
    pub left: u64,
}

impl Recent<'_> {
    /// No entries, the read going on from `since`: what a table that reads no log view answers.
    #[must_use]
    pub const fn nothing(since: u64) -> Self {
        Self { entries: Vec::new(), next: since, left: 0 }
    }
}

/// A recent entry, with the name of the actor that made it.
#[derive(Debug)]
pub struct Named<'a> {
    pub actor: ActorName,
    pub recorded: &'a Recorded,
}

/// How a read names the actor that made an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorName {
    /// Its path: the actor is live, or ended recently enough for the table to remember its name.
    Path(ActorPath),
    /// Its id alone: it ended longer ago than the table remembers names.
    Unnamed(ActorId),
}
