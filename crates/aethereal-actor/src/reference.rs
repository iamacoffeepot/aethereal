//! A reference to an actor the engine has established exists (ADR-0008).

use crate::{Made, Refers, SeatNumber, Seated};
use aethereal_data::{ActorId, Boundary, Decode, DecodeError, Encode, EncodeError, Reader, Schema, Shape, Writer};
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// A reference to an actor the engine has established exists, and the seat it sits in (ADR-0008, ADR-0011).
///
/// The seat is carried so that whatever is kept for the actor is found by its
/// number, in the order the engine keeps its actors, with no lookup by id. It
/// is not written where the reference crosses a boundary: a reference read
/// back finds its seat again in the table that proves it.
///
/// An actor comes to hold a reference by being handed one in mail, by being
/// told the sender of mail, or by asking the engine to resolve a path. It says
/// the actor existed when the reference was made, and not that the actor is
/// still there.
///
/// The engine's own constructors of a turn and of mail are public, so code
/// can build one by hand for an id that was never made. Such a reference is
/// caught where it matters: mail carrying it is refused where it is decoded,
/// and a send to it is not delivered.
///
/// A reference is a bound value: it crosses [`Actors`](crate::Actors) and
/// [`Within`](crate::Within), and it is proven against the table of actors
/// wherever it is decoded. It does not cross `Bare`:
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

/// A reference is its actor's id in bytes. Its seat is found again where it is read.
impl Schema for ErasedActorRef {
    const SHAPE: Shape = ActorId::SHAPE;
}

impl<B: Boundary> Encode<B> for ErasedActorRef
where
    for<'a> B::WriterOffers<'a>: Refers,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.seated.id.encode(out)
    }
}

impl<B: Boundary> Decode<B> for ErasedActorRef
where
    for<'a> B::ReaderOffers<'a>: Made,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let id = ActorId::decode(input)?;
        let seat = input.offers().seat(id).ok_or(DecodeError::UnknownActor(id))?;

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
    for<'a> B::ReaderOffers<'a>: Made,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        ErasedActorRef::decode(input).map(|reference| Self::new(reference.seated()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actors, ActorsReader, ActorsWriter, Within, WithinReader, WithinWriter};
    use aethereal_data::{Bound, Budget};

    /// A table that has made one actor.
    struct Only(ActorId);

    impl Made for Only {
        fn seat(&self, id: ActorId) -> Option<SeatNumber> {
            (id == self.0).then_some(SEAT)
        }
    }

    /// The seat every actor in these tests sits in.
    const SEAT: SeatNumber = SeatNumber::new(3);

    /// A reference to the actor `bits` names, in [`SEAT`].
    const fn reference(bits: u64) -> ErasedActorRef {
        ErasedActorRef::new(Seated { id: ActorId::new(bits), seat: SEAT })
    }

    /// A kind that holds a reference beside plain data.
    #[derive(Bound, Debug, PartialEq)]
    struct Subscribe {
        topic: u32,
        to: ErasedActorRef,
    }

    #[test]
    fn reference_in_mail_is_proven_where_it_is_read() {
        // Catches a decode that takes an id as a reference without asking the table, and a derived codec that
        // loses the reference's place among plain fields.
        let sent = Subscribe { topic: 7, to: reference(41) };
        let mut out = Writer::<Actors>::new(ActorsWriter::new());
        assert_eq!(sent.encode(&mut out), Ok(()));
        let bytes = out.into_bytes();

        let table = Only(ActorId::new(41));
        let mut budget = Budget::new(64);
        let mut reader = Reader::<Actors>::new(&bytes, &mut budget, ActorsReader::new(&table));
        assert_eq!(Subscribe::decode(&mut reader), Ok(sent));

        let other = Only(ActorId::new(42));
        let mut reader = Reader::<Actors>::new(&bytes, &mut budget, ActorsReader::new(&other));
        assert_eq!(Subscribe::decode(&mut reader), Err(DecodeError::UnknownActor(ActorId::new(41))));
    }

    #[test]
    fn reference_in_state_is_proven_when_read_back() {
        // Catches `Within` taking a stored id on trust, where `Actors` proves it.
        let held = reference(41);
        let mut out = Writer::<Within>::new(WithinWriter::new());
        assert_eq!(held.encode(&mut out), Ok(()));
        let bytes = out.into_bytes();

        let other = Only(ActorId::new(42));
        let mut budget = Budget::new(64);
        let mut reader = Reader::<Within>::new(&bytes, &mut budget, WithinReader::new(&other));
        assert_eq!(ErasedActorRef::decode(&mut reader), Err(DecodeError::UnknownActor(ActorId::new(41))));
    }
}
