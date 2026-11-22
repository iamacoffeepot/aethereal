//! The protocol a reference's layout names: its kinds as a set, and for a named protocol what each is answered with
//! (ADR-0016).

use crate::kind::kind_id;
use crate::{KindId, KindPtr, Plain};
use alloc::vec::Vec;
use core::slice;

/// The most kinds a protocol lists: a tuple of kinds holds at most 64.
pub const MOST_LISTED: usize = 64;

/// The protocol a reference's layout names: its kinds, as a set, and for a named protocol what each is answered with
/// (ADR-0016).
///
/// Its kinds are reached by their nodes, not their ids, so a kind may hold a reference to a protocol that takes it: the
/// ids are computed from the graph the nodes make (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolShape {
    /// A tuple of kinds, each told.
    Told(&'static [KindPtr]),
    /// A named protocol: each kind it takes, with what it is answered with.
    Named(&'static [Answered]),
}

/// One kind of a named protocol, and the kinds it is answered with: none for a tell (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Answered {
    /// The kind taken.
    pub kind: KindPtr,
    /// The kinds it is answered with, as a set.
    pub answers: &'static [KindPtr],
}

/// A kind a flat reference names: by its id, or, inside the description of a cycle, by the member it is (ADR-0006).
///
/// Its order is the order a flat protocol holds them in: ids ascending, then members ascending.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Referent {
    /// A kind outside the cycle described, or any kind where no cycle is: its id.
    Kind(KindId),
    /// The member at this place in the list of a cycle's members.
    Member(u32),
}

/// The protocol of a flat reference node: its kinds in ascending order, each once (ADR-0016).
///
/// A decoded schema whose kinds are out of that order is refused, so two nodes of one protocol are equal.
#[derive(Plain, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProtocolNode {
    /// A tuple of kinds, each told.
    Told(Vec<Referent>),
    /// A named protocol: each kind it takes, with what it is answered with.
    Named(Vec<AnsweredNode>),
}

/// One kind of a named protocol in a flat schema, and the kinds it is answered with, in ascending order (ADR-0016).
#[derive(Plain, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnsweredNode {
    /// The kind taken.
    pub kind: Referent,
    /// The kinds it is answered with, in ascending order, each once.
    pub answers: Vec<Referent>,
}

/// A set of kinds by their ids, ascending, each once: what a constant writes a protocol's kinds as where no cycle is
/// being hashed, as a state's section does.
#[derive(Clone, Copy)]
pub struct KindIds {
    ids: [KindId; MOST_LISTED],
    len: usize,
}

impl KindIds {
    /// No kinds.
    const EMPTY: Self = Self { ids: [KindId::NONE; MOST_LISTED], len: 0 };

    /// The ids of `kinds`, ascending, each once. Each id is computed from its kind's graph.
    #[must_use]
    pub const fn of(kinds: &[KindPtr]) -> Self {
        let mut set = Self::EMPTY;
        let mut index = 0;
        while index < kinds.len() {
            set.insert(kind_id(kinds[index]));
            index += 1;
        }

        set
    }

    /// Adds `id` in its place, unless it is held.
    const fn insert(&mut self, id: KindId) {
        let mut at = 0;
        while at < self.len && self.ids[at].bits() < id.bits() {
            at += 1;
        }
        let held = at < self.len && self.ids[at].bits() == id.bits();
        if held {
            return;
        }

        assert!(self.len < MOST_LISTED, "a protocol lists at most 64 kinds");
        let mut index = self.len;
        while index > at {
            self.ids[index] = self.ids[index - 1];
            index -= 1;
        }
        self.ids[at] = id;
        self.len += 1;
    }

    /// The ids, ascending.
    #[must_use]
    pub const fn ids(&self) -> &[KindId] {
        self.ids.split_at(self.len).0
    }

    /// Whether `other` holds the same ids.
    #[must_use]
    pub const fn same(&self, other: &Self) -> bool {
        same_ids(self.ids(), other.ids())
    }
}

/// A named protocol's entries by their kinds' ids, ascending, each kind once with the answer of its first entry.
#[derive(Clone, Copy)]
pub struct AnsweredIds {
    kinds: KindIds,
    answers: [AnswerId; MOST_LISTED],
}

/// What one kind of a named protocol is answered with, by id: a handler answers with one kind or none (ADR-0059).
#[derive(Clone, Copy)]
enum AnswerId {
    /// It is a tell.
    Nothing,
    /// It is answered with this kind.
    One(KindId),
}

impl AnsweredIds {
    /// The entries of `entries`, ordered by their kinds' ids.
    ///
    /// # Panics
    ///
    /// When `entries` lists more than 64 kinds, or answers one with more than one kind: in a constant, the build stops.
    #[must_use]
    pub const fn of(entries: &[Answered]) -> Self {
        assert!(entries.len() <= MOST_LISTED, "a protocol lists at most 64 kinds");
        let mut ids = [KindId::NONE; MOST_LISTED];
        let mut kinds = KindIds::EMPTY;
        let mut index = 0;
        while index < entries.len() {
            ids[index] = kind_id(entries[index].kind);
            kinds.insert(ids[index]);
            index += 1;
        }

        let mut answers = [AnswerId::Nothing; MOST_LISTED];
        let mut place = 0;
        while place < kinds.len {
            // The first entry of the kind at `place`: a kind listed twice keeps the answer of its first entry.
            let mut entry = 0;
            while ids[entry].bits() != kinds.ids[place].bits() {
                entry += 1;
            }
            answers[place] = match entries[entry].answer() {
                Some(answer) => AnswerId::One(kind_id(answer)),
                None => AnswerId::Nothing,
            };
            place += 1;
        }

        Self { kinds, answers }
    }

    /// How many distinct kinds it holds.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.kinds.len
    }

    /// The kind at `place`, and the kinds it is answered with, as a set: one kind or none.
    #[must_use]
    pub const fn entry(&self, place: usize) -> (KindId, &[KindId]) {
        let answers = match &self.answers[place] {
            AnswerId::Nothing => &[],
            AnswerId::One(id) => slice::from_ref(id),
        };

        (self.kinds.ids[place], answers)
    }
}

impl Answered {
    /// The kind it is answered with, if it is answered: a handler answers with one kind or none (ADR-0059), so a shape
    /// that lists more stops the build where it is read.
    #[must_use]
    pub(crate) const fn answer(&self) -> Option<KindPtr> {
        match self.answers {
            [] => None,
            [answer] => Some(*answer),
            [..] => panic!("a kind of a named protocol is answered with one kind or none"),
        }
    }
}

/// A member's place in a cycle's list, which holds at most as many members as a graph holds kinds.
#[expect(clippy::cast_possible_truncation, reason = "a graph holds at most 128 kinds, so a place fits")]
const fn place(listed: usize) -> u32 {
    listed as u32
}

/// Whether two ascending sets of ids are one.
const fn same_ids(one: &[KindId], other: &[KindId]) -> bool {
    if one.len() != other.len() {
        return false;
    }

    let mut index = 0;
    while index < one.len() {
        if one[index].bits() != other[index].bits() {
            return false;
        }
        index += 1;
    }

    true
}

impl ProtocolShape {
    /// Whether `other` names one protocol with it: the same kinds, each answered alike, in whatever order either lists
    /// them. What gives two references one node in a flat schema.
    #[must_use]
    pub const fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Told(kinds), Self::Told(other_kinds)) => KindIds::of(kinds).same(&KindIds::of(other_kinds)),
            (Self::Named(entries), Self::Named(other_entries)) => {
                let ours = AnsweredIds::of(entries);
                let theirs = AnsweredIds::of(other_entries);
                if ours.count() != theirs.count() {
                    return false;
                }
                let mut place = 0;
                while place < ours.count() {
                    let (kind, answers) = ours.entry(place);
                    let (other_kind, other_answers) = theirs.entry(place);
                    let alike = kind.bits() == other_kind.bits() && same_ids(answers, other_answers);
                    if !alike {
                        return false;
                    }
                    place += 1;
                }

                true
            }
            _ => false,
        }
    }

    /// Its flat form, each kind named as [`Referent::Member`] when it is one of `members`, by its place there, and by
    /// its id otherwise; ascending, each once.
    pub(super) fn node_in(self, members: &[KindPtr]) -> ProtocolNode {
        let referent = |kind: KindPtr| {
            let listed = members.iter().position(|member| member.is(kind));
            listed.map_or_else(|| Referent::Kind(kind_id(kind)), |listed| Referent::Member(place(listed)))
        };
        let set = |kinds: &[KindPtr]| {
            let mut flat: Vec<Referent> = kinds.iter().map(|kind| referent(*kind)).collect();
            flat.sort_unstable();
            flat.dedup();
            flat
        };
        match self {
            Self::Told(kinds) => ProtocolNode::Told(set(kinds)),
            Self::Named(entries) => {
                let mut flat: Vec<AnsweredNode> = Vec::new();
                for entry in entries {
                    let kind = referent(entry.kind);
                    // A kind listed twice keeps the answers of its first entry.
                    let first = flat.iter().all(|held| held.kind != kind);
                    if first {
                        flat.push(AnsweredNode { kind, answers: set(entry.answers) });
                    }
                }
                flat.sort_unstable();

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

/// Whether each of `kinds` is above the one before it.
fn strictly_ascending(kinds: &[Referent]) -> bool {
    kinds.windows(2).all(|pair| matches!(pair, [first, second] if first < second))
}
