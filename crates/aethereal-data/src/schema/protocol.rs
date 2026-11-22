//! The protocol a reference's layout names: its kinds as a set, and for a named protocol what each is answered with
//! (ADR-0016).

use crate::{KindId, Plain};
use alloc::vec::Vec;

/// The protocol a reference's layout names: its kinds, as a set, and for a named protocol what each is answered with
/// (ADR-0016).
///
/// Its kinds are a set, so two protocols listing the same kinds in another order, or one kind twice, are one layout:
/// whatever reads it reads its kinds in [`Ascending`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolShape {
    /// A tuple of kinds, each told: their ids.
    Told(&'static [KindId]),
    /// A named protocol: each kind it takes, with what it is answered with.
    Named(&'static [Answered]),
}

/// One kind of a named protocol, and the kinds it is answered with: none for a tell (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answered {
    /// The kind taken.
    pub kind: KindId,
    /// The kinds it is answered with, as a set.
    pub answers: &'static [KindId],
}

/// The protocol of a flat reference node: its kinds in ascending order of their ids, each once (ADR-0016).
///
/// A decoded schema whose kinds are out of that order is refused, so two nodes of one protocol are equal.
#[derive(Plain, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolNode {
    /// A tuple of kinds, each told: their ids.
    Told(Vec<KindId>),
    /// A named protocol: each kind it takes, with what it is answered with.
    Named(Vec<AnsweredNode>),
}

/// One kind of a named protocol in a flat schema, and the kinds it is answered with, in ascending order (ADR-0016).
#[derive(Plain, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnsweredNode {
    /// The kind taken.
    pub kind: KindId,
    /// The kinds it is answered with, in ascending order of their ids, each once.
    pub answers: Vec<KindId>,
}

impl ProtocolShape {
    /// Whether `other` names one protocol with it: the same kinds, each answered alike, in whatever order either lists
    /// them. What gives two references one node in a flat schema.
    #[must_use]
    pub const fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Told(kinds), Self::Told(other_kinds)) => same_kind_set(kinds, other_kinds),
            (Self::Named(entries), Self::Named(other_entries)) => {
                let mut ours = Ascending::answered(entries);
                let mut theirs = Ascending::answered(other_entries);
                loop {
                    match (ours.next_entry(), theirs.next_entry()) {
                        (None, None) => return true,
                        (Some(entry), Some(other_entry)) => {
                            let same_kind = entry.kind.bits() == other_entry.kind.bits();
                            let same_answers = same_kind_set(entry.answers, other_entry.answers);
                            let alike = same_kind && same_answers;
                            if !alike {
                                return false;
                            }
                        }
                        _ => return false,
                    }
                }
            }
            _ => false,
        }
    }

    /// Its flat form: its kinds, and each one's answers, in ascending order, each once.
    pub(super) fn node(self) -> ProtocolNode {
        match self {
            Self::Told(kinds) => ProtocolNode::Told(ascending_ids(kinds)),
            Self::Named(entries) => {
                let mut order = Ascending::answered(entries);
                let mut flat = Vec::with_capacity(order.count());
                while let Some(entry) = order.next_entry() {
                    flat.push(AnsweredNode { kind: entry.kind, answers: ascending_ids(entry.answers) });
                }

                ProtocolNode::Named(flat)
            }
        }
    }
}

impl ProtocolNode {
    /// Whether its kinds, and each one's answers, are in ascending order, each once: the one order a flat schema holds
    /// them in.
    pub(super) fn is_ascending(&self) -> bool {
        match self {
            Self::Told(kinds) => strictly_ascending(kinds),
            Self::Named(entries) => {
                let kinds_ascend =
                    entries.windows(2).all(|pair| matches!(pair, [first, second] if first.kind < second.kind));
                let answers_ascend = entries.iter().all(|entry| strictly_ascending(&entry.answers));

                kinds_ascend && answers_ascend
            }
        }
    }
}

/// Whether `kinds` and `other` hold the same kinds, in whatever order.
const fn same_kind_set(kinds: &'static [KindId], other: &'static [KindId]) -> bool {
    let mut ours = Ascending::kinds(kinds);
    let mut theirs = Ascending::kinds(other);
    loop {
        match (ours.next_kind(), theirs.next_kind()) {
            (None, None) => return true,
            (Some(kind), Some(other_kind)) => {
                if kind.bits() != other_kind.bits() {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

/// `kinds` in ascending order, each once.
fn ascending_ids(kinds: &'static [KindId]) -> Vec<KindId> {
    let mut order = Ascending::kinds(kinds);
    let mut flat = Vec::with_capacity(order.count());
    while let Some(kind) = order.next_kind() {
        flat.push(kind);
    }

    flat
}

/// Whether each of `kinds` is above the one before it.
fn strictly_ascending(kinds: &[KindId]) -> bool {
    kinds.windows(2).all(|pair| matches!(pair, [first, second] if first < second))
}

/// A set of kinds read in ascending order of their ids, each once: the order a reference's layout names its
/// protocol's kinds in, whatever order they are listed in (ADR-0016).
///
/// A constant cannot sort, so each next kind is found by a pass over the list: the work is the square of its length,
/// and a protocol lists at most 64 kinds.
#[derive(Debug, Clone, Copy)]
pub struct Ascending {
    listed: Listed,
    passed: Passed,
}

/// What an [`Ascending`] reads in order.
#[derive(Debug, Clone, Copy)]
enum Listed {
    /// Kinds.
    Kinds(&'static [KindId]),
    /// A named protocol's entries, by the kind of each.
    Answered(&'static [Answered]),
}

/// How far an [`Ascending`] has read.
#[derive(Debug, Clone, Copy)]
enum Passed {
    /// Nothing yet: the next kind is the least.
    Nothing,
    /// Every kind up to this one: the next is the least above it.
    Through(KindId),
}

impl Ascending {
    /// `kinds`, in ascending order, each once.
    #[must_use]
    pub const fn kinds(kinds: &'static [KindId]) -> Self {
        Self { listed: Listed::Kinds(kinds), passed: Passed::Nothing }
    }

    /// A named protocol's `entries`, in ascending order of their kinds, each kind once.
    #[must_use]
    pub const fn answered(entries: &'static [Answered]) -> Self {
        Self { listed: Listed::Answered(entries), passed: Passed::Nothing }
    }

    /// How many distinct kinds it reads, from where it stands.
    #[must_use]
    pub const fn count(self) -> usize {
        let mut rest = self;
        let mut count = 0;
        while rest.step().is_some() {
            count += 1;
        }

        count
    }

    /// The next kind of a list of kinds, or none when every kind was read or it reads a named protocol's entries.
    pub const fn next_kind(&mut self) -> Option<KindId> {
        match (self.listed, self.step()) {
            (Listed::Kinds(kinds), Some(index)) => Some(kinds[index]),
            _ => None,
        }
    }

    /// The next entry of a named protocol, or none when every kind was read or it reads a list of kinds.
    pub const fn next_entry(&mut self) -> Option<Answered> {
        match (self.listed, self.step()) {
            (Listed::Answered(entries), Some(index)) => Some(entries[index]),
            _ => None,
        }
    }

    /// The place of the least kind above every kind read so far, which it reads: none when there is none.
    const fn step(&mut self) -> Option<usize> {
        let mut least: Option<usize> = None;
        let mut index = 0;
        while index < self.len() {
            let kind = self.kind_at(index);
            let above = match self.passed {
                Passed::Nothing => true,
                Passed::Through(passed) => kind.bits() > passed.bits(),
            };
            let lower = match least {
                Some(found) => kind.bits() < self.kind_at(found).bits(),
                None => true,
            };
            if above && lower {
                least = Some(index);
            }
            index += 1;
        }

        if let Some(found) = least {
            self.passed = Passed::Through(self.kind_at(found));
        }

        least
    }

    /// How many entries the list holds, repeats included.
    const fn len(&self) -> usize {
        match self.listed {
            Listed::Kinds(kinds) => kinds.len(),
            Listed::Answered(entries) => entries.len(),
        }
    }

    /// The kind of the entry at `index`, which is below [`Self::len`].
    const fn kind_at(&self, index: usize) -> KindId {
        match self.listed {
            Listed::Kinds(kinds) => kinds[index],
            Listed::Answered(entries) => entries[index].kind,
        }
    }
}
