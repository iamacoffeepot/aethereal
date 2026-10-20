//! A reference to an actor the engine has established exists (ADR-0008).

use crate::{
    Actor, ChildOf, Conforms, Covers, Handled, Handles, Listed, Path, PathTo, Refers, SeatNumber, Seated, Table, Tells,
    ToAny, ToType,
};
use aethereal_data::{
    ActorId, Boundary, Decode, DecodeError, Encode, EncodeError, Field, Kind, KindId, Reader, Schema, Shape, Writer,
};
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// A reference to an actor the engine has established exists, and the seat it sits in (ADR-0008, ADR-0011).
///
/// The seat is carried so that whatever is kept for the actor is found by its
/// number, in the order the engine keeps its actors, with no lookup by id. It
/// is written beside the id where the reference crosses a boundary.
///
/// An actor comes to hold a reference by being handed one in mail, by being
/// told the sender of mail, or by asking the engine to resolve a path. It says
/// the actor existed when the reference was made, and not that the actor is
/// still there.
///
/// A reference is a bound value: it crosses [`Actors`](crate::Actors) and
/// [`Within`](crate::Within). It was proven when it was made, so it is read
/// back as it was written, with nothing looked up. Bytes written by hand for
/// an id that was never made are not caught there: a send to such a reference
/// is not delivered. It does not cross `Bare`:
///
/// ```compile_fail
/// use aethereal_actor::ErasedActorRef;
/// use aethereal_data::{Bare, Encode, Writer};
///
/// fn write(reference: &ErasedActorRef) {
///     let mut out = Writer::<Bare>::new(());
///     let written = reference.encode(&mut out);
/// }
/// ```
///
/// A type that holds one is not plain data:
///
/// ```compile_fail
/// use aethereal_actor::ErasedActorRef;
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Subscribe {
///     to: ErasedActorRef,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErasedActorRef {
    seated: Seated,
}

impl ErasedActorRef {
    /// The reference to the actor `seated` names, which the caller has established exists.
    pub(crate) const fn new(seated: Seated) -> Self {
        Self { seated }
    }

    /// The id of the actor referred to.
    #[must_use]
    pub const fn id(self) -> ActorId {
        self.seated.id
    }

    /// The seat of the actor referred to, which it sits in or sat in.
    #[must_use]
    pub const fn seat(self) -> SeatNumber {
        self.seated.seat
    }

    /// The actor referred to, and its seat.
    #[must_use]
    pub const fn seated(self) -> Seated {
        self.seated
    }
}

/// A reference is its actor's id and seat in bytes, read back as they were written.
impl Schema for ErasedActorRef {
    const SHAPE: Shape = Shape::Struct {
        name: "ErasedActorRef",
        fields: &[Field { name: "id", shape: ActorId::SHAPE }, Field { name: "seat", shape: u64::SHAPE }],
    };
}

impl<B: Boundary> Encode<B> for ErasedActorRef
where
    for<'a> B::WriterOffers<'a>: Refers,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.seated.id.encode(out)?;
        self.seated.seat.number().encode(out)
    }
}

/// A reference was proven when it was made, so the bytes are read with nothing looked up (ADR-0008).
impl<B: Boundary> Decode<B> for ErasedActorRef
where
    for<'a> B::ReaderOffers<'a>: Refers,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let id = ActorId::decode(input)?;
        let seat = SeatNumber::new(u64::decode(input)?);

        Ok(Self::new(Seated { id, seat }))
    }
}

/// A reference to an actor of type `T` the engine has established exists: the reference an actor holds, the
/// [`ErasedActorRef`] being for the few that cannot name the type (ADR-0008).
///
/// Only the engine makes one, when it makes the actor, so a reference proves
/// its actor's type for as long as it exists, and a send through it is not
/// checked against the receiver's declaration (ADR-0013). It crosses what an
/// [`ErasedActorRef`] crosses, the same way: between actors in one engine and
/// into an actor's own state, never out of the engine.
///
/// A codec written by hand could put any id where a reference is read. Nothing
/// stops that, as it would cost every reference it reads: kinds that carry a
/// reference derive their codecs.
pub struct ActorRef<T> {
    seated: Seated,
    of: PhantomData<fn() -> T>,
}

impl<T> ActorRef<T> {
    /// The reference to the actor of type `T` that `seated` names, which the caller has established exists.
    pub(crate) const fn new(seated: Seated) -> Self {
        Self { seated, of: PhantomData }
    }

    /// The id of the actor referred to.
    #[must_use]
    pub const fn id(self) -> ActorId {
        self.seated.id
    }

    /// The actor referred to, and its seat.
    #[must_use]
    pub const fn seated(self) -> Seated {
        self.seated
    }

    /// The same reference, without its type.
    #[must_use]
    pub const fn erase(self) -> ErasedActorRef {
        ErasedActorRef::new(self.seated)
    }

    /// The same reference, to the protocol `P`, which its type [`Conforms`] to, each of what that asks found at its
    /// place in `Is`, which the compiler infers: `reference.protocol()` where the protocol is named by the type it is
    /// given to (ADR-0016). The compiler checks it, and nothing is read when it runs.
    #[must_use]
    pub const fn protocol<P, Is>(self) -> ProtocolRef<P>
    where
        T: Conforms<P, Is>,
    {
        ProtocolRef::new(self.seated)
    }
}

impl<T> Clone for ActorRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ActorRef<T> {}

impl<T> PartialEq for ActorRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.seated == other.seated
    }
}

impl<T> Eq for ActorRef<T> {}

impl<T> Hash for ActorRef<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.seated.hash(state);
    }
}

impl<T> fmt::Debug for ActorRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ActorRef").field(&self.seated).finish()
    }
}

impl<T> From<ActorRef<T>> for ErasedActorRef {
    fn from(reference: ActorRef<T>) -> Self {
        reference.erase()
    }
}

/// Written and read as an [`ErasedActorRef`] is: its type is the type of the field it is read into.
impl<T> Schema for ActorRef<T> {
    const SHAPE: Shape = ErasedActorRef::SHAPE;
}

impl<B: Boundary, T> Encode<B> for ActorRef<T>
where
    for<'a> B::WriterOffers<'a>: Refers,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.erase().encode(out)
    }
}

impl<B: Boundary, T> Decode<B> for ActorRef<T>
where
    for<'a> B::ReaderOffers<'a>: Refers,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        ErasedActorRef::decode(input).map(|reference| Self::new(reference.seated()))
    }
}

/// A reference to an actor whose type takes every kind of the protocol `P`, whatever that type is: what one list
/// of listeners of many types holds (ADR-0016).
///
/// It is proven when it is made, as an [`ActorRef`] is, and nothing checks it
/// again. It is made from an `ActorRef<T>` whose `T` [`Conforms`] to `P`, by
/// its `protocol`, which the compiler checks; from an [`ErasedActorRef`] by a turn's `cast`,
/// which reads the declaration of the actor's type once; or by narrowing one
/// to a protocol it [`Covers`]. It crosses what an `ActorRef` crosses, the same
/// way.
///
/// It is made only from a type that handles every kind:
///
/// ```compile_fail
/// use aethereal_actor::{Actor, ActorRef, ProtocolRef};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct Poke;
///
/// impl Kind for Poke {
///     const NAME: KindName = KindName::new("aether.example.poke");
/// }
///
/// fn listen<T: Actor>(reference: ActorRef<T>) -> ProtocolRef<(Poke,)> {
///     reference.protocol()
/// }
/// ```
pub struct ProtocolRef<P> {
    seated: Seated,
    of: PhantomData<fn() -> P>,
}

impl<P> ProtocolRef<P> {
    /// The reference to the actor `seated` names, whose type the caller has established takes every kind of `P`.
    pub(crate) const fn new(seated: Seated) -> Self {
        Self { seated, of: PhantomData }
    }

    /// The id of the actor referred to.
    #[must_use]
    pub const fn id(self) -> ActorId {
        self.seated.id
    }

    /// The actor referred to, and its seat.
    #[must_use]
    pub const fn seated(self) -> Seated {
        self.seated
    }

    /// The same reference, without its protocol.
    #[must_use]
    pub const fn erase(self) -> ErasedActorRef {
        ErasedActorRef::new(self.seated)
    }

    /// The same reference, to the protocol `Q`, which takes no kind `P` does not: `(A, B)` narrows to `(B, A)` and to
    /// `(A,)`. Nothing is read when it runs.
    ///
    /// It builds only when every kind of `Q` is one of `P`'s:
    ///
    /// ```compile_fail
    /// use aethereal_actor::ProtocolRef;
    ///
    /// struct Poke;
    /// struct Shut;
    ///
    /// fn widen(reference: ProtocolRef<(Poke,)>) -> ProtocolRef<(Poke, Shut)> {
    ///     reference.narrow()
    /// }
    /// ```
    #[must_use]
    pub const fn narrow<Q, Is>(self) -> ProtocolRef<Q>
    where
        P: Listed,
        P::Kinds: Covers<Q, Is>,
    {
        ProtocolRef::new(self.seated)
    }
}

impl<P> Clone for ProtocolRef<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for ProtocolRef<P> {}

impl<P> PartialEq for ProtocolRef<P> {
    fn eq(&self, other: &Self) -> bool {
        self.seated == other.seated
    }
}

impl<P> Eq for ProtocolRef<P> {}

impl<P> Hash for ProtocolRef<P> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.seated.hash(state);
    }
}

impl<P> fmt::Debug for ProtocolRef<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ProtocolRef").field(&self.seated).finish()
    }
}

impl<P> From<ProtocolRef<P>> for ErasedActorRef {
    fn from(reference: ProtocolRef<P>) -> Self {
        reference.erase()
    }
}

/// Written and read as an [`ErasedActorRef`] is: its protocol is the type of the field it is read into.
impl<P> Schema for ProtocolRef<P> {
    const SHAPE: Shape = ErasedActorRef::SHAPE;
}

impl<B: Boundary, P> Encode<B> for ProtocolRef<P>
where
    for<'a> B::WriterOffers<'a>: Refers,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.erase().encode(out)
    }
}

impl<B: Boundary, P> Decode<B> for ProtocolRef<P>
where
    for<'a> B::ReaderOffers<'a>: Refers,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        ErasedActorRef::decode(input).map(|reference| Self::new(reference.seated()))
    }
}

/// A proven reference to an actor that takes mail of kind `K`, and the send a turn declares to reach it: what the
/// send verbs take, so one verb sends through either reference (ADR-0016).
///
/// A send through a protocol reference builds only when the turn's sends list it:
///
/// ```compile_fail
/// use aethereal_actor::{ProtocolRef, Turn};
/// use aethereal_data::{Kind, KindName, Plain};
///
/// #[derive(Plain)]
/// struct Poke;
///
/// impl Kind for Poke {
///     const NAME: KindName = KindName::new("aether.example.poke");
/// }
///
/// fn poke<A>(turn: &mut Turn<'_, A, ()>, listener: ProtocolRef<(Poke,)>) {
///     let sent = turn.send_to(listener, &Poke);
/// }
/// ```
pub trait Reaches<K> {
    /// What a turn's sends list for it: [`ToType<T, K>`] for an `ActorRef<T>`, [`ToAny<P, K>`] for a
    /// `ProtocolRef<P>`, and [`Tells<Path<L>, K>`] for a [`Path<L>`].
    type Marker;

    /// The actor it reaches as it is sent, and its seat, read from `table` when it is a path: the place in
    /// [`SeatNumber::NOWHERE`] when it holds no live actor of its type, where the mail is dropped (ADR-0022). A
    /// reference reaches its actor without reading the table.
    fn reached(self, table: &dyn Table) -> Seated;
}

impl<T: Actor + Handles<K>, K: Kind + Handled> Reaches<K> for ActorRef<T> {
    type Marker = ToType<T, K>;

    fn reached(self, _: &dyn Table) -> Seated {
        self.seated
    }
}

impl<P: Listed, K: Kind> Reaches<K> for ProtocolRef<P> {
    type Marker = ToAny<P, K>;

    fn reached(self, _: &dyn Table) -> Seated {
        self.seated
    }
}

/// A reference, which reaches its actor with no lookup: what a send to many at once takes, written once for all.
pub trait Proven: Copy {
    /// The actor referred to, and its seat.
    fn seated(self) -> Seated;
}

impl<T> Proven for ActorRef<T> {
    fn seated(self) -> Seated {
        self.seated
    }
}

impl<P> Proven for ProtocolRef<P> {
    fn seated(self) -> Seated {
        self.seated
    }
}

/// A path is checked at each send, as it proves nothing: the actor found there must be of `L`'s type, and a place
/// that holds another is as one that holds none, as two types of one name share an id (ADR-0022). Mail to a place
/// that holds none is mailed to no seat, where the engine drops it as it drops mail to an actor that has gone.
impl<L: ChildOf + PathTo + Handles<K>, K: Kind + Handled> Reaches<K> for Path<L> {
    type Marker = Tells<Self, K>;

    fn reached(self, table: &dyn Table) -> Seated {
        table.reach(self.names(), KindId::of::<L>())
    }
}

/// What a turn watches: the actor a reference refers to, or the one at a typed path as it is watched, checked as a
/// send at it is (ADR-0022).
pub trait Watchable {
    /// The actor watched, read from `table` when it is a path: the place in [`SeatNumber::NOWHERE`] when it holds no
    /// live actor of its type, which is shown as ended.
    fn watched(self, table: &dyn Table) -> ErasedActorRef;
}

impl Watchable for ErasedActorRef {
    fn watched(self, _: &dyn Table) -> ErasedActorRef {
        self
    }
}

impl<T> Watchable for ActorRef<T> {
    fn watched(self, _: &dyn Table) -> ErasedActorRef {
        self.into()
    }
}

impl<P> Watchable for ProtocolRef<P> {
    fn watched(self, _: &dyn Table) -> ErasedActorRef {
        self.into()
    }
}

impl<L: ChildOf + PathTo> Watchable for Path<L> {
    fn watched(self, table: &dyn Table) -> ErasedActorRef {
        ErasedActorRef::new(table.reach(self.names(), KindId::of::<L>()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actors, ActorsReader, ActorsWriter, Written};
    use aethereal_data::{Bound, Budget};
    use alloc::vec::Vec;

    /// A reference to the actor `bits` names, in seat `seat`.
    const fn reference(bits: u64, seat: u64) -> ErasedActorRef {
        ErasedActorRef::new(Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) })
    }

    /// A kind that holds references beside plain data.
    #[derive(Bound, Debug, PartialEq)]
    struct Subscribe {
        topic: u32,
        to: ErasedActorRef,
        typed: ActorRef<()>,
        protocol: ProtocolRef<()>,
    }

    #[test]
    fn reference_is_read_back_with_its_seat() {
        // Catches a codec that drops the seat, or swaps it with the id, and a derived codec that loses a reference's
        // place among plain fields, at either boundary a reference crosses.
        let sent = Subscribe {
            topic: 7,
            to: reference(41, 3),
            typed: ActorRef::new(reference(42, 9).seated()),
            protocol: ProtocolRef::new(reference(43, 5).seated()),
        };

        let mut held = Vec::new();
        let mut out = Writer::<Actors>::new(ActorsWriter::new(&mut held));
        assert_eq!(sent.encode(&mut out), Ok(()));
        let bytes = out.into_bytes();
        let mut budget = Budget::new(64);
        let pins = crate::PinSource::lying(&bytes, &held);
        let mut reader = Reader::<Actors>::new(&bytes, &mut budget, ActorsReader::of(&pins));
        assert_eq!(Subscribe::decode(&mut reader).as_ref(), Ok(&sent));

        let read = Written::of(&sent).map(|written| written.read::<Subscribe>(64));
        assert_eq!(read, Ok(Ok(sent)));
    }
}
