//! The end of a step: each part of the views folded by itself, on the workers (ADR-0011).

use super::costs::{CostKey, Costs, ItemKind};
use super::{Seating, Sitting, item_kind};
use crate::stream::{self, Streams};
use crate::table::Actors;
use crate::watch::{Notice, Watches};
use crate::{BySeat, Column, Lives, Log, Part, lives};
use aethereal_actor::Seated;
use std::num::NonZeroUsize;

/// The parts of the views one worker folds at the end of a step (ADR-0011).
pub struct Fold {
    pub(super) parts: Vec<Pile>,
}

/// One part of every view, taken out with what every worker routed to it in the step, and the part's seats, which
/// say what its next turns are expected to cost.
pub(super) struct Pile {
    pub(super) part: usize,
    pub(super) table: Column<Actors>,
    pub(super) log: Column<Log>,
    pub(super) lives: Column<Lives>,
    pub(super) streams: Column<Streams>,
    pub(super) watches: Column<Watches>,
    pub(super) seating: Seating,
    /// What the part's watches hand on once it settles: endings, for the streams of the actors that watch.
    pub(super) forward: Vec<Notice>,
    /// What the turns waiting in the part are expected to cost, in nanoseconds, once it is folded.
    pub(super) expected: u64,
}

impl Pile {
    /// An empty pile, for views kept in `parts` parts: to be filled and emptied again step after step.
    pub(super) fn empty(parts: NonZeroUsize) -> Self {
        Self {
            part: 0,
            table: Column::empty(parts),
            log: Column::empty(parts),
            lives: Column::empty(parts),
            streams: Column::empty(parts),
            watches: Column::empty(parts),
            seating: Seating { seated: BySeat::empty(parts) },
            forward: Vec::new(),
            expected: 0,
        }
    }
}

/// The engine as a step's workers read it while they fold: what turns are expected to cost (ADR-0011).
pub struct Folding<'a> {
    pub(super) costs: &'a Costs,
}

impl Folding<'_> {
    /// Folds each part of `fold`: every worker's records for it, view by view, then the part's watches settle, and
    /// what its next turns are expected to cost is summed.
    #[must_use]
    pub fn fold(&self, mut fold: Fold) -> Fold {
        for pile in &mut fold.parts {
            pile.table.fold();
            pile.log.fold();
            pile.lives.fold();
            pile.streams.fold();
            pile.watches.fold();
            pile.watches.shard_mut().settle(&mut pile.forward);
            pile.expected = expected(pile.streams.shard(), pile.lives.shard(), &pile.seating.seated, self.costs);
        }

        fold
    }
}

/// What the turns waiting in one part are expected to cost, in nanoseconds: one for each item that has arrived for
/// an actor seated there, by the actor's kind, and each closing turn to run.
pub(super) fn expected(streams: &stream::Shard, lives: &lives::Shard, seated: &BySeat<Sitting>, costs: &Costs) -> u64 {
    let cost = |me: Seated, item: ItemKind| {
        let kind = seated.get(me).map(|sitting| sitting.kind);

        kind.map_or(0, |actor| costs.expect(&CostKey { actor, item }))
    };
    let items: u64 = streams.arrived().map(|(me, handed)| cost(me, item_kind(handed))).sum();
    let closings: u64 = lives.ended().iter().map(|me| cost(*me, ItemKind::Closing)).sum();

    items + closings
}
