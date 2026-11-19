//! Actors whose state is split into sections by key, whose sections' turns run side by side on the step's workers
//! (spike: keyed sections).
//!
//! Spike code. A keyed actor is seated as any root is, and registered here as well: its sections' mail is kept in
//! the parts of their lanes (`stream::lane`), not of its seat, so a step's workers each run the keys of the parts
//! they were given. Its sections live in the parts too, beside the streams, and move with them.

#[cfg(test)]
mod tests;

use super::costs::{CostKey, Tally};
use super::since::{Sees, Since};
use super::{Asked, Lists, Ran, Reading, Room, batch, item_kind, keep_lines};
use crate::stream::Run;
use crate::{ByActor, Clock};
use aethereal_actor::{Above, Chain, Effect, Keeping, PinSource, RunsSections, SeatNumber, Seated, Turning, What};
use aethereal_data::{ActorId, ActorPath, KindId};
use std::any::Any;

/// The keyed actors the engine runs, by the bits of their ids: fixed between steps, and read by every worker.
#[derive(Default)]
pub struct KeyedActors {
    by_actor: ByActor<KeyedActor>,
}

/// One keyed actor: its code and what it reads, shared by every worker that runs a section of it, and what is fixed
/// of it when it was seated.
pub struct KeyedActor {
    pub(super) runs: Box<dyn RunsSections>,
    pub(super) me: Seated,
    pub(super) path: ActorPath,
    pub(super) above: Above,
    pub(super) kind: KindId,
}

impl KeyedActors {
    /// Keeps `actor`, seated as `actor.me` says.
    pub(super) fn keep(&mut self, actor: KeyedActor) {
        self.by_actor.insert(actor.me.id.bits(), actor);
    }

    /// The keyed actor whose id's bits are `bits`, when it is one.
    pub(super) fn get(&self, bits: u64) -> Option<&KeyedActor> {
        self.by_actor.get(&bits)
    }

    /// The kind of the keyed actor whose id's bits are `bits`, when it is one: what its sections' turns are expected
    /// to cost is kept under it.
    pub(super) fn kind_of(&self, bits: u64) -> Option<KindId> {
        self.by_actor.get(&bits).map(|actor| actor.kind)
    }
}

/// One part's sections, a store for each keyed actor that has a section in the part, by the bits of its id: moved with
/// the part, so only the worker that owns the part reaches them, and nothing is merged when the step folds.
#[derive(Default)]
pub struct Sections {
    by_actor: ByActor<Box<dyn Any + Send>>,
}

impl Sections {
    /// The store of the actor whose id's bits are `bits`, made by `runs` when the part has none yet.
    fn store(&mut self, bits: u64, runs: &dyn RunsSections) -> &mut (dyn Any + Send) {
        self.by_actor.entry(bits).or_insert_with(|| runs.store()).as_mut()
    }

    /// The store of the actor whose id's bits are `bits`, when the part has one.
    pub(super) fn get(&self, bits: u64) -> Option<&(dyn Any + Send)> {
        self.by_actor.get(&bits).map(AsRef::as_ref)
    }
}

impl Reading<'_> {
    /// The turns of `run`, the items of one section of a keyed actor, each measured into `tally`, what they ask for
    /// added to `asked` and routed into `lists`. A turn that fails puts its section back as it was and drops what it
    /// asked for but its lines, and its fault is logged; the actor goes on (spike: keyed sections).
    ///
    /// Spike: a section's turns are never cut at the slice, never fused, and are not recorded for a harness.
    #[allow(clippy::too_many_arguments, reason = "a worker's own lists, lent one by one so the parcel can be split")]
    pub(super) fn keyed_turns(
        &self,
        room: &mut Room<'_>,
        run: Run,
        clock: &impl Clock,
        tally: &mut Tally,
        asked: &mut Asked,
        lists: &mut Lists,
    ) {
        let Room { streams, instances, sections, .. } = room;
        let found = self.keyed.get(run.bits);
        let is_live = found.is_some_and(|keyed| self.table.seat(keyed.me.id).is_some());
        let (Some(keyed), true) = (found, is_live) else {
            let me = Seated { id: ActorId::new(run.bits), seat: SeatNumber::new(run.seat) };
            lists.unhanded_all(asked, me, streams.items(&run));
            return;
        };
        let me = keyed.me;
        let store = sections.store(run.bits, keyed.runs.as_ref());
        let pieces = streams.items(&run);
        let carried = streams.carried();
        let pins = PinSource::step(self.mail, carried, streams.carried_pins());
        let made = super::made::Made::none();
        // A section holds no resource and asks nothing, so it keeps nothing and waits on nothing.
        let mut keeping = Keeping::default();
        let waiting = super::ask::Waiting::default();
        let key = run.section.key();
        for handed in pieces {
            tally.time(CostKey { actor: keyed.kind, item: item_kind(handed) }, clock, || {
                let table = Since {
                    table: self.table,
                    stood: &lists.fusing.stood,
                    sees: Sees::Step,
                    blobs: self.blobs,
                    now: self.now,
                    cycle: self.cycle,
                    log: self.log,
                    stats: self.stats,
                    heads: self.heads,
                };
                asked.next_turn();
                let (source, tie) = (handed.source(), handed.tie());
                let answering = waiting.answering(tie);
                let (item, context) = batch::read(handed, self, carried, &pins, answering);
                let chain = Chain { root: context.root, depth: context.depth - 1 };
                let turning = Turning::new(
                    me,
                    &keyed.path,
                    keyed.above,
                    &table,
                    &made,
                    asked,
                    instances,
                    keeping.keeps(),
                    0,
                    source,
                    chain,
                    tie,
                    handed.due(),
                );
                let outcome = keyed.runs.turn(store, key, item, turning);
                // A failed turn of a section ends no actor: its section is back as it was, and its fault is logged
                // as a closing turn's is, which marks nothing (ADR-0009). Spike: the log does not say which key.
                if let Err(fault) = outcome {
                    let buffers = asked.buffers_mut();
                    keep_lines(buffers, me);
                    buffers.effects.push(Effect::new(me, What::EndFailed(fault)));
                }
                let stood = asked.take_stood();
                lists.route(Ran { stood, context }, asked, me.id);
            });
        }
    }
}
