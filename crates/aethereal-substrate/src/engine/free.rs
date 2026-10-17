//! Spike: parts that step on their own, each running whenever mail has been handed to it, with no step of the whole
//! engine between (not a design yet: what an ADR would weigh).
//!
//! Each worker owns some parts for the run. A part steps when its inbox holds anything: it folds what was handed to
//! it, runs its turns as a step would, and hands each record its turns made straight to the inbox of the part it is
//! for, the bytes of mail copied with it. A turn that changes the table, or stands up or watches an actor, stops the
//! run: what it asked for is folded by the engine once every worker has stopped, as a step's records are.

use super::birth::{self, Birth};
use super::{Engine, Lists, Parcel, ROOTS, Reading, parity};
use crate::lives::{self, Lives};
use crate::log::{self, Log};
use crate::stream::{self, Streams};
use crate::{ByActor, BySeat, Clock, View};
use aethereal_actor::{Arena, Asked, Declared, Payload, SeatNumber, Seats, Span};
use aethereal_data::ActorId;
use std::collections::VecDeque;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// The first writer slot that names a part's inbound bytes and not an arena: slot `FREE + part` is part `part`'s.
const FREE: u32 = u32::MAX - (1 << 20);

/// Where a turn finds the bytes of mail handed straight to a part.
#[derive(Clone, Copy)]
pub(super) enum Inbound<'a> {
    /// Every part's, as the engine keeps them between runs.
    All(&'a [Vec<u8>]),
    /// One part's, while a worker steps it.
    One(usize, &'a [u8]),
}

impl Inbound<'_> {
    /// The bytes `payload` names, when it names a part's inbound bytes.
    pub(super) fn read(&self, payload: Payload) -> Option<&[u8]> {
        let Payload::InArena(Span { slot, start, len }) = payload else {
            return None;
        };
        if slot < FREE {
            return None;
        }
        let part = (slot - FREE) as usize;
        let bytes = match self {
            Self::All(all) => all.get(part)?.as_slice(),
            Self::One(own, bytes) if *own == part => bytes,
            Self::One(..) => return None,
        };
        let start = start as usize;

        bytes.get(start..start + len as usize)
    }
}

/// The children every part stood up while parts step on their own, which the table does not hold until the run
/// ends: what a turn finds of a child in another part that it was told of.
pub(super) struct Fresh {
    /// In shards by the bits of the child's id, so workers looking up different children seldom wait on each other.
    shards: Vec<Mutex<ByActor<(SeatNumber, &'static Declared)>>>,
}

/// How many shards [`Fresh`] keeps.
const FRESH_SHARDS: usize = 64;

impl Default for Fresh {
    fn default() -> Self {
        Self { shards: (0..FRESH_SHARDS).map(|_| Mutex::default()).collect() }
    }
}

impl Fresh {
    /// The seat of the child `id`, and what its type declares, when a part stood it up in the run.
    pub(super) fn find(&self, id: ActorId) -> Option<(SeatNumber, &'static Declared)> {
        lock(self.shard(id.bits())).get(&id.bits()).copied()
    }

    fn shard(&self, bits: u64) -> &Mutex<ByActor<(SeatNumber, &'static Declared)>> {
        &self.shards[usize::try_from(bits >> 58).unwrap_or(0) % FRESH_SHARDS]
    }
}

/// What one part's turns handed to another: the children to seat in it, its records of the streams, the lives, and
/// the logs, and the bytes of the mail among them.
#[derive(Default)]
struct Batch {
    births: Vec<Birth>,
    streams: Vec<(u64, stream::Entry)>,
    lives: Vec<(u64, lives::Record)>,
    log: Vec<(u64, <Log as View>::Record)>,
    bytes: Vec<u8>,
}

/// A run of parts stepping on their own, as every worker reads it.
pub struct FreeRun<'a> {
    reading: Reading<'a>,
    fresh: Fresh,
    inboxes: Vec<Mutex<Vec<Batch>>>,
    /// How many batches each inbox holds, read without its lock: a worker locks an inbox only when it holds one.
    held: Vec<AtomicUsize>,
    /// Batches handed to an inbox and not yet folded.
    in_flight: AtomicUsize,
    /// Workers with nothing to step.
    idle: AtomicUsize,
    /// Whether a turn asked for what only a step of the whole engine applies.
    stopped: AtomicBool,
    /// Workers that have stopped stepping, before any folds what is left in its inboxes.
    drained: AtomicUsize,
    workers: usize,
}

/// One worker's parts for a run, and what it writes to.
pub struct FreeShare {
    parts: Vec<FreePart>,
    /// Emptied batches, whose room is left in the writer's buckets as it hands records on.
    spare: Vec<Batch>,
    lists: Lists,
    arena: Arena,
    /// How many times its parts stepped.
    steps: u64,
}

struct FreePart {
    parcel: Parcel,
    log: BySeat<VecDeque<log::Entry>>,
    inbound: Vec<u8>,
    /// How many children were seated in it.
    born: usize,
}

impl Engine {
    /// Takes every part out for `workers` workers to step on their own, part `p` to worker `p % workers`, when the
    /// engine can: with more than one worker and no fused hops.
    pub fn free_shares(&mut self, workers: usize) -> Option<Vec<FreeShare>> {
        let parts = self.seating.len();
        let can = workers > 1 && self.hops == 0 && parts >= workers;
        if !can {
            return None;
        }
        self.step += 1;
        self.waiting.clear();
        for part in 0..parts {
            self.give_room(part);
        }
        self.writing = ROOTS + 1..ROOTS + 1 + workers;
        let parity = parity(self.step);
        let share = self.mail_bytes / (2 * (ROOTS + 1 + workers));
        for arena in &mut self.arenas[parity] {
            arena.clear(share);
        }
        let mut shares: Vec<FreeShare> = (0..workers)
            .map(|place| {
                let (arena, lists) = self.writer(parity, place, share);
                FreeShare { parts: Vec::new(), spare: Vec::new(), lists, arena, steps: 0 }
            })
            .collect();
        for part in 0..parts {
            let parcel = self.parcel(part);
            let log = self.log.take(part);
            let inbound = mem::take(&mut self.inbound[part]);
            shares[part % workers].parts.push(FreePart { parcel, log, inbound, born: 0 });
        }

        Some(shares)
    }

    /// The run the workers read while they step their parts.
    #[must_use]
    pub fn free_run(&self, workers: usize) -> FreeRun<'_> {
        FreeRun {
            reading: self.reading(),
            fresh: Fresh::default(),
            inboxes: (0..self.seating.len()).map(|_| Mutex::new(Vec::new())).collect(),
            held: (0..self.seating.len()).map(|_| AtomicUsize::new(0)).collect(),
            in_flight: AtomicUsize::new(0),
            idle: AtomicUsize::new(0),
            stopped: AtomicBool::new(false),
            drained: AtomicUsize::new(0),
            workers,
        }
    }

    /// Puts back what the workers' runs left, and folds where they lie what their turns asked for that only a step of
    /// the whole engine applies: answers with how many times parts stepped.
    pub fn end_free(&mut self, shares: Vec<FreeShare>) -> u64 {
        let mut steps = 0;
        for FreeShare { parts, lists, arena, steps: stepped, .. } in shares {
            steps += stepped;
            for FreePart { parcel, log, inbound, born } in parts {
                let part = parcel.part;
                self.seated[part] += born;
                self.actors_seated += born;
                self.put(parcel);
                self.log.put(part, log);
                self.inbound[part] = inbound;
            }
            self.take_in(lists, arena);
        }
        let mut forward = mem::take(&mut self.spare.forward);
        for part in 0..self.seating.len() {
            let rows = &mut self.lists[self.writing.clone()];
            for row in rows.iter_mut() {
                let bucket = row.routes.births.bucket_mut(part);
                let born = birth::seat_all(&mut self.seating[part], &mut self.actors[part], bucket);
                self.seated[part] += born;
                self.actors_seated += born;
            }
            self.table.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.table));
            self.log.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.log));
            self.lives.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.lives));
            self.streams.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.streams));
            self.watches.fold_part(part, rows.iter_mut().map(|lists| &mut lists.routes.watches));
            self.watches.at_mut(part).settle(&mut forward);
            self.expected[part] = self.expect(part);
            self.waiting.push(part);
        }
        self.hand_on(forward);

        steps
    }
}

impl FreeRun<'_> {
    /// Steps `share`'s parts until no part anywhere has anything to step, or a turn stops the run, then folds what
    /// is left in its parts' inboxes, once every worker has stopped stepping.
    pub fn run(&self, share: FreeShare, clock: &impl Clock) -> FreeShare {
        let FreeShare { mut parts, mut spare, mut lists, arena, mut steps } = share;
        let mut tally = mem::take(&mut lists.tally);
        let mut asked = Asked::continuing(mem::take(&mut lists.effects), arena, Seats::default());
        let mut idle = false;
        // Passes in a row that found nothing to step: an idle worker waits longer between looks, so the counts every
        // worker reads are not read so often that writing them, as mail is handed on, is slowed.
        let mut empty = 0_u32;
        loop {
            let mut progressed = false;
            for part in &mut parts {
                let batches = self.take(part.parcel.part);
                if !batches.is_empty() {
                    if idle {
                        idle = false;
                        self.idle.fetch_sub(1, Ordering::SeqCst);
                    }
                    let count = batches.len();
                    for batch in batches {
                        spare.push(fold_in(part, batch));
                    }
                    self.in_flight.fetch_sub(count, Ordering::SeqCst);
                }
                let has_work = part.parcel.streams.has_items() || part.parcel.lives.has_ended();
                if !has_work {
                    continue;
                }
                if idle {
                    idle = false;
                    self.idle.fetch_sub(1, Ordering::SeqCst);
                }
                let own = part.parcel.part;
                let reading =
                    Reading { inbound: Inbound::One(own, &part.inbound), fresh: Some(&self.fresh), ..self.reading };
                reading.run_part(part.parcel.room(), clock, &mut tally, &mut asked, &mut lists);
                part.inbound.clear();
                steps += 1;
                progressed = true;
                // The table's and the watches' records wait for the run to end, as a step's do, and the engine folds
                // them; the children are found in the run's own list until then.
                self.hand_on(&mut lists, &asked, &mut spare);
                let (effects, stood, mut arena, seats) = asked.into_parts();
                drop(stood);
                arena.clear(0);
                asked = Asked::continuing(effects, arena, seats);
            }
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            if progressed {
                empty = 0;
                continue;
            }
            empty = empty.saturating_add(1);
            for _ in 0..1_u32 << empty.min(8) {
                std::hint::spin_loop();
            }
            if !idle {
                idle = true;
                self.idle.fetch_add(1, Ordering::SeqCst);
            }
            let done = self.in_flight.load(Ordering::SeqCst) == 0 && self.idle.load(Ordering::SeqCst) == self.workers;
            if done {
                break;
            }
            std::hint::spin_loop();
        }

        self.drained.fetch_add(1, Ordering::SeqCst);
        while self.drained.load(Ordering::SeqCst) < self.workers {
            std::hint::spin_loop();
        }
        for part in &mut parts {
            let batches = self.take(part.parcel.part);
            for batch in batches {
                spare.push(fold_in(part, batch));
            }
        }
        let (effects, _, arena, _) = asked.into_parts();
        lists.effects = effects;
        lists.tally = tally;

        FreeShare { parts, spare, lists, arena, steps }
    }

    /// Hands each part's records in `lists`, made by the part just stepped, to that part's inbox, the bytes of its
    /// mail copied out of `asked`'s arena.
    fn hand_on(&self, lists: &mut Lists, asked: &Asked, spare: &mut Vec<Batch>) {
        // The children are put where every turn finds them before any mail that could name them is handed on.
        let has_stood = !lists.fusing.stood.is_empty();
        if has_stood {
            for (id, (_, seat, declared)) in &lists.fusing.stood {
                lock(self.fresh.shard(*id)).insert(*id, (*seat, *declared));
            }
        }
        for (part, inbox) in self.inboxes.iter().enumerate() {
            let routes = &lists.routes;
            let records = routes.births.records(part)
                + routes.streams.records(part)
                + routes.lives.records(part)
                + routes.log.records(part);
            if records == 0 {
                continue;
            }
            let room = spare.pop().unwrap_or_default();
            let routes = &mut lists.routes;
            let mut batch = Batch {
                births: routes.births.take_leaving(part, room.births),
                streams: routes.streams.take_leaving(part, room.streams),
                lives: routes.lives.take_leaving(part, room.lives),
                log: routes.log.take_leaving(part, room.log),
                bytes: room.bytes,
            };
            let slot = FREE + u32::try_from(part).unwrap_or(0);
            for (_, entry) in &mut batch.streams {
                let Some(payload) = entry.payload_mut() else {
                    continue;
                };
                let bytes = asked.arena().read(*payload).unwrap_or_default();
                let start = u32::try_from(batch.bytes.len()).unwrap_or(u32::MAX);
                let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
                batch.bytes.extend_from_slice(bytes);
                *payload = Payload::InArena(Span { slot, start, len });
            }
            self.in_flight.fetch_add(1, Ordering::SeqCst);
            let mut held = lock(inbox);
            held.push(batch);
            self.held[part].store(held.len(), Ordering::Release);
        }
    }

    /// The batches in part `part`'s inbox, taken out: none, with no lock taken, when it holds none.
    fn take(&self, part: usize) -> Vec<Batch> {
        let holds = self.held[part].load(Ordering::Acquire) > 0;
        if !holds {
            return Vec::new();
        }
        let mut held = lock(&self.inboxes[part]);
        self.held[part].store(0, Ordering::Release);

        mem::take(&mut *held)
    }
}

/// Folds `batch` into `part`, its mail's bytes put after the part's, and answers with it emptied, for its room.
fn fold_in(part: &mut FreePart, mut batch: Batch) -> Batch {
    let base = u32::try_from(part.inbound.len()).unwrap_or(u32::MAX);
    part.inbound.extend_from_slice(&batch.bytes);
    for (_, entry) in &mut batch.streams {
        if let Some(Payload::InArena(span)) = entry.payload_mut() {
            span.start += base;
        }
    }
    part.born += birth::seat_all(&mut part.parcel.seating, &mut part.parcel.actors, &mut batch.births);
    Streams::fold_all(&mut part.parcel.streams, &mut batch.streams);
    Lives::fold_all(&mut part.parcel.lives, &mut batch.lives);
    Log::fold_all(&mut part.log, &mut batch.log);
    batch.bytes.clear();

    batch
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

