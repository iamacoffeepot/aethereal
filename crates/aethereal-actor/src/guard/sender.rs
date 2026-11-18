//! The source of the item being handled, as a guard reads it (ADR-0058).

#[cfg(feature = "guest")]
use crate::guest::{Calls, sent_is, sent_lists};
use crate::{
    Above, Actor, ActorRef, ChildOf, Children, Declared, ErasedActorRef, Kinds, Listed, ProtocolRef, Seated, SentAs,
};
use aethereal_data::KindId;

/// The source of the item being handled, as the receiver's turn reads it: nothing is read until a guard asks
/// (ADR-0058).
///
/// It holds only what was fixed when the mail was sent, the sender's seat and the type it sent as, and what is fixed
/// of the receiver: its own seat and type, what stands above it, and the children it has made, live or ended. It holds
/// no table, so no check here reads what the engine holds of the sender now, which can change between the send and the
/// delivery: a sender that ended after it sent, from its closing turn among others, or moved onto new code, is checked
/// as it was when it sent (ADR-0020, ADR-0054). Each check answers the reference a guard hands on, typed, so a guard
/// never casts.
pub struct Sender<'t> {
    source: Seated,
    /// The type the sender sent the mail as, stamped at the send.
    sent_as: SenderType,
    /// The receiver, and what its type declares.
    me: Seated,
    declared: &'static Declared,
    above: Above,
    /// The children the receiver made before this step, live or ended.
    made: &'t dyn Children,
}

/// The type the sender of a piece of mail sent it as, as a guard reads it: stamped on the mail at the send (ADR-0058).
#[derive(Clone, Copy)]
pub enum SenderType {
    /// As the engine stamped it.
    Stamped(SentAs),
    /// As the host of a guest holds the stamp of the `item`th item of the batch it handed the guest's code, which the
    /// guest asks it about through `calls`: the guest holds no other actor's type (ADR-0015).
    #[cfg(feature = "guest")]
    Hosted { item: u32, calls: &'static Calls },
}

impl SenderType {
    /// Whether it is the actor type whose id is `actor_type`.
    #[must_use]
    pub fn is(self, actor_type: KindId) -> bool {
        match self {
            Self::Stamped(sent_as) => sent_as.is(actor_type),
            #[cfg(feature = "guest")]
            Self::Hosted { item, calls } => sent_is(calls, item, actor_type),
        }
    }

    /// Whether the type lists every one of `kinds`, as a cast reads it (ADR-0016).
    #[must_use]
    pub fn lists_all(self, kinds: &[KindId]) -> bool {
        match self {
            Self::Stamped(sent_as) => sent_as.lists_all(kinds),
            #[cfg(feature = "guest")]
            Self::Hosted { item, calls } => sent_lists(calls, item, kinds),
        }
    }
}

impl<'t> Sender<'t> {
    /// The sender `source` names, which sent the mail as `sent_as`, of mail handed to the actor `me` names, whose
    /// type declares `declared`, which stands beneath `above`, and has made the children `made` holds.
    pub(crate) const fn new(
        source: Seated,
        sent_as: SenderType,
        me: Seated,
        declared: &'static Declared,
        above: Above,
        made: &'t dyn Children,
    ) -> Self {
        Self { source, sent_as, me, declared, above, made }
    }

    /// Who it is.
    #[must_use]
    pub const fn reference(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.source)
    }

    /// It, when it sent the mail as an actor of type `T`.
    #[must_use]
    pub fn actor<T: Actor>(&self) -> Option<ActorRef<T>> {
        self.sent_as.is(KindId::of::<T>()).then(|| ActorRef::new(self.source))
    }

    /// It, when the type it sent the mail as lists every kind of `P`, as a cast reads it (ADR-0016).
    #[must_use]
    pub fn speaking<P: Listed>(&self) -> Option<ProtocolRef<P>> {
        let kinds = <P::Kinds as Kinds>::KINDS;

        self.sent_as.lists_all(kinds).then(|| ProtocolRef::new(self.source))
    }

    /// It, when it is the receiver's own seat, and the receiver is of type `A`.
    #[must_use]
    pub fn receiver<A: Actor>(&self) -> Option<ActorRef<A>> {
        let is_me = self.source == self.me;
        let is_type = self.declared.id() == KindId::of::<A>();
        let is_receiver = is_me && is_type;

        is_receiver.then(|| ActorRef::new(self.source))
    }

    /// It, when it is the actor directly above the receiver, and sent the mail as one of type `P`.
    #[must_use]
    pub fn parent<P: Actor>(&self) -> Option<ActorRef<P>> {
        let is_parent = self.parent_seat() == Some(self.source);
        let is_type = self.sent_as.is(KindId::of::<P>());
        let is_admitted = is_parent && is_type;

        is_admitted.then(|| ActorRef::new(self.source))
    }

    /// It, when it is directly beneath the receiver, a child the receiver made before this step, and sent the mail as
    /// one of type `T`.
    #[must_use]
    pub fn child<T: ChildOf>(&self) -> Option<ActorRef<T>> {
        let is_child = self.made.has_made(self.source.id);
        let is_type = self.sent_as.is(KindId::of::<T>());
        let is_admitted = is_child && is_type;

        is_admitted.then(|| ActorRef::new(self.source))
    }

    /// The receiver's parent, and its seat: none for a root.
    const fn parent_seat(&self) -> Option<Seated> {
        match self.above {
            Above::Chassis => None,
            Above::Actor(parent, _) | Above::Elsewhere(parent) => Some(parent),
        }
    }
}
