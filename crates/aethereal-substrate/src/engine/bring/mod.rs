//! What the chassis brings into the engine between steps, through the sources actors own (ADR-0010).
//!
//! The chassis pushes what happens outside the engine onto a source's feed,
//! whenever it happens, and the engine holds none of it until the chassis
//! brings it in between two steps. Each event then reaches whoever owns the
//! source as mail from that owner, the root of a chain of its own (ADR-0012),
//! and is handled in the next step.

#[cfg(test)]
mod tests;

use super::gift::{self, Arrival};
use super::{Engine, ROOTS, parity};
use crate::{Context, lives, log, stream};
use aethereal_actor::{Actors, Arena, Controller, Gift, Given, MailId, Mailed, Requests, Seated, Source, Tie};
use aethereal_data::{ActorId, Encode, Kind, KindId};
use crossbeam_queue::SegQueue;
use std::any;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The chassis's end of a source: what it pushes events of kind `K` onto, as they happen, for the engine to bring to
/// the source's owner at the next [`Engine::bring`] (ADR-0010).
///
/// It is cloned for each place the chassis hears of such events, and every
/// clone pushes onto the one feed. It is closed once its source is dropped.
/// The feed is a queue no one waits on: a push never waits for another, nor
/// for the engine, and the engine never waits for a push.
pub struct Feed<K> {
    fed: Arc<Fed<K>>,
}

/// What a feed holds between two brings.
struct Fed<K> {
    /// The events pushed and not yet brought, in the order pushed.
    events: SegQueue<K>,
    /// Whether its source has been dropped: nothing pushed then reaches an actor.
    closed: AtomicBool,
}

/// Why an event was not pushed: the source it was for has been dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

impl<K> Feed<K> {
    /// Pushes `event`, for the engine to bring to the source's owner at the next bring: refused once the source has
    /// been dropped.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the source has been dropped.
    pub fn push(&self, event: K) -> Result<(), Closed> {
        if self.is_closed() {
            return Err(Closed);
        }
        // An event pushed as the feed closes is dropped with it, as one pushed and not yet brought is.
        self.fed.events.push(event);

        Ok(())
    }

    /// Whether the source has been dropped, as of the last bring: the chassis then stops what it fed it from.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.fed.closed.load(Ordering::Acquire)
    }
}

impl<K> Clone for Feed<K> {
    fn clone(&self) -> Self {
        Self { fed: Arc::clone(&self.fed) }
    }
}

/// A feed as the engine brings from it, whatever its kind.
trait Brings: Send + Sync {
    /// Writes each event pushed before this bring began into `arena`, as mail from `owner` to itself, counting on from
    /// `sent`, the mail `owner` has sent, into `mailed`. An event that holds resources is kept only once `claim` has
    /// claimed every one of them for the mail of the count it is written as (ADR-0019).
    fn bring(&self, owner: Seated, sent: u64, arena: &mut Arena, mailed: &mut Vec<Mailed>, claim: Claim<'_>)
    -> Brought;

    /// What the kind of its events is called, where the engine records one it could not write.
    fn name(&self) -> &'static str;

    /// Closes it, dropping what was pushed and not brought: its source has been dropped.
    fn close(&self);
}

impl<K: Kind + Encode<Actors> + Send + 'static> Brings for Fed<K> {
    fn bring(
        &self,
        owner: Seated,
        mut sent: u64,
        arena: &mut Arena,
        mailed: &mut Vec<Mailed>,
        claim: Claim<'_>,
    ) -> Brought {
        let mut unwritten = 0;
        let mut tickets = Vec::new();
        // What is pushed while it brings waits for the next bring, so a bring is bounded however fast the chassis
        // pushes.
        let ready = self.events.len();
        for event in (0..ready).map_while(|_| self.events.pop()) {
            tickets.clear();
            let Ok(bytes) = arena.write_carrying(&event, &mut tickets) else {
                unwritten += 1;
                continue;
            };
            // Most events hold no resource.
            let is_claimed = tickets.is_empty() || claim(sent + 1, &tickets);
            if !is_claimed {
                unwritten += 1;
                continue;
            }
            sent += 1;
            mailed.push(Mailed { by: owner, to: owner, count: sent, kind: KindId::of::<K>(), bytes, tie: Tie::TELL });
        }

        Brought { sent, unwritten }
    }

    fn name(&self) -> &'static str {
        any::type_name::<K>()
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        while self.events.pop().is_some() {}
    }
}

/// What an event's resources are claimed through as it is brought: the count of the mail it is written as, and the
/// tickets it holds, answered with whether every one was claimed for it.
type Claim<'a> = &'a mut dyn FnMut(u64, &[u64]) -> bool;

/// A resource an event brought carries: its ticket, and the mail, of `owner` and `count`, it arrives with.
struct Claimed {
    ticket: u64,
    owner: Seated,
    count: u64,
}

/// What one bring from a feed came to.
struct Brought {
    /// The mail its owner has sent, counting what was brought.
    sent: u64,
    /// How many of the events could not be written as mail.
    unwritten: usize,
}

/// A source the chassis made, as the engine keeps it to bring its feed in.
pub(super) struct Bringing {
    /// The ticket of the source, which it keeps whoever owns it.
    ticket: u64,
    /// Who held it at the last bring.
    seen: Seen,
    feed: Arc<dyn Brings>,
}

/// Where a source was last found.
#[derive(Clone, Copy)]
enum Seen {
    /// In the engine's keeping, handed in for a root not made yet, or nowhere looked yet.
    Handed,
    /// In the keeping of this actor.
    With(Seated),
}

/// Where a source is now.
enum Found {
    /// With the root's config it was handed for, which is not made yet: what is pushed waits in its feed.
    Handed,
    /// With this actor, its own or arriving for it.
    With(Seated),
    /// Nowhere: it was dropped.
    Gone,
}

impl Engine {
    /// Makes a controller of requests `K`, for the chassis, and answers with what names it, to put in the config of
    /// the capability it is for, and the end the host takes its requests from (ADR-0019). The engine holds the
    /// controller until [`Self::add_root`] is given that config, as [`Self::hand`] does.
    pub fn controller<K: Send + 'static>(&mut self) -> (Given<Controller<K>>, Requests<K>) {
        let ticket = self.ticket();
        let (controller, requests) = Controller::new();
        let (gift, given) = Gift::handed(ticket, controller);
        self.handed.push(gift);

        (given, requests)
    }

    /// Makes a source of `K`, for the chassis, and answers with what names it, to put in the config of the root it is
    /// for, and the feed the chassis pushes what it brings onto (ADR-0010). The engine holds the source until
    /// [`Self::add_root`] is given that config, as [`Self::hand`] does.
    pub fn source<K: Kind + Encode<Actors> + Send + 'static>(&mut self) -> (Given<Source<K>>, Feed<K>) {
        let ticket = self.ticket();
        let (gift, given) = Gift::handed(ticket, Source::<K>::default());
        self.handed.push(gift);
        let fed = Arc::new(Fed { events: SegQueue::new(), closed: AtomicBool::new(false) });
        self.sources.push(Bringing { ticket, seen: Seen::Handed, feed: Arc::clone(&fed) as Arc<dyn Brings> });

        (given, Feed { fed })
    }

    /// Brings what the chassis has pushed onto every feed since the last bring into the engine, between steps: each
    /// event as mail from the actor that owns its source to itself, which it handles in the next step (ADR-0010). Each
    /// is the root of a chain of its own (ADR-0012), and what one source brings keeps the order it was pushed in.
    ///
    /// What is pushed for a source still on its way to the root it was handed for waits in its feed. A feed whose
    /// source has been dropped is closed, and what was pushed onto it is dropped.
    ///
    /// An event may hold a resource the chassis handed in with [`Self::hand`], which no config or other event holds:
    /// it goes with the event to the source's owner, which takes it in the turn that handles it, or it is dropped and
    /// recorded in the owner's log, as a resource in any mail is (ADR-0019). An event that holds any other resource,
    /// or that cannot be written as mail, is dropped and recorded in the owner's log.
    pub fn bring(&mut self) {
        self.bring_jobs();
        // The jobs that finished since the last bring left room for those waiting.
        self.start_waiting();
        // Most engines have no source.
        if self.sources.is_empty() {
            return;
        }
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        let mut unbrought = Vec::new();
        let mut claimed = Vec::new();
        let mut sources = mem::take(&mut self.sources);
        sources.retain_mut(|source| match self.find(source.ticket, source.seen) {
            Found::With(owner) => {
                source.seen = Seen::With(owner);
                // An actor whose closing turn has run keeps nothing, so its sources are found nowhere at the next bring.
                if let Some(life) = self.lives.shard(owner.seat.number()).life(owner) {
                    let before = life.sent();
                    let handed = &self.handed;
                    let mut claim = |count, tickets: &[u64]| claim(handed, &mut claimed, owner, count, tickets);
                    let Brought { sent, unwritten } =
                        source.feed.bring(owner, before, &mut arena, &mut mailed, &mut claim);
                    unbrought.extend((0..unwritten).map(|_| log::unbrought(owner, source.feed.name())));
                    // A second source of the same owner counts on from the first.
                    let has_sent = sent > before;
                    if has_sent {
                        self.lives.fold_records([lives::sent(owner, sent)], &[]);
                    }
                }

                true
            }
            Found::Handed => true,
            Found::Gone => {
                source.feed.close();
                false
            }
        });
        self.sources = sources;
        self.arenas[parity][ROOTS] = arena;
        // Most brings carry no resource.
        if !claimed.is_empty() {
            self.carry_claimed(claimed);
        }
        // Each piece is the first of its chain: nothing in the engine caused it (ADR-0012).
        self.fold_brought(mailed, unbrought, parity, |mailed| MailId { sender: mailed.by.id, count: mailed.count });
    }

    /// Folds the mail brought, `mailed`, each the first of the chain `chain` names for it, and what could not be
    /// brought, `unbrought`, into the views, and marks the parts it went to waiting.
    pub(super) fn fold_brought(
        &mut self,
        mailed: Vec<Mailed>,
        unbrought: Vec<(u64, (ActorId, log::Logged))>,
        parity: usize,
        chain: impl Fn(&Mailed) -> MailId,
    ) {
        let written = &self.arenas[parity];
        self.log.fold_records(unbrought, written);
        let mut parts: Vec<usize> = mailed.iter().map(|mailed| self.part(mailed.to.seat)).collect();
        let entries = mailed.into_iter().map(|mailed| {
            let context = Context { root: chain(&mailed), depth: 0 };

            stream::entry(mailed, &context)
        });
        self.streams.fold_records(entries, written);
        parts.sort_unstable();
        parts.dedup();
        for part in parts {
            self.expected[part] = self.expect(part);
            self.waiting.push(part);
        }
    }

    /// Takes each resource the events brought carry out of the engine's keeping, and puts it on its way to the owner of
    /// the source that brought it, with the mail it holds (ADR-0019).
    #[cold]
    fn carry_claimed(&mut self, claimed: Vec<Claimed>) {
        let mut lost = Vec::new();
        for Claimed { ticket, owner, count } in claimed {
            let Some(at) = self.handed.iter().position(|gift| gift.ticket() == ticket) else {
                continue;
            };
            let Gift { kept, .. } = self.handed.swap_remove(at);
            let mail = MailId { sender: owner.id, count };
            let part = self.part(owner.seat);
            let mut bucket = vec![Arrival { to: owner, by: owner, mail, kept }];
            gift::arrive_all(&mut self.actors[part], &mut bucket, &mut lost);
        }
        self.log.fold_records(lost.into_iter().map(|(by, name)| log::dropped(by, name)), &[]);
    }

    /// Where the source `ticket` names is now, looked for first where it was last `seen`.
    fn find(&self, ticket: u64, seen: Seen) -> Found {
        if let Seen::With(owner) = seen {
            let is_still_held =
                self.actors[self.part(owner.seat)].get(owner).is_some_and(|held| held.keeping.holds(ticket));
            if is_still_held {
                return Found::With(owner);
            }
        }
        let is_handed = self.handed.iter().any(|gift| gift.ticket() == ticket);
        if is_handed {
            return Found::Handed;
        }
        // It has moved since: given to another actor, which sits wherever it sits.
        let parts = self.actors.iter().enumerate();
        let owner = parts.flat_map(|(part, actors)| actors.seated(part)).find(|(_, held)| held.keeping.holds(ticket));

        owner.map_or(Found::Gone, |(owner, _)| Found::With(owner))
    }
}

/// Claims, for the mail of `owner` and `count`, each of `tickets`: answered with whether every one is a resource the
/// chassis handed in, among `handed`, that no event brought before it claimed, noted in `claimed`. None is claimed
/// unless every one can be.
fn claim(handed: &[Gift], claimed: &mut Vec<Claimed>, owner: Seated, count: u64, tickets: &[u64]) -> bool {
    let is_free = |ticket: &u64| {
        let is_handed = handed.iter().any(|gift| gift.ticket() == *ticket);
        let is_taken = claimed.iter().any(|claim| claim.ticket == *ticket);

        is_handed && !is_taken
    };
    let can_claim = tickets.iter().all(is_free);
    if can_claim {
        claimed.extend(tickets.iter().map(|&ticket| Claimed { ticket, owner, count }));
    }

    can_claim
}
