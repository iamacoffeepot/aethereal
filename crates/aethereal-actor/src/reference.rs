//! A reference to an actor the engine has established exists (ADR-0008).

use crate::{Made, Refers};
use aethereal_data::{ActorId, Boundary, Decode, DecodeError, Encode, EncodeError, Reader, Schema, Shape, Writer};

/// A reference to an actor the engine has established exists (ADR-0008).
///
/// Nothing outside this crate builds one. An actor comes to hold a reference
/// by being handed one in mail, by being told the sender of mail, or by asking
/// the engine to resolve a path. It says the actor existed when the reference
/// was made, and not that the actor is still there.
///
/// A reference is a bound value: it crosses [`Actors`](crate::Actors) and
/// [`Within`](crate::Within), and it is proven against the table of actors
/// wherever it is decoded. It does not cross `Bare`:
///
/// ```compile_fail
/// use aethereal_actor::ActorRef;
/// use aethereal_data::{Bare, Encode, Writer};
///
/// fn write(reference: &ActorRef) {
///     let mut out = Writer::<Bare>::new(());
///     let written = reference.encode(&mut out);
/// }
/// ```
///
/// A type that holds one is not plain data:
///
/// ```compile_fail
/// use aethereal_actor::ActorRef;
/// use aethereal_data::Plain;
///
/// #[derive(Plain)]
/// struct Subscribe {
///     to: ActorRef,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorRef {
    id: ActorId,
}

impl ActorRef {
    /// The reference to the actor `id` stands for, which the caller has established exists.
    pub(crate) const fn new(id: ActorId) -> Self {
        Self { id }
    }

    /// The id of the actor referred to.
    #[must_use]
    pub const fn id(self) -> ActorId {
        self.id
    }
}

/// A reference is its actor's id in bytes.
impl Schema for ActorRef {
    const SHAPE: Shape = ActorId::SHAPE;
}

impl<B: Boundary> Encode<B> for ActorRef
where
    for<'a> B::WriterOffers<'a>: Refers,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.id.encode(out)
    }
}

impl<B: Boundary> Decode<B> for ActorRef
where
    for<'a> B::ReaderOffers<'a>: Made,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let id = ActorId::decode(input)?;
        if input.offers().made(id) {
            Ok(Self::new(id))
        } else {
            Err(DecodeError::UnknownActor(id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actors, ActorsReader, ActorsWriter, Within, WithinReader, WithinWriter};
    use aethereal_data::{Budget, Codec};

    /// A table that has made one actor.
    struct Only(ActorId);

    impl Made for Only {
        fn made(&self, id: ActorId) -> bool {
            id == self.0
        }
    }

    /// A kind that holds a reference beside plain data.
    #[derive(Debug, PartialEq, Codec)]
    struct Subscribe {
        topic: u32,
        to: ActorRef,
    }

    #[test]
    fn reference_in_mail_is_proven_where_it_is_read() {
        // Catches a decode that takes an id as a reference without asking the table, and a derived codec that
        // loses the reference's place among plain fields.
        let sent = Subscribe { topic: 7, to: ActorRef::new(ActorId::new(41)) };
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
        let held = ActorRef::new(ActorId::new(41));
        let mut out = Writer::<Within>::new(WithinWriter::new());
        assert_eq!(held.encode(&mut out), Ok(()));
        let bytes = out.into_bytes();

        let other = Only(ActorId::new(42));
        let mut budget = Budget::new(64);
        let mut reader = Reader::<Within>::new(&bytes, &mut budget, WithinReader::new(&other));
        assert_eq!(ActorRef::decode(&mut reader), Err(DecodeError::UnknownActor(ActorId::new(41))));
    }
}
