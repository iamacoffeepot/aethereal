//! The end of a step: each part of the views folded by itself, on the workers (ADR-0011).

use super::birth::{self, Birth};
use super::costs::{CostKey, Costs, ItemKind};
use super::gift::{self, Arrival};
use super::{Held, Seating, Sitting, item_kind};
use crate::stages::Stages;
use crate::stream::{self, Streams};
use crate::table::Actors;
use crate::watch::{Notice, Watches};
use crate::{BySeat, Column, Lives, Log, Part, lives};
use aethereal_actor::{Arena, Seated};
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
    pub(super) stages: Column<Stages>,
    pub(super) seating: Seating,
    /// The part's actors, which the children routed to it join.
    pub(super) actors: BySeat<Held>,
    /// Each writer's children for the part, and notes of children for its parents, in the writers' order: emptied as
    /// they are seated, and handed back to their rows for their room.
    pub(super) births: Vec<Vec<Birth>>,
    /// How many children were seated in the part when it was folded.
    pub(super) born: usize,
    /// Each writer's resources on their way to the part's actors, in the writers' order: emptied as they are handed
    /// over, and handed back to their rows for their room (ADR-0010).
    pub(super) gifts: Vec<Vec<Arrival>>,
    /// The givers and names of the resources whose receivers had gone, for the givers' logs, which may lie in any
    /// part.
    pub(super) lost: Vec<(Seated, &'static str)>,
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
            stages: Column::empty(parts),
            seating: Seating { seated: BySeat::empty(parts) },
            actors: BySeat::empty(parts),
            births: Vec::new(),
            born: 0,
            gifts: Vec::new(),
            lost: Vec::new(),
            forward: Vec::new(),
            expected: 0,
        }
    }
}

/// The engine as a step's workers read it while they fold: what turns are expected to cost (ADR-0011).
pub struct Folding<'a> {
    pub(super) costs: &'a Costs,
    /// How long an actor kept to a slice runs in a step, in nanoseconds.
    pub(super) slice: u64,
    /// The step's arenas, by writer, where the text of the lines its turns logged lies.
    pub(super) written: &'a [Arena],
}

impl Folding<'_> {
    /// Folds each part of `fold`: the children every worker stood up in it are seated, the resources given to its
    /// actors are handed to them, every worker's records for it are folded, view by view, then the part's watches settle, and what its next turns are expected to cost is
    /// summed.
    #[must_use]
    pub fn fold(&self, mut fold: Fold) -> Fold {
        for pile in &mut fold.parts {
            for bucket in &mut pile.births {
                pile.born += birth::seat_all(&mut pile.seating, &mut pile.actors, bucket);
            }
            for bucket in &mut pile.gifts {
                gift::arrive_all(&mut pile.actors, bucket, &mut pile.lost);
            }
            pile.table.fold(self.written);
            pile.log.fold(self.written);
            pile.lives.fold(self.written);
            pile.streams.fold(self.written);
            pile.watches.fold(self.written);
            pile.stages.fold(self.written);
            pile.watches.shard_mut().settle(&mut pile.forward);
            pile.expected =
                expected(pile.streams.shard(), pile.lives.shard(), &pile.seating.seated, self.costs, self.slice);
        }

        fold
    }
}

/// What the turns waiting in one part are expected to cost, in nanoseconds: one for each item that has arrived for
/// an actor seated there, by the actor's kind, and each closing turn to run.
pub(super) fn expected(
    streams: &stream::Shard,
    lives: &lives::Shard,
    seated: &BySeat<Sitting>,
    costs: &Costs,
    slice: u64,
) -> u64 {
    // Items that arrive together are mostly for one kind of actor, of one kind, so the last cost looked up is kept.
    let mut last = None;
    let mut cost = |me: Seated, item: ItemKind| {
        let Some(actor) = seated.get(me).map(|sitting| sitting.kind) else {
            return 0;
        };
        let key = CostKey { actor, item };
        match last {
            Some((kept, cost)) if kept == key => cost,
            _ => {
                let cost = costs.expect(&key);
                last = Some((key, cost));

                cost
            }
        }
    };
    let items: u64 =
        streams.arrived().map(|(me, handed, pieces)| cost(me, item_kind(handed)) * u64::from(pieces)).sum();
    let carried: u64 = streams
        .backlog(super::CARRIED_WEIGHED)
        .map(|(me, handed, pieces)| (cost(me, item_kind(handed)) * pieces).min(slice))
        .sum();
    let items = items + carried;
    let closings: u64 = lives.ended().iter().map(|me| cost(*me, ItemKind::Closing)).sum();

    items + closings
}
