//! The native host's runners of jobs: a pool of threads beside the engine's workers, each job read and run on the
//! thread that takes it (ADR-0018).

#[cfg(test)]
mod tests;

use aethereal_actor::{JobId, JobType};
use aethereal_data::KindId;
use aethereal_substrate::{Inline, Jobs, Taken};
use crossbeam_queue::SegQueue;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};

/// The native host's runners: threads for computing, as many as it is made with, each running a job it takes by
/// polling it once, as [`Inline`] does (ADR-0018).
///
/// It lets as many jobs be in flight as it has threads, so the engine keeps
/// the rest waiting in the order they were run, and the queue its threads take
/// from never holds more than they can start on. A thread with nothing to take
/// parks until a job is handed out. Dropping it stops its threads once each
/// has finished the job it holds, and drops the jobs not started: they go
/// unanswered.
///
/// Until a runner for waiting is made, a job that waits, an `Io` job among
/// them, runs here too, and is answered with `Waits` as [`Inline`] answers it.
pub struct Runners {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
    /// Which thread the next job handed out wakes.
    next: AtomicUsize,
}

/// What the threads share: how to run each job type, the jobs handed out and not yet taken, and whether to stop.
struct Shared {
    inline: Inline,
    queue: SegQueue<(JobId, KindId, Taken)>,
    stopping: AtomicBool,
}

impl Runners {
    /// Runners of the job types `types` on `threads` threads of their own.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(types: Vec<JobType>, threads: NonZeroUsize) -> Self {
        let shared =
            Arc::new(Shared { inline: Inline::new(types), queue: SegQueue::new(), stopping: AtomicBool::new(false) });
        let threads = (0..threads.get())
            .map(|number| {
                let shared = Arc::clone(&shared);
                #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
                let spawned =
                    thread::Builder::new().name(format!("aethereal-job-{number}")).spawn(move || run(&shared));
                let Ok(thread) = spawned else {
                    panic!("the host makes a thread for jobs");
                };
                thread
            })
            .collect();

        Self { shared, threads, next: AtomicUsize::new(0) }
    }

    /// Runners of the job types `types` on the cores an engine run on `workers` workers leaves free, one at the least.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn beside(types: Vec<JobType>, workers: NonZeroUsize) -> Self {
        let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let free = NonZeroUsize::new(cores.saturating_sub(workers.get())).unwrap_or(NonZeroUsize::MIN);

        Self::new(types, free)
    }
}

/// Takes jobs and runs each until it is told to stop, parking whenever there is none to take.
fn run(shared: &Shared) {
    loop {
        while let Some((id, kind, job)) = shared.queue.pop() {
            shared.inline.start(id, kind, job);
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        // A job handed out after the queue was found empty unparks this thread, and then this returns at once.
        thread::park();
    }
}

impl Jobs for Runners {
    fn start(&self, id: JobId, kind: KindId, job: Taken) {
        self.shared.queue.push((id, kind, job));
        let next = self.next.fetch_add(1, Ordering::Relaxed) % self.threads.len();
        if let Some(thread) = self.threads.get(next) {
            thread.thread().unpark();
        }
    }

    /// A job is polled once and has finished as it is taken, so none is left to stop.
    fn stop(&self, _: JobId) {}

    fn most(&self) -> usize {
        self.threads.len()
    }
}

impl Drop for Runners {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        for thread in &self.threads {
            thread.thread().unpark();
        }
        for thread in self.threads.drain(..) {
            // A thread that panicked has gone already, and its job with it.
            let _ = thread.join();
        }
    }
}
