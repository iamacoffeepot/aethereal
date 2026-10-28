//! Jobs: the seam through which the engine has its host run the work turns hand it outside the steps, and the feed
//! their outcomes come back on (ADR-0018).

#[cfg(test)]
mod tests;

use super::gift::{self, Arrival};
use super::{Engine, ROOTS, parity};
use crate::{lives, log};
use aethereal_actor::{
    BlobId, Copied, Dialer, Fault, JobCode, JobId, JobType, Kept, Lending, MailId, Mailed, Never, NoDialer, Outcome,
    Pinning, Produced, Seated, Storage, Submitted, Tie, Timer, Unanswered,
};
use aethereal_data::{Kind, KindId, Plain};
use crossbeam_queue::SegQueue;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// What runs the jobs the engine hands out, which each host implements: a pool of threads natively, Web Workers in a
/// browser, or inline between steps where there is one thread (ADR-0018).
pub trait Jobs: Send + Sync {
    /// Runs the job `id` with the code of its type `of`, as `job` holds it: its bytes, and where its outcome goes once
    /// it has run. A native turn's job carries its own type, and a guest's is found among the types the program lets
    /// its guests run, so a runner runs every job it is handed and needs no list of its own (ADR-0015).
    fn start(&self, id: JobId, of: JobType, job: Taken);

    /// The most jobs it lets be in flight at once: handed out, and their [`Finish`] not yet done or dropped. The engine
    /// keeps the rest waiting, in the order they were run, and hands each out as one in flight finishes.
    fn most(&self) -> usize;

    /// Stops the job `id`, if it can, once the actor that ran it has cancelled it: best effort, as a job may have
    /// finished, or be past where it can be stopped. Whatever it pushes of the job after is dropped. A job it has
    /// finished, or never started, it does nothing for.
    fn stop(&self, id: JobId);
}

/// A job as the engine hands it to its host: its bytes, the blobs it refers to, the most of its bytes it may read, and
/// where its outcome goes.
pub struct Taken {
    /// The job's bytes, written at `Actors`.
    pub bytes: Vec<u8>,
    /// A copy of each blob it refers to, and where the changes to their pins go as it runs.
    pub lent: Lending,
    /// The most bytes it may read, as mail may.
    pub limit: usize,
    /// Where its outcome goes.
    pub finish: Finish,
}

/// Where a job's outcome goes: the engine's feed of finished jobs, which it brings in at [`Engine::bring`]. The feed is
/// a queue no one waits on: a runner pushing onto it never waits for another, nor for the engine, and the engine never
/// waits for a runner.
///
/// A host that drops it without [`Finish::done`] leaves its job unanswered.
pub struct Finish {
    by: Seated,
    count: u64,
    answer: KindId,
    feed: Arc<SegQueue<Finished>>,
    /// One of the engine's tokens of a job in flight, so how many there are is how many jobs are in flight: one is let
    /// go of once its job is done, or its `Finish` dropped.
    _in_flight: Arc<()>,
}

impl Finish {
    /// Pushes the job's outcome onto the engine's feed, to answer the actor that ran it at the next bring.
    pub fn done(self, outcome: Outcome) {
        let Self { by, count, answer, feed, .. } = self;
        feed.push(Finished { by, count, answer, outcome });
    }
}

/// A job that has run, as the engine's feed holds it until the next bring.
pub(super) struct Finished {
    /// The actor that ran it, and the count it took.
    by: Seated,
    count: u64,
    /// The kind of its result.
    answer: KindId,
    outcome: Outcome,
}

/// What a job made, as the engine answers its actor with it: the result's bytes, the pins on the blobs it refers to,
/// and each resource it carries, under the engine's ticket (ADR-0018).
struct Answering {
    bytes: Vec<u8>,
    held: Vec<Pinning>,
    kept: Vec<Kept>,
}

/// Runs each job inline, as it is handed out between steps: for a host with one thread, and for tests. A job whose
/// future is not ready the first time it is polled, one that waits, is answered with [`Waits`], as nothing would
/// poll it again.
///
/// It has no thread to wait on, so its timer is [`Never`]: a [`Sleep`] for any time past the first answers with
/// [`Waits`]. Its jobs read files from the storage it is made with (ADR-0032). A dial waits, so its dialer is
/// [`NoDialer`], and every dial is unavailable (ADR-0033).
///
/// [`Sleep`]: aethereal_actor::Sleep
pub struct Inline {
    timer: Arc<dyn Timer>,
    storage: Arc<dyn Storage>,
    dialer: Arc<dyn Dialer>,
}

impl Inline {
    /// Runs jobs reading files from `storage`.
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { timer: Arc::new(Never), storage, dialer: Arc::new(NoDialer) }
    }
}

impl Jobs for Inline {
    fn start(&self, _: JobId, of: JobType, job: Taken) {
        let Taken { bytes, lent, limit, finish } = job;
        let (timer, storage, dialer) = (Arc::clone(&self.timer), Arc::clone(&self.storage), Arc::clone(&self.dialer));
        let mut future = of.drive(bytes, lent, limit, timer, storage, dialer);
        let mut context = Context::from_waker(Waker::noop());
        let polled = panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut context)));
        let outcome = match polled {
            Ok(Poll::Ready(outcome)) => outcome,
            Ok(Poll::Pending) => Err(fault::<Waits>()),
            Err(_) => Err(fault::<Panicked>()),
        };
        finish.done(outcome);
    }

    /// Every job has run by the time it is handed out, so none is left to stop.
    fn stop(&self, _: JobId) {}

    /// Every job has run by the time it is handed out, so none is ever in flight.
    fn most(&self) -> usize {
        usize::MAX
    }
}

/// The fault whose reason is `K`, which holds nothing.
fn fault<K: Kind>() -> Fault {
    Fault::of_parts(KindId::of::<K>(), Vec::new())
}

/// The reason of a guest's job no runner runs: the program does not let its guests run its type (ADR-0015).
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.job.no_runner")]
pub struct NoRunner;

/// The reason of a job that waited where its runner polls each job once.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.job.waits")]
pub struct Waits;

/// The reason of a job one of whose blobs did not fit the budget for blobs when it was answered.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.job.no_room")]
pub struct NoRoom;

/// The reason of a job that panicked.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.job.panicked")]
pub struct Panicked;

/// What runs an engine's jobs (ADR-0018, ADR-0029).
pub(super) enum Runner {
    /// None yet: the host makes it once its program's roots have bootstrapped, and the jobs their `begin`s run wait
    /// until then.
    Unmade,
    /// What the host made.
    Made(Arc<dyn Jobs>),
}

impl Engine {
    /// The engine, its jobs run by `jobs` (ADR-0018).
    pub(super) fn with_jobs(mut self, jobs: Arc<dyn Jobs>) -> Self {
        self.runner = Runner::Made(jobs);

        self
    }

    /// The engine, letting its guests run the jobs of `types` as well (ADR-0015).
    pub(super) fn with_guest_jobs(mut self, types: Vec<JobType>) -> Self {
        self.guest_jobs.extend(types);

        self
    }

    /// Runs this engine's jobs on `jobs` from now on, and hands it the jobs waiting, as many as it lets be in flight
    /// (ADR-0029): a host makes its runners once its program's roots have bootstrapped, so the jobs their `begin`s
    /// ran wait for it. Jobs another runner was handed finish there, and are answered as ever.
    pub fn set_jobs(&mut self, jobs: Arc<dyn Jobs>) {
        self.runner = Runner::Made(jobs);
        self.start_waiting();
    }

    /// Queues each of `submitted`, once the turns that ran them have, behind those already waiting, and hands out as
    /// many as the host lets be in flight (ADR-0018).
    pub(super) fn start_jobs(&mut self, submitted: impl Iterator<Item = Submitted>) {
        self.waiting_jobs.extend(submitted);
        self.start_waiting();
    }

    /// Hands the jobs waiting to the host in the order they were run, while it lets more be in flight. The job of an
    /// actor that has ended is dropped: it would be answered to no one, and its blobs are no longer pinned.
    pub(super) fn start_waiting(&mut self) {
        loop {
            let has_room = self.jobs_left() > 0;
            if !has_room {
                return;
            }
            let Some(job) = self.waiting_jobs.pop_front() else {
                return;
            };
            let is_live = self.lives.shard(job.by.seat.number()).life(job.by).is_some();
            if is_live {
                self.hand_out(job);
            }
        }
    }

    /// Hands `job` to the host to run, with the code of its type and a copy of each blob it refers to, once for each
    /// blob. Each is held: the context the job's actor keeps pins it, and the blobs a step put are taken in before its
    /// jobs are handed out. A guest's job whose type the program does not let its guests run is answered with
    /// [`NoRunner`], and never handed out (ADR-0015).
    fn hand_out(&self, job: Submitted) {
        let id = job.id();
        let Submitted { by, count, code, answer, bytes, refers } = job;
        let feed = Arc::clone(&self.finished);
        let finish = Finish { by, count, answer, feed, _in_flight: Arc::clone(&self.in_flight) };
        let Some(of) = self.code_of(code) else {
            finish.done(Err(fault::<NoRunner>()));
            return;
        };
        let lent = Lending::new(self.number.0, self.copied(refers), Arc::clone(&self.job_pins));
        if let Runner::Made(jobs) = &self.runner {
            jobs.start(id, of, Taken { bytes, lent, limit: self.read_limit, finish });
        }
    }

    /// The type a job is run with: a native turn's own, or the one of a guest's job's kind the program lets its guests
    /// run, when it lets them run one.
    fn code_of(&self, code: JobCode) -> Option<JobType> {
        match code {
            JobCode::Native(of) => Some(of),
            JobCode::Guest(kind) => self.guest_jobs.iter().find(|of| of.kind() == kind).copied(),
        }
    }

    /// Stops the job `job` an actor cancelled: one still waiting is dropped, never handed out, and the host is asked
    /// to stop one it was handed.
    pub(super) fn stop_job(&mut self, job: JobId) {
        let waiting = self.waiting_jobs.iter().position(|waiting| waiting.id() == job);
        match (waiting, &self.runner) {
            (Some(at), _) => drop(self.waiting_jobs.remove(at)),
            (None, Runner::Made(jobs)) => jobs.stop(job),
            (None, Runner::Unmade) => {}
        }
    }

    /// How many more jobs the host lets be in flight: as many as it lets be, less those its runners hold the finish of,
    /// and none before it has made its runners.
    fn jobs_left(&self) -> usize {
        let Runner::Made(jobs) = &self.runner else {
            return 0;
        };
        let in_flight = Arc::strong_count(&self.in_flight) - 1;

        jobs.most().saturating_sub(in_flight)
    }

    /// A copy of each blob among `refers`, once each.
    fn copied(&self, mut refers: Vec<BlobId>) -> Vec<Copied> {
        // Most jobs refer to no blob.
        if refers.is_empty() {
            return Vec::new();
        }
        refers.sort_unstable();
        refers.dedup();

        refers
            .into_iter()
            .filter_map(|id| self.blobs.read(id).map(|bytes| Copied { id, bytes: bytes.to_vec() }))
            .collect()
    }

    /// Brings in each job finished before this bring began, between steps: its outcome as mail from the actor that ran it
    /// to itself, tied to the job, the root of a chain of its own, which its handler of what the job answers with is
    /// handed with the context it kept (ADR-0018). A fault is written into the actor's log too. The outcome of a job
    /// whose actor has ended is dropped. A resource the job handed goes with its answer, which its actor takes in the
    /// handler of the answer, or it is dropped and recorded in its log, as a resource in any mail is (ADR-0010); the
    /// resources of a job that faulted, or whose actor has ended, are dropped. What finishes while it brings waits for
    /// the next, so a bring is bounded however fast the runners push.
    pub(super) fn bring_jobs(&mut self) {
        let ready = self.finished.len();
        // Most brings find no job finished.
        if ready == 0 {
            return;
        }
        let feed = Arc::clone(&self.finished);
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        let mut faults = Vec::new();
        let mut arrivals = Vec::new();
        for Finished { by, count, answer, outcome } in (0..ready).map_while(|_| feed.pop()) {
            let Some(life) = self.lives.shard(by.seat.number()).life(by) else {
                continue;
            };
            let sent = life.sent() + 1;
            let written = match outcome.and_then(|produced| self.put_produced(produced)) {
                Ok(Answering { bytes, held, kept }) => {
                    let written = arena.write_holding_bytes(&bytes, held);
                    written.map(|bytes| (answer, bytes, Tie::answers(count), kept))
                }
                Err(fault) => {
                    faults.push(log::job_faulted(by, fault));
                    let bytes = arena.write_mail(&Unanswered::Faulted);
                    bytes.map(|bytes| (KindId::of::<Unanswered>(), bytes, Tie::unanswered(count), Vec::new()))
                }
            };
            let Ok((kind, bytes, tie, kept)) = written else {
                continue;
            };
            self.lives.fold_records([lives::sent(by, sent)], &[]);
            let mail = MailId { sender: by.id, count: sent };
            arrivals.extend(kept.into_iter().map(|kept| Arrival { to: by, by, mail, kept }));
            mailed.push(Mailed { by, to: by, count: sent, kind, bytes, tie });
        }
        self.arenas[parity][ROOTS] = arena;
        // Most jobs hand no resource.
        if !arrivals.is_empty() {
            self.carry_handed(arrivals);
        }
        // Each answer is the first of its chain: nothing the actor did in the step it is brought in caused it.
        self.fold_brought(mailed, faults, parity, |mailed| MailId { sender: mailed.by.id, count: mailed.count });
    }

    /// Puts each resource a job handed on its way to the actor that ran it, with the answer that holds it (ADR-0018).
    #[cold]
    fn carry_handed(&mut self, arrivals: Vec<Arrival>) {
        let mut lost = Vec::new();
        for arrival in arrivals {
            let part = self.part(arrival.to.seat);
            gift::arrive_all(&mut self.actors[part], &mut vec![arrival], &mut lost);
        }
        self.log.fold_records(lost.into_iter().map(|(by, name)| log::dropped(by, name)), &[]);
    }

    /// Puts each blob a job put, between steps, out of the budget for blobs, and writes over each reference to one
    /// in its result the blob's own; then keeps each resource it handed, under a ticket of the engine's, written over
    /// the job's own where it lies in the result: the result's bytes, the pins on the blobs it refers to, and the
    /// resources it carries (ADR-0018).
    ///
    /// # Errors
    ///
    /// A fault of [`NoRoom`] when a blob did not fit, and then none is kept, nor any resource.
    fn put_produced(&mut self, produced: Produced) -> Result<Answering, Fault> {
        let Produced { bytes, held, put, handed } = produced;
        let (mut bytes, held) = self.put_blobs(bytes, held, put)?;
        let mut kept = Vec::with_capacity(handed.len());
        for (at, resource) in handed {
            let ticket = self.ticket();
            let begins = usize::try_from(at).unwrap_or(usize::MAX);
            let written = bytes.get_mut(begins..begins.saturating_add(8));
            if let Some(written) = written {
                written.copy_from_slice(&ticket.to_le_bytes());
            }
            kept.push(resource.ticketed(ticket));
        }

        Ok(Answering { bytes, held, kept })
    }

    /// Puts each of `put`, the blobs a job put, out of the budget for blobs, and writes over each reference to one in
    /// `bytes`, its result, the blob's own: the result's bytes, and the pins on the blobs it refers to.
    ///
    /// # Errors
    ///
    /// A fault of [`NoRoom`] when a blob did not fit, and then none is kept.
    fn put_blobs(
        &mut self,
        mut bytes: Vec<u8>,
        held: Vec<Pinning>,
        put: Vec<Vec<u8>>,
    ) -> Result<(Vec<u8>, Vec<Pinning>), Fault> {
        // Most jobs put no blob.
        if put.is_empty() {
            return Ok((bytes, held));
        }
        let mut blobs = Vec::with_capacity(put.len());
        for blob in put {
            blobs.push(self.put_blob(blob).map_err(|_| fault::<NoRoom>())?);
        }
        let mut written = Vec::with_capacity(held.len());
        for (at, pin) in held {
            let index = pin.id().made_index().and_then(|index| usize::try_from(index).ok());
            let Some(blob) = index.and_then(|index| blobs.get(index)) else {
                written.push((at, pin));
                continue;
            };
            // A reference is its engine's number, then its own: the number is written over.
            let begins = usize::try_from(at).unwrap_or(usize::MAX).saturating_add(4);
            let number = bytes.get_mut(begins..begins.saturating_add(8));
            if let Some(number) = number {
                number.copy_from_slice(&blob.id().bits().to_le_bytes());
            }
            written.push((at, blob.clone()));
        }

        Ok((bytes, written))
    }
}
