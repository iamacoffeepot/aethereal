//! The table as one actor's turn reads it: the step's start, and that actor's own spawns since (ADR-0009).

use crate::ActorTable;
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
    pub(super) stood: &'a [(ActorId, ActorPath)],
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
        let stood = self.stood.iter().find(|(child, _)| *child == id).map(|(_, path)| path.clone());

        stood.or_else(|| self.table.path(id))
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        self.table.child(parent, segment)
    }
}
