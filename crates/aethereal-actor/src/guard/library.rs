//! The library's guards, and the references a guard hands on (ADR-0058).

use super::{Checks, Guard, Sender, SenderTypes, Sent};
use crate::{Actor, ActorRef, ChildOf, ErasedActorRef, Kinds, Listed, ProtocolRef};
use aethereal_data::KindId;
use core::marker::PhantomData;

/// Every sender: the guard of a handler that takes its kind from anyone, handed the sender as it came.
///
/// It reads nothing: its check answers the reference the turn already holds, so a handler it guards costs what an
/// unguarded one did.
pub struct Anyone;

/// The receiver's own seat: what its host brought it, a job's answer, a stage, or what it sent itself. The seat is
/// compared, so a sibling of the same type, in another seat, is refused.
pub struct Itself;

/// The actor directly above the receiver, by seat: on a keyed parent's child, its one parent.
pub struct Parent;

/// An actor of type `T` directly beneath the receiver: a child it made.
pub struct Child<T>(PhantomData<fn() -> T>);

/// An actor whose type lists every kind of `P`, a tuple of kinds or a named protocol: the form of a subscription,
/// handed the subscriber as the reference it keeps.
pub struct Speaks<P>(PhantomData<fn() -> P>);

impl<A> Guard<A> for Anyone {
    type Sent = ErasedActorRef;
}

impl<A, K> Checks<A, K> for Anyone {
    #[inline]
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ErasedActorRef> {
        Some(sender.reference())
    }
}

impl<A: Actor> Guard<A> for Itself {
    type Sent = ActorRef<A>;
}

impl<A: Actor, K> Checks<A, K> for Itself {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ActorRef<A>> {
        sender.receiver::<A>()
    }
}

impl<A: ChildOf<Parent: Actor>> Guard<A> for Parent {
    type Sent = ActorRef<A::Parent>;
}

impl<A: ChildOf<Parent: Actor>, K> Checks<A, K> for Parent {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ActorRef<A::Parent>> {
        sender.parent::<A::Parent>()
    }
}

impl<A, T: ChildOf> Guard<A> for Child<T> {
    type Sent = ActorRef<T>;
}

impl<A, K, T: ChildOf> Checks<A, K> for Child<T> {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ActorRef<T>> {
        sender.child::<T>()
    }
}

impl<A, P: Listed + 'static> Guard<A> for Speaks<P> {
    type Sent = ProtocolRef<P>;
}

impl<A, K, P: Listed + 'static> Checks<A, K> for Speaks<P> {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ProtocolRef<P>> {
        ProtocolRef::<P>::from_sender(sender)
    }
}

/// An actor of type `T`, any of its type.
impl<A, T: Actor> Guard<A> for ActorRef<T> {
    type Sent = Self;
}

impl<A, K, T: Actor> Checks<A, K> for ActorRef<T> {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<Self> {
        Self::from_sender(sender)
    }
}

impl<T: Actor> Sent for ActorRef<T> {
    const TYPES: SenderTypes = SenderTypes::Types(&[KindId::of::<T>()]);

    fn from_sender(sender: &Sender<'_>) -> Option<Self> {
        sender.actor::<T>()
    }
}

impl<P: Listed + 'static> Sent for ProtocolRef<P> {
    const TYPES: SenderTypes = SenderTypes::Speaking(<P::Kinds as Kinds>::KINDS);

    fn from_sender(sender: &Sender<'_>) -> Option<Self> {
        sender.speaking::<P>()
    }
}

impl Sent for ErasedActorRef {
    const TYPES: SenderTypes = SenderTypes::Every;

    #[inline]
    fn from_sender(sender: &Sender<'_>) -> Option<Self> {
        Some(sender.reference())
    }
}
