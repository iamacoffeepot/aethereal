//! The boundaries an actor's values cross: `Within` and `Actors` (ADR-0004).

use crate::{BlobHash, BlobId, Declared, SeatNumber, Seated};
use aethereal_data::{ActorId, ActorPath, Boundary, EncodeError, KindId, PathSegment};
#[cfg(feature = "std")]
use alloc::vec::Vec;
use core::marker::PhantomData;

/// The table of actors as a turn reads it: as it stood when the turn's step began (ADR-0008, ADR-0011).
///
/// The engine answers it. A turn writes nothing here: what it asks for is an
/// effect, applied after it returns.
pub trait Table {
    /// The actor `path` names, and its seat, when that actor is live.
    fn resolve(&self, path: &ActorPath) -> Option<Seated>;

    /// The id of the child `segment` names beneath the actor `parent` stands for. Only the engine computes an id.
    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId;

    /// Whether the actor `id` stands for is live.
    fn is_live(&self, id: ActorId) -> bool;

    /// What the type of the actor `id` stands for declares (ADR-0013), when it has been made, live or ended, or this
    /// actor stood it up in an earlier turn of the step.
    fn declared(&self, id: ActorId) -> Option<&'static Declared>;

    /// Whether the type of the actor `id` stands for lists every one of `kinds`, as [`Self::declared`] finds it: what
    /// a turn's cast to a protocol reads (ADR-0016). A table that holds no other actor's type asks whoever does.
    fn lists(&self, id: ActorId, kinds: &[KindId]) -> bool {
        self.declared(id).is_some_and(|declared| declared.lists_all(kinds))
    }

    /// Whether the table held the actor `id` stands for as the step began, or it was stood up since in this part of
    /// the engine: whether, for whoever holds a reference to it, its ending is still to come (ADR-0008).
    fn held(&self, id: ActorId) -> bool;

    /// The seat of the child `id` stands for, beneath the actor whose turn reads the table, when it was made or that
    /// actor stood it up in an earlier turn of the step.
    fn child_seat(&self, id: ActorId) -> Option<SeatNumber>;

    /// The bytes of the blob `id` names, when the store held it as the step began (ADR-0014).
    fn blob(&self, id: BlobId) -> Option<&[u8]>;

    /// The hash the store kept for the blob `id` names as the step began, when it held one.
    fn blob_hash(&self, id: BlobId) -> Option<BlobHash>;
}

/// The children an actor has made, live or ended, whose keys it may not use again (ADR-0008).
///
/// The engine answers it from the actor's seat, where a parent's own
/// knowledge of its keys is kept.
pub trait Children {
    /// Whether the actor has made the child `child` stands for.
    fn has_made(&self, child: ActorId) -> bool;
}

/// An actor that has made no child: one being stood up, or one whose turn cannot spawn.
impl Children for () {
    fn has_made(&self, _: ActorId) -> bool {
        false
    }
}

/// What a boundary's writer and reader offer a reference: leave to cross, and nothing else (ADR-0008).
///
/// A reference means something only inside the engine that holds its actor,
/// so it is written and read only at a boundary that stays inside one engine.
/// It was proven when it was made, so nothing is looked up to read it.
pub trait Refers {}

/// The boundary between actors in one engine: what mail and a config cross (ADR-0004).
pub struct Actors;

impl Boundary for Actors {
    type WriterOffers<'a> = ActorsWriter<'a>;
    type ReaderOffers<'a> = ActorsReader<'a>;
}

/// The boundary of one actor's own state: stored, and read back by that actor (ADR-0004).
pub struct Within;

impl Boundary for Within {
    type WriterOffers<'a> = WithinWriter<'a>;
    type ReaderOffers<'a> = WithinReader<'a>;
}

/// What a writer at [`Actors`] holds beside the bytes written so far: the pins of the blobs it refers to, each at
/// where its reference begins (ADR-0014), and where it notes the resources written, when they are carried on
/// (ADR-0010).
#[cfg(feature = "std")]
pub struct ActorsWriter<'a> {
    held: &'a mut Vec<crate::Pinning>,
    carrying: Carrying<'a>,
}

/// Whether a writer at [`Actors`] carries on the resources it writes.
#[cfg(feature = "std")]
enum Carrying<'a> {
    /// It does not: a resource written there is refused.
    Nothing,
    /// It notes the ticket of each into this list, for the turn that writes to carry it with the mail.
    Into(&'a mut Vec<u64>),
}

#[cfg(feature = "std")]
impl<'a> ActorsWriter<'a> {
    /// A writer at [`Actors`] that keeps the pins of what it writes in `held`, and carries no resource.
    #[must_use]
    pub const fn new(held: &'a mut Vec<crate::Pinning>) -> Self {
        Self { held, carrying: Carrying::Nothing }
    }

    /// A writer at [`Actors`] that keeps the pins of what it writes in `held`, and notes in `tickets` the ticket of
    /// each resource it writes, to carry on (ADR-0010).
    #[must_use]
    pub const fn carrying(held: &'a mut Vec<crate::Pinning>, tickets: &'a mut Vec<u64>) -> Self {
        Self { held, carrying: Carrying::Into(tickets) }
    }

    /// Keeps `pin`, for a reference whose bytes begin at `at`.
    pub(crate) fn hold(&mut self, at: u32, pin: crate::BlobRef) {
        self.held.push((at, pin));
    }

    /// Notes that the resource `ticket` names is written, to carry on: refused by a writer that carries nothing.
    pub(crate) fn carry(&mut self, ticket: u64) -> Result<(), EncodeError> {
        match &mut self.carrying {
            Carrying::Into(tickets) => {
                tickets.push(ticket);
                Ok(())
            }
            Carrying::Nothing => Err(EncodeError::NotCarried { name: "Given" }),
        }
    }
}

/// What a writer at [`Actors`] holds beside the bytes written so far.
#[cfg(not(feature = "std"))]
#[derive(Default)]
pub struct ActorsWriter<'a> {
    /// Nothing is held: no blob can be referred to without the pins `std` keeps.
    held: PhantomData<&'a ()>,
}

#[cfg(not(feature = "std"))]
impl ActorsWriter<'_> {
    /// What a writer at [`Actors`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: PhantomData }
    }

    /// Refuses the resource `ticket` names: without `std` nothing carries one.
    #[expect(clippy::unused_self, reason = "the same call as the writer that carries resources")]
    pub(crate) const fn carry(&mut self, _: u64) -> Result<(), EncodeError> {
        Err(EncodeError::NotCarried { name: "Given" })
    }
}

impl Refers for ActorsWriter<'_> {}

/// What a reader at [`Actors`] holds beside the bytes still to read and the budget.
#[derive(Default)]
pub struct ActorsReader<'a> {
    /// Nothing is held yet. The lifetime is kept for what a later bound value needs held.
    held: PhantomData<&'a ()>,
}

impl ActorsReader<'_> {
    /// What a reader at [`Actors`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: PhantomData }
    }
}

impl Refers for ActorsReader<'_> {}

/// What a writer at [`Within`] holds beside the bytes written so far: where it notes the blobs written, when it is
/// asked to.
#[derive(Default)]
pub struct WithinWriter<'a> {
    #[cfg_attr(not(feature = "std"), expect(dead_code, reason = "no blob is referred to without std"))]
    noting: Noting<'a>,
}

/// Whether a writer at [`Within`] notes the blobs it writes references to.
#[derive(Default)]
enum Noting<'a> {
    #[default]
    No,
    /// Into this list, each once for each reference.
    #[cfg(feature = "std")]
    Into(&'a mut Vec<BlobId>),
    /// Nothing is noted without std, where no blob is referred to; the lifetime is kept for the list it would hold.
    #[cfg(not(feature = "std"))]
    #[expect(dead_code, reason = "no blob is referred to without std")]
    Into(PhantomData<&'a ()>),
}

impl<'a> WithinWriter<'a> {
    /// What a writer at [`Within`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { noting: Noting::No }
    }

    /// A writer at [`Within`] that notes in `blobs` the blob of each reference it writes: for a guest's library, which
    /// tells its host what an actor's state taken out refers to (ADR-0014, ADR-0015).
    #[cfg(feature = "std")]
    #[must_use]
    pub const fn noting(blobs: &'a mut Vec<BlobId>) -> Self {
        Self { noting: Noting::Into(blobs) }
    }

    /// Notes that a reference to the blob `blob` is written, when it is asked to.
    #[cfg(feature = "std")]
    pub(crate) fn note(&mut self, blob: BlobId) {
        if let Noting::Into(blobs) = &mut self.noting {
            blobs.push(blob);
        }
    }
}

impl Refers for WithinWriter<'_> {}

/// What a reader at [`Within`] holds beside the bytes still to read and the budget.
#[derive(Default)]
pub struct WithinReader<'a> {
    /// Nothing is held yet. The lifetime is kept for what a later bound value needs held.
    held: PhantomData<&'a ()>,
}

impl WithinReader<'_> {
    /// What a reader at [`Within`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: PhantomData }
    }
}

impl Refers for WithinReader<'_> {}
