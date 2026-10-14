//! The table as one actor's turn reads it: the step's start, and that actor's own spawns since (ADR-0009).

use crate::{ActorTable, ByActor};
use aethereal_actor::{Made, SeatNumber, Seated, Table};
use aethereal_data::{ActorId, ActorPath, PathSegment};

/// The table as one actor's turn reads it (ADR-0009).
///
/// Only an actor spawns beneath itself, so it knows whether a key is free
/// from the table as the step began and from what it has itself stood up
/// since, in its earlier turns of this step. Those children are not live to
/// anyone yet: a resolve does not find them, and a reference to one is not
/// yet proven.
pub(super) struct Since<'a> {
    pub(super) table: &'a ActorTable,
    /// The paths and seats of the children the actor stood up in its earlier turns of the step, under their ids.
    pub(super) stood: &'a ByActor<(ActorPath, SeatNumber)>,
}

/// A child stood up in an earlier turn is not yet proven to anyone, so a reference to it is not read back.
impl Made for Since<'_> {
    fn seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.table.seat(id)
    }
}

impl Table for Since<'_> {
    fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        Table::resolve(self.table, path)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        let stood = self.stood.get(&id.bits()).map(|(path, _)| path.clone());

        stood.or_else(|| self.table.path(id))
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        self.table.child(parent, segment)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        let stood = self.stood.get(&id.bits()).map(|(_, seat)| *seat);

        stood.or_else(|| self.table.seat(id))
    }
}
