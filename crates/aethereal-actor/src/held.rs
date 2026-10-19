//! Held requests: a request an actor answers in a later turn than the one that handled it, and what the engine keeps
//! of the debt until it is answered (ADR-0017).

use crate::{Fault, Seated, Within};
use aethereal_data::{
    ActorId, Decode, DecodeError, Encode, EncodeError, Field, Kind, KindId, KindName, Plain, Reader, Schema, Shape,
    Writer,
};
use core::any;
use core::fmt;
use core::marker::PhantomData;
use core::mem;

/// A request this actor owes an answer of kind `A`, held from the turn that handled it to answer in a later one
/// (ADR-0017).
///
/// It is neither `Clone` nor `Copy`, and [`Turn::answer`](crate::Turn::answer) takes it by value, so it answers once.
/// It crosses [`Within`] and no other boundary, so it is kept in the actor's state or in what it keeps for a request
/// of its own, and is never mail. One that is dropped without an answer is answered by the engine, once the step that
/// dropped it is folded, that the request went unanswered.
#[must_use = "a request held is answered, or the engine says it went unanswered"]
pub struct Held<A> {
    owed: Owed,
    of: PhantomData<fn() -> A>,
}

/// A debt an actor holds: who holds it, who asked, and the asker's count of the request (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owed {
    pub holder: Seated,
    pub asker: Seated,
    pub request: u64,
}

/// What a turn did to the debts its actor holds, for the engine to file once the turn has run (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owes {
    /// It held the request it handled, to answer with a kind `answer` names.
    Holds { owed: Owed, answer: KindId },
    /// It answered a debt with a kind `answer` names.
    Answers { owed: Owed, answer: KindId },
}

impl<A> Held<A> {
    /// The debt `owed` names, held by one more value.
    pub(crate) const fn new(owed: Owed) -> Self {
        Self { owed, of: PhantomData }
    }

    /// The debt it names.
    pub(crate) const fn owed(&self) -> Owed {
        self.owed
    }

    /// The debt it names, used up: a debt answered is no longer held.
    pub(crate) fn into_owed(self) -> Owed {
        let owed = self.owed;
        mem::forget(self);

        owed
    }
}

impl<A> Drop for Held<A> {
    fn drop(&mut self) {
        holders::change(self.owed, -1);
    }
}

impl<A> fmt::Debug for Held<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Held<{}>({:?})", any::type_name::<A>(), self.owed)
    }
}

/// A held request is its debt in bytes.
impl<A> Schema for Held<A> {
    const SHAPE: Shape = Shape::Struct {
        name: "Held",
        fields: &[
            Field { name: "holder", shape: u64::SHAPE },
            Field { name: "holder_seat", shape: u64::SHAPE },
            Field { name: "asker", shape: u64::SHAPE },
            Field { name: "asker_seat", shape: u64::SHAPE },
            Field { name: "request", shape: u64::SHAPE },
        ],
    };
}

/// Bytes a held request is written into hold its debt as well, until they are read back into a held request, which
/// takes it over: so a debt kept in what an actor keeps for a request of its own is not let go of (ADR-0017).
impl<A> Encode<Within> for Held<A> {
    fn encode(&self, out: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
        let Owed { holder, asker, request } = self.owed;
        holder.id.bits().encode(out)?;
        holder.seat.number().encode(out)?;
        asker.id.bits().encode(out)?;
        asker.seat.number().encode(out)?;
        request.encode(out)?;
        holders::change(self.owed, 1);

        Ok(())
    }
}

impl<A> Decode<Within> for Held<A> {
    fn decode(input: &mut Reader<'_, Within>) -> Result<Self, DecodeError> {
        let holder = seated(input)?;
        let asker = seated(input)?;
        let request = u64::decode(input)?;

        Ok(Self::new(Owed { holder, asker, request }))
    }
}

/// An actor and its seat, read as a held request writes them.
fn seated(input: &mut Reader<'_, Within>) -> Result<Seated, DecodeError> {
    let id = ActorId::new(u64::decode(input)?);
    let seat = crate::SeatNumber::new(u64::decode(input)?);

    Ok(Seated { id, seat })
}

/// The reason of a turn that held a request where none is owed, the mail it handles being no request or one already
/// answered, or that answered a debt its actor does not hold of that kind; and of one that holds in a guest's code,
/// which holds nothing yet (ADR-0015, ADR-0017).
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct NotOwed;

impl Kind for NotOwed {
    const NAME: KindName = KindName::new("aether.actor.not_owed");
}

impl From<NotOwed> for Fault {
    fn from(_: NotOwed) -> Self {
        Self::bare::<NotOwed>()
    }
}

/// How many values hold each debt, as they change on a worker: noted as values are made, written and dropped, and
/// taken by the engine after each part it runs, as [`pins`](crate::pins) are for blobs (ADR-0017).
#[cfg(feature = "std")]
pub mod holders {
    use super::Owed;
    use alloc::vec::Vec;
    use core::cell::RefCell;
    use core::mem;

    /// One change to how many values hold one debt.
    pub type Change = (Owed, i64);

    /// Whether this thread notes changes: only while the engine runs a share on it.
    enum Buffer {
        Closed,
        /// The changes noted since they were last taken.
        Open(Vec<Change>),
    }

    std::thread_local! {
        static OWN: RefCell<Buffer> = const { RefCell::new(Buffer::Closed) };
    }

    /// Notes that the debt `owed` names is held by `by` more values. Nothing is noted where no engine runs: a debt
    /// dropped there is dropped with its engine.
    pub(crate) fn change(owed: Owed, by: i64) {
        // A thread being torn down has nothing left to note into.
        let _ = OWN.try_with(|own| {
            if let Buffer::Open(changes) = &mut *own.borrow_mut() {
                changes.push((owed, by));
            }
        });
    }

    /// Begins noting changes on this thread, into `room`, emptied and kept for its room.
    pub fn open(room: Vec<Change>) {
        debug_assert!(room.is_empty(), "changes are noted into an empty list");
        OWN.with(|own| *own.borrow_mut() = Buffer::Open(room));
    }

    /// Adds the changes noted since they were last taken to `into`, and goes on noting.
    pub fn take(into: &mut Vec<Change>) {
        OWN.with(|own| {
            if let Buffer::Open(changes) = &mut *own.borrow_mut() {
                into.append(changes);
            }
        });
    }

    /// Stops noting changes on this thread, and answers with the list they were noted into, for its room.
    #[must_use]
    pub fn close() -> Vec<Change> {
        OWN.with(|own| match mem::replace(&mut *own.borrow_mut(), Buffer::Closed) {
            Buffer::Open(changes) => changes,
            Buffer::Closed => Vec::new(),
        })
    }
}

/// Without std no engine runs, and nothing is noted.
#[cfg(not(feature = "std"))]
mod holders {
    use super::Owed;

    pub(crate) const fn change(_: Owed, _: i64) {}
}
