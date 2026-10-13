//! A tree of slots indexed by the bits of a number, each written once (ADR-0008).

use super::actor::Actor;
use std::sync::{Arc, OnceLock};

/// How many child slots a node has: one for each value of four bits of a key.
const SLOTS: usize = 16;

/// One node of a tree indexed by a key's bits, four at a time from the most significant.
///
/// A node holds one actor and a child for each value of the next four bits.
/// Every slot is written once and nothing written is moved or removed, so a
/// reader walks the tree while a writer adds to it and the reader never
/// waits. It points at actors and owns none of what they are.
///
/// Two keys that agree on all sixteen runs of four bits are one key, so a walk
/// meets its key or an empty slot by the seventeenth node.
#[derive(Default)]
pub(super) struct Radix {
    /// The actor kept at this node; empty until the first key to reach the node is written.
    held: OnceLock<Held>,
    /// The node beneath this one for each value of the next four bits; empty until a key goes that way.
    children: [OnceLock<Box<Self>>; SLOTS],
}

/// An actor, and the key it was placed under.
struct Held {
    key: u64,
    actor: Arc<Actor>,
}

impl Radix {
    /// The actor placed under `key`, when the tree beneath this node holds one.
    pub(super) fn find(&self, key: u64) -> Option<&Arc<Actor>> {
        let mut node = self;
        let mut slots = Slots(key);
        loop {
            let held = node.held.get()?;
            if held.key == key {
                return Some(&held.actor);
            }
            node = node.children[slots.next()].get()?;
        }
    }

    /// Places `actor` under `key`, at the first empty node on the key's way down.
    ///
    /// Refused with the actor already under `key`, when there is one.
    pub(super) fn place(&self, key: u64, actor: &Arc<Actor>) -> Result<(), &Arc<Actor>> {
        let mut node = self;
        let mut slots = Slots(key);
        loop {
            match node.held.get() {
                Some(held) if held.key == key => return Err(&held.actor),
                Some(_) => node = node.children[slots.next()].get_or_init(Box::default),
                // Another writer may fill the node first; the node is then read again.
                None => {
                    let placed = node.held.set(Held { key, actor: Arc::clone(actor) }).is_ok();
                    if placed {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Every actor placed in the tree beneath this node, in no stated order.
    pub(super) fn all(&self) -> Vec<Arc<Actor>> {
        let mut found = Vec::new();
        let mut unwalked = vec![self];
        while let Some(node) = unwalked.pop() {
            found.extend(node.held.get().map(|held| Arc::clone(&held.actor)));
            unwalked.extend(node.children.iter().filter_map(|child| child.get().map(Box::as_ref)));
        }

        found
    }
}

/// The child slots a key passes through on its way down, most significant four bits first.
struct Slots(u64);

impl Slots {
    /// The next slot down.
    const fn next(&mut self) -> usize {
        #[allow(clippy::cast_possible_truncation, reason = "the top four bits of a u64 fit in any usize")]
        let slot = (self.0 >> 60) as usize;
        self.0 <<= 4;

        slot
    }
}
