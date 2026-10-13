//! The tree the table's lines are kept in (ADR-0008).

use super::Refused;
use super::entry::Entry;
use aethereal_data::ActorId;
use std::sync::OnceLock;

/// How many child slots a node has: one for each value of four bits of an id.
const SLOTS: usize = 16;

/// One node of a tree indexed by an id's bits, four at a time from the most significant.
///
/// A node holds one line and a child for each value of the next four bits.
/// Every slot is written once and nothing written is moved or removed, so a
/// reader walks the tree while a writer adds to it and neither waits. Each
/// slot is a `OnceLock` for that reason: the tree is shared by every thread
/// that runs a turn, and belongs to no actor.
///
/// Two ids that agree on all sixteen runs of four bits are one id, so a walk
/// meets its id or an empty slot by the seventeenth node.
#[derive(Default)]
pub(super) struct Node {
    /// The line kept at this node; empty until the first id to reach the node is written.
    entry: OnceLock<Entry>,
    /// The node beneath this one for each value of the next four bits; empty until an id goes that way.
    children: [OnceLock<Box<Self>>; SLOTS],
}

impl Node {
    /// The line of `id`, when the tree beneath this node holds it.
    pub(super) fn find(&self, id: ActorId) -> Option<&Entry> {
        let mut node = self;
        let mut slots = Slots::of(id);
        loop {
            let held = node.entry.get()?;
            if held.id() == id {
                return Some(held);
            }
            node = node.children[slots.next()].get()?;
        }
    }

    /// Writes `entry` at the first empty node on its id's way down. Refused when the tree already holds its id.
    pub(super) fn place(&self, entry: Entry) -> Result<(), Refused> {
        let mut node = self;
        let mut slots = Slots::of(entry.id());
        let mut entry = entry;
        loop {
            match node.entry.get() {
                Some(held) if held.id() == entry.id() => return Err(held.refusal(entry.path())),
                Some(_) => node = node.children[slots.next()].get_or_init(Box::default),
                // Another writer may fill the node first; the line then comes back, and the node is read again.
                None => match node.entry.set(entry) {
                    Ok(()) => return Ok(()),
                    Err(unplaced) => entry = unplaced,
                },
            }
        }
    }
}

/// The child slots an id passes through on its way down, most significant four bits first.
struct Slots(u64);

impl Slots {
    /// The slots of `id`, from the root down.
    const fn of(id: ActorId) -> Self {
        Self(id.bits())
    }

    /// The next slot down.
    const fn next(&mut self) -> usize {
        #[allow(clippy::cast_possible_truncation, reason = "the top four bits of a u64 fit in any usize")]
        let slot = (self.0 >> 60) as usize;
        self.0 <<= 4;

        slot
    }
}
