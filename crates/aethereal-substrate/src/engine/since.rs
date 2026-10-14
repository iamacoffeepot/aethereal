//! The table as one actor's turn reads it: the step's start, and that actor's own spawns since (ADR-0009).

use crate::{ActorTable, ByActor};
use aethereal_actor::{Made, Table};
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
    /// The paths of the children the actor stood up in its earlier turns of the step, under their ids.
    pub(super) stood: &'a ByActor<ActorPath>,
}

impl Made for Since<'_> {
    fn made(&self, id: ActorId) -> bool {
        self.table.made(id)
    }
}

impl Table for Since<'_> {
    fn resolve(&self, path: &ActorPath) -> Option<ActorId> {
        self.table.resolve(path)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        let stood = self.stood.get(&id.bits()).cloned();

        stood.or_else(|| self.table.path(id))
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        self.table.child(parent, segment)
    }
}
