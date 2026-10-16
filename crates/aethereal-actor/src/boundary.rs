//! The boundaries an actor's values cross: `Within` and `Actors` (ADR-0004).

use crate::{Declared, SeatNumber, Seated};
use aethereal_data::{ActorId, ActorPath, Boundary, PathSegment};
use core::marker::PhantomData;

/// The table of actors as a turn reads it: as it stood when the turn's step began (ADR-0008, ADR-0011).
///
/// The engine answers it. A turn writes nothing here: what it asks for is an
/// effect, applied after it returns.
pub trait Table {
    /// The actor `path` names, and its seat, when that actor is live.
    fn resolve(&self, path: &ActorPath) -> Option<Seated>;

    /// The path `id` stands for, when an actor has been made under it, live or ended.
    fn path(&self, id: ActorId) -> Option<ActorPath>;

    /// The id of the child `segment` names beneath the actor `parent` stands for. Only the engine computes an id.
    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId;

    /// Whether the actor `id` stands for is live.
    fn is_live(&self, id: ActorId) -> bool;

    /// What the type of the actor `id` stands for declares (ADR-0013), when it has been made, live or ended, or this
    /// actor stood it up in an earlier turn of the step.
    fn declared(&self, id: ActorId) -> Option<&'static Declared>;

    /// The seat of the child `id` stands for, beneath the actor whose turn reads the table, when it was made or that
    /// actor stood it up in an earlier turn of the step.
    fn child_seat(&self, id: ActorId) -> Option<SeatNumber>;
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

/// What a writer at [`Actors`] holds beside the bytes written so far.
#[derive(Default)]
pub struct ActorsWriter<'a> {
    /// Nothing is held yet. The lifetime is kept for what a later bound value needs held.
    held: PhantomData<&'a ()>,
}

impl ActorsWriter<'_> {
    /// What a writer at [`Actors`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: PhantomData }
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

/// What a writer at [`Within`] holds beside the bytes written so far.
#[derive(Default)]
pub struct WithinWriter<'a> {
    /// Nothing is held yet. The lifetime is kept for what a later bound value needs held.
    held: PhantomData<&'a ()>,
}

impl WithinWriter<'_> {
    /// What a writer at [`Within`] holds.
    #[must_use]
    pub const fn new() -> Self {
        Self { held: PhantomData }
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
