//! Actors whose state is split into sections by key, whose sections' turns run side by side on the step's workers
//! (spike: keyed sections).
//!
//! Spike code. A keyed actor is seated as any root is, and registered here as well: its sections' mail is kept in
//! the parts of their lanes (`stream::lane`), not of its seat, so a step's workers each run the keys of the parts
//! they were given. Its sections live in the parts too, beside the streams, and move with them: each section's state,
//! and what it keeps of requests, the contexts of those it asked and the requests it holds, as an ordinary actor's
//! are kept beside it in its seat.

#[cfg(test)]
mod tests;

use super::ask::{Answering, Waiting};
use super::costs::{CostKey, Tally};
use super::debts::{Debt, Debts};
use super::since::{Sees, Since};
use super::{Asked, Engine, Lists, ROOTS, Ran, Reading, Room, ask, batch, item_kind, parity, tied, view_part};
use crate::stream::{self, Run};
use crate::{ByActor, Clock};
use aethereal_actor::{
    Above, Chain, Due, Keeping, KeyBits, Mailed, Owed, PinSource, RunsSections, SeatNumber, Seated, SectionKey, SentAs,
    Stamps, Tie, Turning, Unanswered,
};
use aethereal_data::{ActorId, ActorPath, KindId};
use std::any::Any;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

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

    /// Forgets each keyed actor `is_gone` says has gone, and answers with where each sat, for their sections to be
    /// swept out of every part.
    pub(super) fn forget_gone(&mut self, is_gone: impl Fn(Seated) -> bool) -> Vec<Seated> {
        let gone: Vec<Seated> = self.by_actor.values().map(|keyed| keyed.me).filter(|me| is_gone(*me)).collect();
        for me in &gone {
            self.by_actor.remove(&me.id.bits());
        }

        gone
    }
}

/// One part's sections, by the keyed actor they are of: moved with the part, so only the worker that owns the part
/// reaches them, and nothing is merged when the step folds.
pub struct Sections {
    /// The number of the part, which a keyed actor's sections in it number their mail from.
    part: u64,
    by_actor: ByActor<ActorSections>,
}

/// One keyed actor's sections in one part.
struct ActorSections {
    /// Their states, in the store the actor's runner made, by key.
    store: Box<dyn Any + Send>,
    /// What each section that waits on an answer or owes one keeps of requests, by key. A section that does neither
    /// has no entry, and an entry is never empty between runs.
    held: HashMap<u64, SectionHeld, BuildHasherDefault<KeyBits>>,
    /// The count of the last piece of mail the actor's sections in this part sent. The part numbers their mail, not
    /// the section: one part runs on one worker in a step, so no two of its sections send at once, and a section
    /// dropped and made again goes on from the part's count, never numbering its mail as mail still on its way.
    sent: u64,
}

/// What one section keeps of requests: the contexts of those it asked, waiting for their answers, and the requests it
/// holds, until it answers them or lets them go (ADR-0017).
#[derive(Default)]
pub(super) struct SectionHeld {
    pub(super) waiting: Waiting,
    pub(super) debts: Debts,
}

impl SectionHeld {
    /// Whether it waits on nothing and owes nothing.
    fn is_empty(&self) -> bool {
        self.waiting.is_empty() && self.debts.is_empty()
    }
}

/// Where the count of the mail a keyed actor's sections in part `part` send starts: above the actor's own, which
/// start at one, and apart from every other part's. Spike: a part's sections send fewer than `2^40` pieces of mail.
const fn first_count(part: u64) -> u64 {
    (part + 1) << 40
}

impl Sections {
    /// The sections of part `part`, none yet.
    pub(super) fn of_part(part: usize) -> Self {
        Self { part: part as u64, by_actor: ByActor::default() }
    }

    /// The sections of the actor whose id's bits are `bits`, made empty by `runs` when the part has none yet.
    fn of(&mut self, bits: u64, runs: &dyn RunsSections) -> &mut ActorSections {
        let part = self.part;

        self.by_actor.entry(bits).or_insert_with(|| ActorSections {
            store: runs.store(),
            held: HashMap::default(),
            sent: first_count(part),
        })
    }

    /// The store of the actor whose id's bits are `bits`, when the part has one.
    pub(super) fn get(&self, bits: u64) -> Option<&(dyn Any + Send)> {
        self.by_actor.get(&bits).map(|actor| actor.store.as_ref())
    }

    /// Changes how many values hold the debt `owed` names, which the section `owed.section` of its holder holds, by
    /// `by`, and answers with it when none is left: let go of, as [`Debts::change`] does. A section left neither
    /// waiting nor owing keeps nothing of requests. Its state is dropped, if it is unused, at its next run.
    pub(super) fn change(&mut self, owed: Owed, by: i64) -> Option<Debt> {
        let key = owed.section.key();
        let actor = self.by_actor.get_mut(&owed.holder.id.bits())?;
        let held = actor.held.get_mut(&key)?;
        let debt = held.debts.change(owed, by);
        if held.is_empty() {
            actor.held.remove(&key);
        }

        debt
    }

    /// Whether the section of `key` of the actor whose id's bits are `bits` waits on an answer or owes one.
    pub(super) fn holds(&self, bits: u64, key: u64) -> bool {
        self.by_actor.get(&bits).is_some_and(|actor| actor.held.contains_key(&key))
    }

    /// Takes out everything the part keeps of the actor whose id's bits are `bits`: what each of its sections kept of
    /// requests. Its states are dropped.
    pub(super) fn take(&mut self, bits: u64) -> Vec<SectionHeld> {
        self.by_actor.remove(&bits).map_or_else(Vec::new, |actor| actor.held.into_values().collect())
    }
}

impl Reading<'_> {
    /// The turns of `run`, the items of one section of a keyed actor, each measured into `tally`, what they ask for
    /// added to `asked` and routed into `lists`. The section's requests are filed beside it, as an ordinary actor's
    /// are beside it in its seat: an answer is handed with the context the section kept, and what the section holds
    /// stays with it. A turn that fails ends the whole actor, as any failed turn does (ADR-0009).
    ///
    /// Spike: a section's turns are never cut at the slice, never fused, and are not recorded for a harness; a
    /// deadline on what a section asks is never kept.
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
        let key = run.section.key();
        let actor = sections.of(run.bits, keyed.runs.as_ref());
        let mut held = actor.held.remove(&key).unwrap_or_default();
        let mut pieces = streams.items(&run);
        let carried = streams.carried();
        let pins = PinSource::step(self.mail, carried, streams.carried_pins());
        let made = super::made::Made::none();
        // A section holds no resource, so it keeps nothing. Spike: what it asks with a deadline is never expired.
        let mut keeping = Keeping::default();
        let mut expiring = Vec::new();
        let mut ended = false;
        while let Some(handed) = pieces.next() {
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
                let (source, tie, returns) = (handed.source(), handed.tie(), handed.returns());
                let answering = held.waiting.answering(tie);
                // An answer to nothing the section waits on runs no turn.
                if matches!(answering, Answering::Stray) {
                    return;
                }
                let (item, context) = batch::read(handed, self, carried, &pins, answering);
                let chain = Chain { root: context.root, depth: context.depth - 1 };
                let sent = actor.sent;
                let turning = Turning::new(
                    me,
                    &keyed.path,
                    keyed.above,
                    &table,
                    &made,
                    asked,
                    instances,
                    keeping.keeps(),
                    sent,
                    source,
                    chain,
                    tie,
                    handed.due(),
                )
                .stamped(Stamps { section: run.section, returns });
                let mut outcome = keyed.runs.turn(actor.store.as_mut(), key, item, turning);
                // Most turns are of plain mail, and ask and hold nothing.
                let is_tied = !tie.is_tell() || asked.is_tied();
                if is_tied {
                    let filed = (&mut held.waiting, &mut held.debts, &mut expiring);
                    tied(me, (source, tie, context, sent, returns), filed, asked, &mut outcome);
                }
                let ran = Ran::of(me, outcome, asked, context);
                let seen = lists.route(ran, asked, me.id);
                actor.sent = seen.sent.map_or(actor.sent, |last| last.max(actor.sent));
                ended = seen.ends;
            });
            // What the section was handed behind the turn that ended its actor is never handed to it.
            if ended {
                lists.unhanded_all(asked, me, pieces);
                break;
            }
        }
        // A section that waits on an answer or owes one keeps what it holds, and is never dropped: an answer still to
        // come finds its context, and a debt its holder. One that does neither is dropped once it is unused.
        if held.is_empty() {
            keyed.runs.release(actor.store.as_mut(), key);
        } else {
            actor.held.insert(key, held);
        }
    }
}

impl Engine {
    /// Sweeps the sections of every keyed actor that has gone out of every part, once its last turns have run: the
    /// asker of each request a section still holds is told it went unanswered, `Ended`, as an ordinary actor's are
    /// after its closing turn, and the contexts its sections kept for their own requests are let go of, so the answers
    /// to them are dropped as mail to an actor that has gone is (ADR-0017). Run as a step is applied.
    ///
    /// Spike: a keyed actor is gone once the table no longer seats it, which is the step after its closing turn; until
    /// then its sections run, as the table read as their step began says it is there.
    pub(super) fn sweep_sections(&mut self) {
        let table = &self.table;
        let gone = self.keyed.forget_gone(|me| table.seat(me.id).is_none());
        // Most steps end no keyed actor.
        if gone.is_empty() {
            return;
        }
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut entries = Vec::new();
        for me in gone {
            for sections in &mut self.sections {
                for mut held in sections.take(me.id.bits()) {
                    for debt in held.debts.take_all() {
                        let Ok(bytes) = arena.write_mail(&Unanswered::Ended) else {
                            unreachable!("a reason why a request was not answered is a byte, and refers to nothing");
                        };
                        let mailed = Mailed {
                            by: me,
                            sent_as: SentAs::Engine,
                            to: debt.asker,
                            count: ask::ended_order(debt.request),
                            kind: KindId::of::<Unanswered>(),
                            bytes,
                            tie: Tie::unanswered(debt.request),
                            due: Due::Never,
                            section: debt.returns,
                            returns: SectionKey::WHOLE,
                        };
                        entries.push(stream::entry(mailed, &debt.chain));
                    }
                }
            }
        }
        self.arenas[parity][ROOTS] = arena;
        let mut parts: Vec<usize> = entries.iter().map(|(key, _)| view_part(*key, self.sections.len())).collect();
        self.streams.fold_records(entries, &self.arenas[parity]);
        parts.sort_unstable();
        parts.dedup();
        for part in parts {
            self.expected[part] = self.expect(part);
            self.waiting.push(part);
        }
    }
}

impl Engine {
    /// Whether any part keeps anything of the keyed actor `actor`: a section's state, or what a section keeps of
    /// requests (spike: keyed sections).
    #[must_use]
    pub fn keeps_sections(&self, actor: ActorId) -> bool {
        self.sections.iter().any(|sections| sections.by_actor.contains_key(&actor.bits()))
    }

    /// Whether the section of `key` of the keyed actor `actor` waits on an answer or owes one, as of the last step
    /// (spike: keyed sections).
    #[must_use]
    pub fn section_holds(&self, actor: ActorId, key: u64) -> bool {
        let lane = stream::lane(actor.bits(), SectionKey::of(key));
        let part = view_part(lane, self.sections.len());

        self.sections[part].holds(actor.bits(), key)
    }
}
