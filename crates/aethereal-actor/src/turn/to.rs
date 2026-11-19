//! What a turn's `to` hands back: a target, found once, that sends and asks chain on (ADR-0016, ADR-0022).

use super::Seat;
#[cfg(feature = "std")]
use crate::KeyedTarget;
use crate::{
    Actors, Askable, Due, ErasedActorRef, Kinds, ProtocolRef, Proven, Reaches, SeatNumber, Seated, Table, Within,
};
use aethereal_data::{ActorId, Encode, EncodeError, Kind, KindId, PathSegment};
use core::marker::PhantomData;

/// The target a turn's [`to`](crate::Turn::to) was given, found once: one actor, through a reference or at a typed path, or several
/// references at once. Each send or ask through it answers the target again, so they chain,
/// `turn.to(renderer).ask(&mesh, &made)?.ask(&texture, &made)?`, and the target may be bound once and sent to many
/// times. It holds the turn while it lives.
///
/// What it sends to is `R`, which each send and ask is checked against at its call, for an actor of type `Me`, as each
/// verb says (ADR-0059); `At` is where it found the target, and `Mode` says whether the actor lives on to be answered: [`Lives`] for a turn or a `begin`, and
/// [`Leaves`] for an actor's closing turn, which asks nothing.
///
/// What it asks is due when the request the turn handles is, or `Never` when it handles none, unless
/// [`within`](Self::within) states a deadline of its own (ADR-0041).
pub struct Toward<'t, 'a, Me, R, At, Mode> {
    seat: &'t mut Seat<'a>,
    at: At,
    /// When what it asks is due: inherited from the turn's request, or stated.
    due: Due,
    sender: PhantomData<fn() -> Me>,
    through: PhantomData<fn() -> R>,
    mode: PhantomData<Mode>,
}

/// A target reached from a turn or a `begin`, whose actor lives on, so it may ask.
pub enum Lives {}

/// A target reached from an actor's closing turn, which may only send: the actor is gone by the time an answer could
/// come.
pub enum Leaves {}

impl<'t, 'a, Me, R, At, Mode> Toward<'t, 'a, Me, R, At, Mode> {
    pub(super) fn new(seat: &'t mut Seat<'a>, at: At) -> Self {
        let due = seat.due;

        Self { seat, at, due, sender: PhantomData, through: PhantomData, mode: PhantomData }
    }
}

impl<Me, R, Mode> Toward<'_, '_, Me, R, Seated, Mode> {
    /// Sends `mail` to the actor. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the target [`Reaches`] `K` from this actor: an [`ActorRef<T>`](crate::ActorRef) whose `T`
    /// handles `K`, a [`ProtocolRef<P>`](crate::ProtocolRef) one of whose kinds `K` is, or a [`Path<L>`](crate::Path)
    /// whose `L` handles `K`, beneath a root of a type this actor depends on; and, for a target that names its
    /// receiver's type, when the receiver's guard of `K` [`Admits`](crate::Admits) this actor's type (ADR-0059). What a
    /// reference reaches is proven, so nothing is checked when it is sent; mail to a path that held no live actor of its
    /// type is dropped, as mail to an actor that has gone is.
    pub fn send<K, I>(&mut self, mail: &K) -> Result<&mut Self, EncodeError>
    where
        R: Reaches<Me, K, I>,
        K: Kind + Encode<Actors>,
    {
        self.seat.write(self.at, mail)?;

        Ok(self)
    }
}

#[cfg(feature = "std")]
impl<Me, R, Mode> Toward<'_, '_, Me, R, Seated, Mode> {
    /// Sends `mail` to the section of the keyed actor it is for, whose key the receiver's type works out from it here,
    /// at the send, and stamps on the mail: the engine routes on the stamp, and never reads the mail to find a key
    /// (spike: keyed sections).
    pub fn send_keyed<K, I>(&mut self, mail: &K) -> Result<&mut Self, EncodeError>
    where
        R: Reaches<Me, K, I> + KeyedTarget<K>,
        K: Kind + Encode<Actors>,
    {
        self.seat.write_keyed(self.at, mail, R::section(mail))?;

        Ok(self)
    }
}

impl<Me, R> Toward<'_, '_, Me, R, Seated, Lives> {
    /// States when what it asks is due, in place of the deadline it would inherit: a [`Due`], or a
    /// [`Time`](crate::Time) it is due at. At that time the engine answers the request
    /// [`Unanswered::Expired`](crate::Unanswered::Expired), and an answer that comes later is dropped (ADR-0041).
    ///
    /// `turn.to(tiles).within(turn.now() + Duration::from_secs(5)).ask(&which, &looking)?` gives up on its own, and
    /// `turn.to(tiles).within(held.due()).ask(&which, &looking)?`, in a later turn than the one that held a request,
    /// asks for it by its deadline.
    #[must_use]
    pub fn within(mut self, due: impl Into<Due>) -> Self {
        self.due = due.into();

        self
    }

    /// Asks `mail` of the actor, keeping `context` until the request is answered, when the asker is handed what
    /// became of it with the context, as [`Answer<A, C>`](crate::Answer) (ADR-0017, ADR-0022).
    ///
    /// It builds only when the target is [`Askable`] from this actor: a [`ProtocolRef<P>`](crate::ProtocolRef), where
    /// `P` answers `K` with one kind, or a [`Path<L>`](crate::Path) or an [`ActorRef<L>`](crate::ActorRef), where `L`
    /// does, as [`AnswersByType`](crate::AnswersByType) says, a path beneath a root of a type this actor depends on,
    /// whose guard of `K` [`Admits`](crate::Admits) this actor's type; and this actor handles that answer with `C`. A
    /// path that held no live actor of its type is asked as an actor that has gone: the request is answered
    /// [`Unanswered::Ended`](crate::Unanswered::Ended).
    ///
    /// It is due as this target says: what [`within`](Self::within) stated, or the deadline of the request the turn
    /// handles, or `Never` (ADR-0041).
    pub fn ask<K, C, I>(&mut self, mail: &K, context: &C) -> Result<&mut Self, EncodeError>
    where
        R: Askable<Me, K, C, I>,
        K: Kind + Encode<Actors>,
        C: Kind + Encode<Within>,
    {
        self.seat.request(self.at, mail, KindId::of::<R::Answer>(), context, self.due)?;

        Ok(self)
    }
}

impl<Me, R: Proven, Mode> Toward<'_, '_, Me, R, &[R], Mode> {
    /// Sends `mail` to each actor the references refer to, in their order: each its own piece of mail, as if each were
    /// sent alone, and the bytes written once for all of them. What they reach is proven, so nothing is checked when
    /// it is sent.
    ///
    /// It builds only when the references [`Reaches`] `K`, as a send to one does, the receivers' guard among it.
    pub fn send<K, I>(&mut self, mail: &K) -> Result<&mut Self, EncodeError>
    where
        R: Reaches<Me, K, I>,
        K: Kind + Encode<Actors>,
    {
        self.seat.write_to_all(self.at.iter().map(|reference| reference.seated()), mail)?;

        Ok(self)
    }
}

/// Where a turn's sender stands, which what its `to` takes is found from: the table the turn reads, and the actor's
/// place in it, beneath its parent.
pub struct Vantage<'s, 'a> {
    seat: &'s Seat<'a>,
}

impl<'s, 'a> Vantage<'s, 'a> {
    pub(super) const fn new(seat: &'s Seat<'a>) -> Self {
        Self { seat }
    }

    /// The table the turn reads.
    pub(crate) fn table(&self) -> &'a dyn Table {
        self.seat.table
    }

    /// The actor directly above the sender, when its type lists every one of `P`'s kinds, as a cast finds:
    /// otherwise, and for a root, the place in [`SeatNumber::NOWHERE`].
    pub(crate) fn parent<P: Kinds>(&self) -> Seated {
        let Some(parent) = self.seat.above() else {
            return Seated { id: ActorId::new(0), seat: SeatNumber::NOWHERE };
        };
        let cast = self.seat.cast::<P>(P::KINDS, ErasedActorRef::new(parent));

        cast.map_or(Seated { id: parent.id, seat: SeatNumber::NOWHERE }, ProtocolRef::seated)
    }

    /// The place `above` then `last` name beneath the sender, from a child of its own down, when one of kind `kind` is
    /// live there, as a typed path is found: otherwise its place in [`SeatNumber::NOWHERE`], and the place in no seat
    /// at all when it would be deeper than any path.
    pub(crate) fn beneath(&self, above: &[PathSegment], last: &PathSegment, kind: KindId) -> Seated {
        let path = above.iter().chain([last]).try_fold(self.seat.path.clone(), |path, segment| path.child(segment));
        let Ok(path) = path else {
            return Seated { id: ActorId::new(0), seat: SeatNumber::NOWHERE };
        };

        self.seat.table.reach(&path, kind)
    }

    /// The child `segment` names beneath the sender, when the sender made it, in this step or before, live or not,
    /// and it is of kind `kind`: otherwise its place in [`SeatNumber::NOWHERE`]. A guest, which holds no other code's
    /// types, has its host find one it cannot check, live.
    pub(crate) fn child(&self, segment: &PathSegment, kind: KindId) -> Seated {
        let me = self.seat.me.id;
        let nowhere = Seated { id: self.seat.table.child(me, segment), seat: SeatNumber::NOWHERE };
        let Some(made) = self.seat.child(segment) else {
            return nowhere;
        };
        let Some(declared) = self.seat.declared_at(made.id) else {
            return self.seat.found_child(me, segment, kind).unwrap_or(nowhere);
        };
        let is_kind = declared.id() == kind;

        if is_kind {
            made
        } else {
            nowhere
        }
    }
}
