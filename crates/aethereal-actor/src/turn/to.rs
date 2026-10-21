//! What a turn's `to` hands back: a target, found once, that sends and asks chain on (ADR-0016, ADR-0022).

use super::Seat;
use crate::{
    Actors, Allows, Askable, ErasedActorRef, Kinds, ProtocolRef, Proven, Reaches, SeatNumber, Seated, Table, Within,
};
use aethereal_data::{ActorId, Encode, EncodeError, Kind, KindId, PathSegment};
use core::marker::PhantomData;

/// The target a turn's [`to`](crate::Turn::to) was given, found once: one actor, through a reference or at a typed path, or several
/// references at once. Each send or ask through it answers the target again, so they chain,
/// `turn.to(renderer).ask(&mesh, &made)?.ask(&texture, &made)?`, and the target may be bound once and sent to many
/// times. It holds the turn while it lives.
///
/// What it sends to is `R`, whose markers the sends of `S` must list, as each verb says; `At` is where it found the
/// target, and `Mode` says whether the actor lives on to be answered: [`Lives`] for a turn or a `begin`, and
/// [`Leaves`] for an actor's closing turn, which asks nothing.
pub struct Toward<'t, 'a, S, R, At, Mode> {
    seat: &'t mut Seat<'a>,
    at: At,
    sends: PhantomData<fn() -> S>,
    through: PhantomData<fn() -> R>,
    mode: PhantomData<Mode>,
}

/// A target reached from a turn or a `begin`, whose actor lives on, so it may ask.
pub enum Lives {}

/// A target reached from an actor's closing turn, which may only send: the actor is gone by the time an answer could
/// come.
pub enum Leaves {}

impl<'t, 'a, S, R, At, Mode> Toward<'t, 'a, S, R, At, Mode> {
    pub(super) fn new(seat: &'t mut Seat<'a>, at: At) -> Self {
        Self { seat, at, sends: PhantomData, through: PhantomData, mode: PhantomData }
    }
}

impl<S, R, Mode> Toward<'_, '_, S, R, Seated, Mode> {
    /// Sends `mail` to the actor. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list what the target [`Reaches`] by: [`ToType<T, K>`](crate::ToType) for
    /// an [`ActorRef<T>`](crate::ActorRef), [`ToAny<P, K>`](crate::ToAny) for a
    /// [`ProtocolRef<P>`](crate::ProtocolRef), and [`Tells<Path<L>, K>`](crate::Tells) for a
    /// [`Path<L>`](crate::Path). What a reference reaches is proven, so nothing is checked when it is sent; mail to a
    /// path that held no live actor of its type is dropped, as mail to an actor that has gone is.
    pub fn send<K, I>(&mut self, mail: &K) -> Result<&mut Self, EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write(self.at, mail)?;

        Ok(self)
    }
}

impl<S, R> Toward<'_, '_, S, R, Seated, Lives> {
    /// Asks `mail` of the actor, keeping `context` until the request is answered, when the asker is handed what
    /// became of it with the context, as [`Answer<A, C>`](crate::Answer) (ADR-0017, ADR-0022).
    ///
    /// It builds only when the turn's sends list what the target is [`Askable`] by: [`Asks<P, K, C>`](crate::Asks)
    /// for a [`ProtocolRef<P>`](crate::ProtocolRef), where `P` answers `K` with one kind, and `Asks<Path<L>, K, C>`
    /// for a [`Path<L>`](crate::Path), where `L` does; and the actor handles that answer with `C`. A path that held no
    /// live actor of its type is asked as an actor that has gone: the request is answered
    /// [`Unanswered::Ended`](crate::Unanswered::Ended).
    pub fn ask<K, C, I>(&mut self, mail: &K, context: &C) -> Result<&mut Self, EncodeError>
    where
        R: Askable<K, C>,
        K: Kind + Encode<Actors>,
        C: Kind + Encode<Within>,
        S: Allows<R::Marker, I>,
    {
        self.seat.request(self.at, mail, KindId::of::<R::Answer>(), context)?;

        Ok(self)
    }
}

impl<S, R: Proven, Mode> Toward<'_, '_, S, R, &[R], Mode> {
    /// Sends `mail` to each actor the references refer to, in their order: each its own piece of mail, as if each were
    /// sent alone, and the bytes written once for all of them. What they reach is proven, so nothing is checked when
    /// it is sent.
    ///
    /// It builds only when the turn's sends list what the references [`Reaches`] by, as a send to one does.
    pub fn send<K, I>(&mut self, mail: &K) -> Result<&mut Self, EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
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
        let cast = self.seat.cast::<P>(ErasedActorRef::new(parent));

        cast.map_or(Seated { id: parent.id, seat: SeatNumber::NOWHERE }, ProtocolRef::seated)
    }

    /// The place `segments` name beneath the sender, from a child of its own down, when one of kind `kind` is live
    /// there, as a typed path is found: otherwise its place in [`SeatNumber::NOWHERE`], and the place in no seat at
    /// all when it would be deeper than any path.
    pub(crate) fn beneath(&self, segments: &[PathSegment], kind: KindId) -> Seated {
        let path = segments.iter().try_fold(self.seat.path.clone(), |path, segment| path.child(segment));
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
