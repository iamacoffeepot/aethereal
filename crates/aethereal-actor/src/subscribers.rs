//! The actors one actor sends to as they come and go: a list it holds, which it watches so the ended leave it
//! (ADR-0016).

use crate::{Ended, ErasedActorRef, Handles, ProtocolRef, Reachable, Turn, Vantage};
use aethereal_data::{ActorId, Boundary, Decode, DecodeError, Encode, EncodeError, Reader, Schema, Shape, Writer};
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
///
/// It is a field of its holder's state, written as the references it holds,
/// in order, so an actor that keeps its listeners is still a kind (ADR-0009).
/// The watches are the engine's, and are not written.
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
        let added = self.hold(reference);
        if added {
            turn.watch(reference);
        }

        added
    }

    /// Adds the actor `reference` refers to, unwatched: nothing, and answers false, when it is held already.
    fn hold(&mut self, reference: ProtocolRef<P>) -> bool {
        let is_held = self.places.contains_key(&reference.id().bits());
        if is_held {
            return false;
        }
        self.places.insert(reference.id().bits(), self.references.len());
        self.references.push(reference);

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

/// Written as the list of references it holds.
impl<P> Schema for Subscribers<P> {
    const SHAPE: Shape = <Vec<ProtocolRef<P>>>::SHAPE;
}

impl<B: Boundary, P> Encode<B> for Subscribers<P>
where
    Vec<ProtocolRef<P>>: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.references.encode(out)
    }
}

/// Read as the list of references it was written as, each found again where it is held, and one written twice held
/// once.
impl<B: Boundary, P> Decode<B> for Subscribers<P>
where
    Vec<ProtocolRef<P>>: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let mut subscribers = Self::new();
        for reference in Vec::<ProtocolRef<P>>::decode(input)? {
            subscribers.hold(reference);
        }

        Ok(subscribers)
    }
}

/// The actors held, as one target of a turn's sends.
impl<'r, Me, P> Reachable<Me> for &'r Subscribers<P> {
    type Through = ProtocolRef<P>;
    type At = &'r [ProtocolRef<P>];

    fn at(self, _: &Vantage<'_, '_>) -> &'r [ProtocolRef<P>] {
        &self.references
    }
}
