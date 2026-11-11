//! The table as one actor's turn reads it: the step's start, the children stood up in its part since, and what its
//! run has done (ADR-0009, ADR-0013).

use super::fuse::Run;
use super::lifecycle::runs;
use crate::heads::{HEADS, Head, Heads};
use crate::id::path_id;
use crate::log::{find, recent};
use crate::stats::Stats;
use crate::table::reach;
use crate::{ActorTable, Blobs, ByActor, Folded, Log};
use aethereal_actor::{
    ActorName, BlobHash, BlobId, Bytes, Cycling, Declared, EngineStats, Logged, Recent, SeatNumber, Seated, Table, Time,
};
use aethereal_data::{ActorId, ActorPath, KindId, KindName, PathSegment};

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
    /// The blobs the store held as the step began (ADR-0014).
    pub(super) blobs: &'a Blobs,
    /// The time the step carries (ADR-0018).
    pub(super) now: Time,
    /// The stages of the chassis's cycle (ADR-0020).
    pub(super) cycle: &'a [Cycling],
    /// The log as the step began (ADR-0044).
    pub(super) log: &'a Folded<Log>,
    /// What the engine keeps for its figures, as the step began (ADR-0053).
    pub(super) stats: &'a Stats,
    /// The heads as the step began (ADR-0054).
    pub(super) heads: &'a Folded<Heads>,
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

    fn reach(&self, path: &ActorPath, kind: KindId) -> Seated {
        reach(self, path, kind)
    }

    fn reach_child(&self, parent: ActorId, segment: &PathSegment, kind: KindId) -> Option<Seated> {
        let Some(run) = self.run() else {
            return self.table.reach_child(parent, segment, kind);
        };
        let id = self.table.child(parent, segment);
        if run.has_ended(id) {
            return None;
        }
        let Some(seat) = run.stood(id) else {
            return self.table.reach_child(parent, segment, kind);
        };
        let is_kind = Table::declared(self, id).is_some_and(|declared| declared.id() == kind);

        is_kind.then_some(Seated { id, seat })
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

    fn held(&self, id: ActorId) -> bool {
        let is_stood = self.stood.contains_key(&id.bits());

        is_stood || self.table.is_live(id)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        let stood = self.stood.get(&id.bits()).map(|(_, seat, _)| *seat);

        stood.or_else(|| self.table.seat(id))
    }

    fn declared(&self, id: ActorId) -> Option<&'static Declared> {
        let stood = self.stood.get(&id.bits()).map(|(_, _, declared)| *declared);

        stood.or_else(|| Table::declared(self.table, id))
    }

    fn blob(&self, id: BlobId) -> Option<&[u8]> {
        self.blobs.read(id)
    }

    fn blob_hash(&self, id: BlobId) -> Option<BlobHash> {
        self.blobs.hash(id)
    }

    /// A pin of its own on the head's blob, as the heads view stood when the step began (ADR-0054).
    fn head(&self, kind: &KindName) -> Option<Bytes> {
        self.heads.shard(HEADS).head(kind).map(Head::blob).cloned()
    }

    fn now(&self) -> Time {
        self.now
    }

    fn runs_stage(&self, stage: KindId) -> bool {
        runs(self.cycle, stage)
    }

    /// A live actor's log is read at its seat; an ended one's, by its id, in the seat it last logged in (ADR-0044).
    fn logged(&self, path: &ActorPath) -> Logged<'_> {
        let live = Table::resolve(self.table, path);
        let kept = live
            .map_or_else(|| find(self.log, path_id(path)), |seated| self.log.shard(seated.seat.number()).get(seated));

        kept.map_or(Logged::Nothing, Logged::Kept)
    }

    /// Every part's ring is read where it lies, each actor named by the table, live or ended recently (ADR-0053).
    fn recent(&self, since: u64) -> Recent<'_> {
        recent(self.log, since, |id| self.table.name(id).map_or(ActorName::Unnamed(id), ActorName::Path))
    }

    /// The figures are read where they lie, each actor named as the log names it (ADR-0053).
    fn engine_stats(&self) -> EngineStats {
        let name = |id| self.table.name(id).map_or(ActorName::Unnamed(id), ActorName::Path);

        EngineStats::Read(self.stats.figures(self.blobs, name))
    }
}
