//! What the chassis brings into the engine between steps, through the sources actors own (ADR-0010).
//!
//! The chassis pushes what happens outside the engine onto a source's feed,
//! whenever it happens, and the engine holds none of it until the chassis
//! brings it in between two steps. Each event then reaches whoever owns the
//! source as mail from that owner, the root of a chain of its own (ADR-0012),
//! and is handled in the next step.

#[cfg(test)]
mod tests;

use super::{Engine, ROOTS, parity};
use crate::{Context, lives, log, stream};
use aethereal_actor::{Actors, Arena, Gift, Given, MailId, Mailed, Seated, Source, Tie};
use aethereal_data::{ActorId, Encode, Kind, KindId};
use std::any;
use std::mem;
use std::sync::{Arc, Mutex, PoisonError};

/// The chassis's end of a source: what it pushes events of kind `K` onto, as they happen, for the engine to bring to
/// the source's owner at the next [`Engine::bring`] (ADR-0010).
///
/// It is cloned for each place the chassis hears of such events, and every
/// clone pushes onto the one feed. It is closed once its source is dropped.
pub struct Feed<K> {
    fed: Arc<Mutex<Fed<K>>>,
}

/// What a feed holds between two brings.
struct Fed<K> {
    /// The events pushed since the last bring, in the order pushed.
    events: Vec<K>,
    flow: Flow,
}

/// Whether a feed's source is still held.
enum Flow {
    /// It is, or it is still on its way to the root it was handed for.
    Open,
    /// It has been dropped: nothing pushed now reaches an actor.
    Closed,
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
        let mut fed = self.fed.lock().unwrap_or_else(PoisonError::into_inner);
        match fed.flow {
            Flow::Open => {
                fed.events.push(event);
                drop(fed);

                Ok(())
            }
            Flow::Closed => Err(Closed),
        }
    }

    /// Whether the source has been dropped, as of the last bring: the chassis then stops what it fed it from.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let fed = self.fed.lock().unwrap_or_else(PoisonError::into_inner);

        matches!(fed.flow, Flow::Closed)
    }
}

impl<K> Clone for Feed<K> {
    fn clone(&self) -> Self {
        Self { fed: Arc::clone(&self.fed) }
    }
}

/// A feed as the engine brings from it, whatever its kind.
trait Brings: Send + Sync {
    /// Writes each event pushed since the last bring into `arena`, as mail from `owner` to itself, counting on from
    /// `sent`, the mail `owner` has sent, into `mailed`.
    fn bring(&self, owner: Seated, sent: u64, arena: &mut Arena, mailed: &mut Vec<Mailed>) -> Brought;

    /// What the kind of its events is called, where the engine records one it could not write.
    fn name(&self) -> &'static str;

    /// Closes it, dropping what was pushed and not brought: its source has been dropped.
    fn close(&self);
}

impl<K: Kind + Encode<Actors> + Send + 'static> Brings for Mutex<Fed<K>> {
    fn bring(&self, owner: Seated, mut sent: u64, arena: &mut Arena, mailed: &mut Vec<Mailed>) -> Brought {
        let mut fed = self.lock().unwrap_or_else(PoisonError::into_inner);
        let mut unwritten = 0;
        for event in fed.events.drain(..) {
            let Ok(bytes) = arena.write_mail(&event) else {
                unwritten += 1;
                continue;
            };
            sent += 1;
            mailed.push(Mailed { by: owner, to: owner, count: sent, kind: KindId::of::<K>(), bytes, tie: Tie::TELL });
        }
        drop(fed);

        Brought { sent, unwritten }
    }

    fn name(&self) -> &'static str {
        any::type_name::<K>()
    }

    fn close(&self) {
        let mut fed = self.lock().unwrap_or_else(PoisonError::into_inner);
        fed.flow = Flow::Closed;
        fed.events = Vec::new();
    }
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
    /// Makes a source of `K`, for the chassis, and answers with what names it, to put in the config of the root it is
    /// for, and the feed the chassis pushes what it brings onto (ADR-0010). The engine holds the source until
    /// [`Self::add_root`] is given that config, as [`Self::hand`] does.
    pub fn source<K: Kind + Encode<Actors> + Send + 'static>(&mut self) -> (Given<Source<K>>, Feed<K>) {
        let ticket = self.ticket();
        let (gift, given) = Gift::handed(ticket, Source::<K>::default());
        self.handed.push(gift);
        let fed = Arc::new(Mutex::new(Fed { events: Vec::new(), flow: Flow::Open }));
        self.sources.push(Bringing { ticket, seen: Seen::Handed, feed: Arc::clone(&fed) as Arc<dyn Brings> });

        (given, Feed { fed })
    }

    /// Brings what the chassis has pushed onto every feed since the last bring into the engine, between steps: each
    /// event as mail from the actor that owns its source to itself, which it handles in the next step (ADR-0010). Each
    /// is the root of a chain of its own (ADR-0012), and what one source brings keeps the order it was pushed in.
    ///
    /// What is pushed for a source still on its way to the root it was handed for waits in its feed. A feed whose
    /// source has been dropped is closed, and what was pushed onto it is dropped. An event that cannot be written as
    /// mail, one that holds a resource on its way, is dropped and recorded in the owner's log.
    pub fn bring(&mut self) {
        // Most engines have no source.
        if self.sources.is_empty() {
            return;
        }
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        let mut unbrought = Vec::new();
        let mut sources = mem::take(&mut self.sources);
        sources.retain_mut(|source| match self.find(source.ticket, source.seen) {
            Found::With(owner) => {
                source.seen = Seen::With(owner);
                // An actor whose closing turn has run keeps nothing, so its sources are found nowhere at the next bring.
                if let Some(life) = self.lives.shard(owner.seat.number()).life(owner) {
                    let before = life.sent();
                    let Brought { sent, unwritten } = source.feed.bring(owner, before, &mut arena, &mut mailed);
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
        self.fold_brought(mailed, unbrought, parity);
    }

    /// Folds the mail brought, `mailed`, and what could not be brought, `unbrought`, into the views, and marks the
    /// parts it went to waiting.
    fn fold_brought(&mut self, mailed: Vec<Mailed>, unbrought: Vec<(u64, (ActorId, log::Logged))>, parity: usize) {
        let written = &self.arenas[parity];
        self.log.fold_records(unbrought, written);
        let mut parts: Vec<usize> = mailed.iter().map(|mailed| self.part(mailed.to.seat)).collect();
        // Each piece is the first of its chain: nothing in the engine caused it (ADR-0012).
        let entries = mailed.into_iter().map(|mailed| {
            let context = Context { root: MailId { sender: mailed.by.id, count: mailed.count }, depth: 0 };

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
