//! The actors one actor sends to as they come and go: a list it holds, which it watches so the ended leave it
//! (ADR-0016).

use crate::{Ended, ErasedActorRef, Handles, ProtocolRef, Reachable, Table, Turn};
use aethereal_data::ActorId;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// The actors that take the protocol `P` and have asked to be sent to: listeners of any type, in one list, which
/// a turn sends to as one target, `turn.to(&subscribers).send(&mail)` (ADR-0016).
///
/// Each is watched when it is added, so its ending reaches the actor that
/// holds the list, whose handler of endings hands it to [`Self::ended`]. An
/// actor is held once however often it is added, and the list keeps the order
/// they were added in until one leaves, when the last takes its place.
///
/// No watch is undone: one removed before it ends is still watched, and its
/// ending, when it comes, finds nothing to remove.
pub struct Subscribers<P> {
    references: Vec<ProtocolRef<P>>,
    /// Where in `references` each actor is, by its id's bits.
    places: BTreeMap<u64, usize>,
}

impl<P> Subscribers<P> {
    /// No one.
    #[must_use]
    pub const fn new() -> Self {
        Self { references: Vec::new(), places: BTreeMap::new() }
    }

    /// Adds the actor `reference` refers to, and watches it: nothing, and answers false, when it is held already.
    pub fn subscribe<A: Handles<Ended>, S>(&mut self, turn: &mut Turn<'_, A, S>, reference: ProtocolRef<P>) -> bool {
        let is_held = self.places.contains_key(&reference.id().bits());
        if is_held {
            return false;
        }
        self.places.insert(reference.id().bits(), self.references.len());
        self.references.push(reference);
        turn.watch(reference);

        true
    }

    /// Removes the actor `reference` refers to: answers whether it was held.
    pub fn unsubscribe(&mut self, reference: impl Into<ErasedActorRef>) -> bool {
        self.remove(reference.into().id())
    }

    /// Removes the actor that `ended` says has ended: answers whether it was held, and so whether the ending was one
    /// of a subscriber's.
    pub fn ended(&mut self, ended: &Ended) -> bool {
        self.remove(ended.actor().id())
    }

    /// The actors held, to send to.
    #[must_use]
    pub fn references(&self) -> &[ProtocolRef<P>] {
        &self.references
    }

    /// Removes the actor `id` stands for, the last taking its place: answers whether it was held.
    fn remove(&mut self, id: ActorId) -> bool {
        let Some(place) = self.places.remove(&id.bits()) else {
            return false;
        };
        self.references.swap_remove(place);
        if let Some(moved) = self.references.get(place) {
            self.places.insert(moved.id().bits(), place);
        }

        true
    }
}

impl<P> Default for Subscribers<P> {
    fn default() -> Self {
        Self::new()
    }
}

/// The actors held, as one target of a turn's sends.
impl<'r, P> Reachable for &'r Subscribers<P> {
    type Through = ProtocolRef<P>;
    type At = &'r [ProtocolRef<P>];

    fn at(self, _: &dyn Table) -> &'r [ProtocolRef<P>] {
        &self.references
    }
}
