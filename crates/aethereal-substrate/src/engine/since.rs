//! The table as one actor's turn reads it: the step's start, the children stood up in its part since, and what its
//! run has done (ADR-0009, ADR-0013).

use super::fuse::Run;
use crate::{ActorTable, ByActor};
use aethereal_actor::{Declared, SeatNumber, Seated, Table};
use aethereal_data::{ActorId, ActorPath, PathSegment};

/// The table as one actor's turn reads it (ADR-0009, ADR-0013).
///
/// Only an actor spawns beneath itself, so it knows whether a key is free
/// from the table as the step began and from the children stood up in its
/// part since, in earlier turns of this step. Those children are not live to
/// anyone yet: a resolve does not find them. A turn of a run sees, over that,
/// the children its run stood up as live, and the actors that ended in its run
/// as ended.
pub(super) struct Since<'a> {
    pub(super) table: &'a ActorTable,
    /// The paths and seats of the children stood up in the part in earlier turns of the step, and what their types
    /// declare, under their ids.
    pub(super) stood: &'a ByActor<(ActorPath, SeatNumber, &'static Declared)>,
    /// What else the turn sees.
    pub(super) sees: Sees<'a>,
}

/// What a turn sees over the table as the step began and the children stood up since (ADR-0013).
#[derive(Clone, Copy)]
pub(super) enum Sees<'a> {
    /// Nothing more: the turn is not in a run.
    Step,
    /// The spawns and endings of the run the turn is in.
    Run(&'a Run),
}

impl Since<'_> {
    /// The run the turn is in, when it is in one.
    const fn run(&self) -> Option<&Run> {
        match self.sees {
            Sees::Step => None,
            Sees::Run(run) => Some(run),
        }
    }
}

impl Table for Since<'_> {
    fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        let Some(run) = self.run() else {
            return Table::resolve(self.table, path);
        };
        let resolved = run.resolve(path).or_else(|| Table::resolve(self.table, path))?;
        let has_ended = run.has_ended(resolved.id);

        (!has_ended).then_some(resolved)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        let stood = self.stood.get(&id.bits()).map(|(path, _, _)| path.clone());

        stood.or_else(|| self.table.path(id))
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        self.table.child(parent, segment)
    }

    fn is_live(&self, id: ActorId) -> bool {
        let Some(run) = self.run() else {
            return Table::is_live(self.table, id);
        };
        if run.has_ended(id) {
            return false;
        }
        let stood_up = run.stood(id).is_some();

        stood_up || Table::is_live(self.table, id)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        let stood = self.stood.get(&id.bits()).map(|(_, seat, _)| *seat);

        stood.or_else(|| self.table.seat(id))
    }

    fn declared(&self, id: ActorId) -> Option<&'static Declared> {
        let stood = self.stood.get(&id.bits()).map(|(_, _, declared)| *declared);

        stood.or_else(|| Table::declared(self.table, id))
    }
}
