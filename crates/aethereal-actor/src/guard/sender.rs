//! The source of the item being handled, as a guard reads it (ADR-0058).

use crate::{
    Above, Actor, ActorRef, ChildOf, Children, Declared, ErasedActorRef, Kinds, Listed, Path, PathTo, ProtocolRef,
    Seated, Table,
};
use aethereal_data::KindId;

/// The source of the item being handled, as the receiver's turn reads it: nothing is read until a guard asks
/// (ADR-0058).
///
/// Each check answers the reference a guard hands on, typed, so a guard never casts. A check of a type reads the
/// receiver's own declaration, or its parent's, when the sender is one of them, and asks the table otherwise.
pub struct Sender<'t> {
    source: Seated,
    /// The receiver, and what its type declares.
    me: Seated,
    declared: &'static Declared,
    above: Above,
    table: &'t dyn Table,
    /// The children the receiver made before this step.
    made: &'t dyn Children,
}

impl<'t> Sender<'t> {
    /// The sender `source` names, of an item handed to the actor `me` names, whose type declares `declared`, which
    /// stands beneath `above`, has made the children `made` holds, and reads `table`.
    pub(crate) const fn new(
        source: Seated,
        me: Seated,
        declared: &'static Declared,
        above: Above,
        table: &'t dyn Table,
        made: &'t dyn Children,
    ) -> Self {
        Self { source, me, declared, above, table, made }
    }

    /// Who it is.
    #[must_use]
    pub const fn reference(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.source)
    }

    /// It, when it is an actor of type `T`.
    #[must_use]
    pub fn actor<T: Actor>(&self) -> Option<ActorRef<T>> {
        self.is_of(KindId::of::<T>()).then(|| ActorRef::new(self.source))
    }

    /// It, when its type lists every kind of `P`, as a cast reads it (ADR-0016).
    #[must_use]
    pub fn speaking<P: Listed>(&self) -> Option<ProtocolRef<P>> {
        let kinds = <P::Kinds as Kinds>::KINDS;
        let lists =
            self.known().map_or_else(|| self.table.lists(self.source.id, kinds), |known| known.lists_all(kinds));

        lists.then(|| ProtocolRef::new(self.source))
    }

    /// It, when it is the receiver's own seat, and the receiver is of type `A`.
    #[must_use]
    pub fn receiver<A: Actor>(&self) -> Option<ActorRef<A>> {
        let is_me = self.source == self.me;
        let is_type = self.declared.id() == KindId::of::<A>();
        let is_receiver = is_me && is_type;

        is_receiver.then(|| ActorRef::new(self.source))
    }

    /// It, when it is the actor directly above the receiver, of type `P`.
    #[must_use]
    pub fn parent<P: Actor>(&self) -> Option<ActorRef<P>> {
        let is_parent = self.parent_seat() == Some(self.source);
        let is_type = self.is_of(KindId::of::<P>());
        let is_admitted = is_parent && is_type;

        is_admitted.then(|| ActorRef::new(self.source))
    }

    /// It, when it is of type `T`, directly beneath the receiver: a child the receiver made before this step.
    #[must_use]
    pub fn child<T: ChildOf>(&self) -> Option<ActorRef<T>> {
        let is_child = self.made.has_made(self.source.id);
        let is_type = self.is_of(KindId::of::<T>());
        let is_admitted = is_child && is_type;

        is_admitted.then(|| ActorRef::new(self.source))
    }

    /// It, when it is the live actor the typed path `path` names, as a resolve sees it.
    #[must_use]
    pub fn at<L: ChildOf + PathTo>(&self, path: &Path<L>) -> Option<ActorRef<L>> {
        let is_there = self.table.resolve(path.names()) == Some(self.source);
        let is_type = self.is_of(KindId::of::<L>());
        let is_admitted = is_there && is_type;

        is_admitted.then(|| ActorRef::new(self.source))
    }

    /// Whether it is an actor of the type whose id is `actor_type`.
    fn is_of(&self, actor_type: KindId) -> bool {
        self.known().map_or_else(|| self.table.is_of(self.source.id, actor_type), |known| known.id() == actor_type)
    }

    /// What its type declares, when it is the receiver or the receiver's parent, whose types the turn holds.
    fn known(&self) -> Option<&'static Declared> {
        if self.source == self.me {
            return Some(self.declared);
        }
        match self.above {
            Above::Actor(parent, declared) if parent == self.source => Some(declared),
            Above::Chassis | Above::Actor(..) | Above::Elsewhere(_) => None,
        }
    }

    /// The receiver's parent, and its seat: none for a root.
    const fn parent_seat(&self) -> Option<Seated> {
        match self.above {
            Above::Chassis => None,
            Above::Actor(parent, _) | Above::Elsewhere(parent) => Some(parent),
        }
    }
}
