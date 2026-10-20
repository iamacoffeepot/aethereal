//! Jobs: the seam through which the engine has its host run the work turns hand it outside the steps, and the feed
//! their outcomes come back on (ADR-0018).

#[cfg(test)]
mod tests;

use super::{Engine, ROOTS, parity};
use crate::{lives, log};
use aethereal_actor::{Fault, JobId, JobType, MailId, Mailed, Outcome, Seated, Submitted, Tie, Unanswered};
use aethereal_data::{Kind, KindId, KindName, Plain};
use crossbeam_queue::SegQueue;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// What runs the jobs the engine hands out, which each host implements: a pool of threads natively, Web Workers in a
/// browser, or inline between steps where there is one thread (ADR-0018).
pub trait Jobs: Send + Sync {
    /// Runs the job `id`, of the kind `kind`, as `job` holds it: its bytes, and where its outcome goes once it has
    /// run.
    fn start(&self, id: JobId, kind: KindId, job: Taken);
}

/// A job as the engine hands it to its host: its bytes, the most of them it may read, and where its outcome goes.
pub struct Taken {
    /// The job's bytes, written at `Actors`.
    pub bytes: Vec<u8>,
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
}

impl Finish {
    /// Pushes the job's outcome onto the engine's feed, to answer the actor that ran it at the next bring.
    pub fn done(self, outcome: Outcome) {
        let Self { by, count, answer, feed } = self;
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

/// Runs each job inline, as it is handed out between steps: for a host with one thread, and for tests. A job whose
/// future is not ready the first time it is polled, one that waits, is answered with [`Waits`], as nothing would
/// poll it again.
pub struct Inline {
    types: Vec<JobType>,
}

impl Inline {
    /// Runs jobs of `types`, and answers any other with [`NoRunner`].
    #[must_use]
    pub const fn new(types: Vec<JobType>) -> Self {
        Self { types }
    }
}

impl Jobs for Inline {
    fn start(&self, _: JobId, kind: KindId, job: Taken) {
        let Taken { bytes, limit, finish } = job;
        let Some(of) = self.types.iter().find(|of| of.kind() == kind) else {
            finish.done(Err(fault::<NoRunner>()));
            return;
        };
        let mut future = of.drive(bytes, limit);
        let mut context = Context::from_waker(Waker::noop());
        let polled = panic::catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut context)));
        let outcome = match polled {
            Ok(Poll::Ready(outcome)) => outcome,
            Ok(Poll::Pending) => Err(fault::<Waits>()),
            Err(_) => Err(fault::<Panicked>()),
        };
        finish.done(outcome);
    }
}

/// The fault whose reason is `K`, which holds nothing.
fn fault<K: Kind>() -> Fault {
    Fault::of_parts(KindId::of::<K>(), Vec::new())
}

/// The reason of a job no runner the host holds runs: its type was not handed to the host.
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct NoRunner;

impl Kind for NoRunner {
    const NAME: KindName = KindName::new("aether.job.no_runner");
}

/// The reason of a job that waited where its runner polls each job once.
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Waits;

impl Kind for Waits {
    const NAME: KindName = KindName::new("aether.job.waits");
}

/// The reason of a job that panicked.
#[derive(Debug, PartialEq, Eq, Plain)]
pub struct Panicked;

impl Kind for Panicked {
    const NAME: KindName = KindName::new("aether.job.panicked");
}

impl Engine {
    /// The engine, its jobs run by `jobs`, which with none set answers every job that no runner runs it (ADR-0018).
    #[must_use]
    pub fn with_jobs(mut self, jobs: Arc<dyn Jobs>) -> Self {
        self.jobs = jobs;

        self
    }

    /// Hands each of `submitted` to the host to run, once the turns that ran them have.
    pub(super) fn start_jobs(&self, submitted: impl Iterator<Item = Submitted>) {
        for job in submitted {
            let id = job.id();
            let Submitted { by, count, kind, answer, bytes } = job;
            let finish = Finish { by, count, answer, feed: Arc::clone(&self.finished) };
            self.jobs.start(id, kind, Taken { bytes, limit: self.read_limit, finish });
        }
    }

    /// Brings in each job finished before this bring began, between steps: its outcome as mail from the actor that ran it
    /// to itself, tied to the job, the root of a chain of its own, which its handler of what the job answers with is
    /// handed with the context it kept (ADR-0018). A fault is written into the actor's log too. The outcome of a job
    /// whose actor has ended is dropped. What finishes while it brings waits for the next, so a bring is bounded
    /// however fast the runners push.
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
        for Finished { by, count, answer, outcome } in (0..ready).map_while(|_| feed.pop()) {
            let Some(life) = self.lives.shard(by.seat.number()).life(by) else {
                continue;
            };
            let sent = life.sent() + 1;
            let written = match outcome {
                Ok(bytes) => arena.write_bytes(&bytes).map(|bytes| (answer, bytes, Tie::answers(count))),
                Err(fault) => {
                    faults.push(log::job_faulted(by, fault));
                    let bytes = arena.write_mail(&Unanswered::Faulted);
                    bytes.map(|bytes| (KindId::of::<Unanswered>(), bytes, Tie::unanswered(count)))
                }
            };
            let Ok((kind, bytes, tie)) = written else {
                continue;
            };
            self.lives.fold_records([lives::sent(by, sent)], &[]);
            mailed.push(Mailed { by, to: by, count: sent, kind, bytes, tie });
        }
        self.arenas[parity][ROOTS] = arena;
        // Each answer is the first of its chain: nothing the actor did in the step it is brought in caused it.
        self.fold_brought(mailed, faults, parity, |mailed| MailId { sender: mailed.by.id, count: mailed.count });
    }
}
