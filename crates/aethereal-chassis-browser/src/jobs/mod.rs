//! The browser's runner of jobs: every job a task on the page's own event loop, read and run inside the task that
//! drives it (ADR-0018).

#[cfg(test)]
mod tests;
#[cfg(target_arch = "wasm32")]
mod timeouts;

use aethereal_actor::{Driven, Fault, JobId, JobType, Outcome, Runner, Storage, Timer};
use aethereal_data::{Kind, KindId};
use aethereal_substrate::{Inline, Jobs, Panicked, Taken};
use std::collections::HashMap;
use std::future::{self, Future};
use std::mem;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};

/// A task on the event loop: a job's run, from reading it to pushing its outcome.
type Task = Pin<Box<dyn Future<Output = ()>>>;

/// Runs every job on the page's own event loop, each as a task of its own once it is handed out, never inside a step,
/// so its answer comes at the bring of a later frame (ADR-0018).
///
/// A job for computing runs once, as [`Inline`] runs it: its future is ready the first time it is polled, and one that
/// waits is answered with [`Waits`](aethereal_substrate::Waits). A job that waits, [`Sleep`](aethereal_actor::Sleep)
/// among them, is polled again whenever it is woken, on the page's timer, `setTimeout`, read from the clock the page's
/// frames are timed by.
///
/// It lets any number of jobs be in flight: a job that waits holds nothing a computation could use. It holds only what
/// may be shared: what each task needs that may not, it makes inside the task. A page built to abort on a panic loses
/// itself to a job's panic, as to any other: nothing is caught there.
pub struct EventLoop {
    /// How each job for computing is run, in a task of its own.
    inline: Arc<Inline>,
    /// The timer the jobs that wait wait on.
    timer: Arc<dyn Timer>,
    /// The storage the jobs that wait read files from (ADR-0032).
    storage: Arc<dyn Storage>,
    /// How a task is put on the event loop.
    spawn: fn(Task),
    /// The jobs that wait it holds, by their keys, so it can stop them.
    held: Arc<Held>,
}

/// The jobs that wait an event loop holds, each as its task and a stop share it.
///
/// A job's task and a stop reach it from two places, the event loop and the engine's fold, so it is behind a lock: one
/// mail at a time orders neither. A page has one thread, so the lock is never contended.
#[derive(Default)]
struct Held {
    jobs: Mutex<HashMap<Key, Arc<Mutex<Stage>>>>,
}

/// A job's key among those the event loop holds: its actor's id and its count.
type Key = (u64, u64);

/// Where a job that waits stands, as its task and a stop share it.
enum Stage {
    /// Handed out, and its task not yet polled: nothing wakes it yet.
    Handed,
    /// Polled, and waiting to be woken by this.
    Waiting(Waker),
    /// Stopped: its task ends at its next poll, dropping the job unanswered.
    Stopped,
}

/// What came of polling a job that waits.
enum Ended {
    /// It ran to its outcome.
    Ran(Outcome),
    /// It was stopped first.
    Stopped,
}

impl EventLoop {
    /// Runs jobs on the page's event loop, waiting on the page's timer and reading files from `storage`.
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self::on(Arc::new(timeouts::Timeouts), storage, wasm_bindgen_futures::spawn_local::<Task>)
    }

    /// Runs jobs as tasks `spawn` puts on an event loop, waiting on `timer` and reading files from `storage`.
    fn on(timer: Arc<dyn Timer>, storage: Arc<dyn Storage>, spawn: fn(Task)) -> Self {
        let inline = Arc::new(Inline::new(Arc::clone(&storage)));

        Self { inline, timer, storage, spawn, held: Arc::default() }
    }

    /// Puts the job `id` of the type `of`, which waits, on the event loop as a task polled whenever it is woken, until
    /// it is ready or stopped.
    fn wait(&self, id: JobId, of: JobType, job: Taken) {
        let key = key(id);
        let stage = Arc::new(Mutex::new(Stage::Handed));
        self.held.lock().insert(key, Arc::clone(&stage));
        let held = Arc::clone(&self.held);
        let timer = Arc::clone(&self.timer);
        let storage = Arc::clone(&self.storage);
        (self.spawn)(Box::pin(async move {
            let Taken { bytes, lent, limit, finish } = job;
            let mut running = of.drive(bytes, lent, limit, timer, storage);
            let ended = future::poll_fn(|context| poll(&stage, &mut running, context)).await;
            // Dropped before it is answered, so the pins its future holds are let go of first.
            drop(running);
            if let Ended::Ran(outcome) = ended {
                held.lock().remove(&key);
                finish.done(outcome);
            }
        }));
    }
}

impl Held {
    /// The jobs, whatever a task that panicked holding them left.
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, Arc<Mutex<Stage>>>> {
        self.jobs.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The stage of a job, whatever a task that panicked holding it left.
fn stage_of(stage: &Mutex<Stage>) -> MutexGuard<'_, Stage> {
    stage.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Polls `running`, whose stage is `stage`, in `context`: ends at once when it was stopped, and otherwise is woken
/// again by `context`'s waker, or a stop.
fn poll(stage: &Mutex<Stage>, running: &mut Driven, context: &mut Context<'_>) -> Poll<Ended> {
    {
        let mut stage = stage_of(stage);
        let is_stopped = matches!(*stage, Stage::Stopped);
        if is_stopped {
            return Poll::Ready(Ended::Stopped);
        }
        *stage = Stage::Waiting(context.waker().clone());
    }
    let polled = panic::catch_unwind(AssertUnwindSafe(|| running.as_mut().poll(context)));

    match polled {
        Ok(Poll::Pending) => Poll::Pending,
        Ok(Poll::Ready(outcome)) => Poll::Ready(Ended::Ran(outcome)),
        Err(_) => Poll::Ready(Ended::Ran(Err(fault::<Panicked>()))),
    }
}

impl Jobs for EventLoop {
    fn start(&self, id: JobId, of: JobType, job: Taken) {
        let waits = of.runner() == Runner::Io;
        if waits {
            self.wait(id, of, job);
            return;
        }
        let inline = Arc::clone(&self.inline);
        (self.spawn)(Box::pin(async move { inline.start(id, of, job) }));
    }

    /// A job that waits is dropped at its task's next poll, which the stop wakes it for. One for computing runs once,
    /// and is left to finish.
    fn stop(&self, id: JobId) {
        let Some(stage) = self.held.lock().remove(&key(id)) else {
            return;
        };
        let was = mem::replace(&mut *stage_of(&stage), Stage::Stopped);
        // Woken once the stage is let go of, should waking poll it at once.
        if let Stage::Waiting(waker) = was {
            waker.wake();
        }
    }

    fn most(&self) -> usize {
        usize::MAX
    }
}

/// The key of the job `id`.
const fn key(id: JobId) -> Key {
    let mail = id.mail();

    (mail.sender.bits(), mail.count)
}

/// The fault whose reason is `K`, which holds nothing.
fn fault<K: Kind>() -> Fault {
    Fault::of_parts(KindId::of::<K>(), Vec::new())
}
