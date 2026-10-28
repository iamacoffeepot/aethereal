//! The native host's runners of jobs: a pool of threads beside the engine's workers, each job read and run on the
//! thread that takes it (ADR-0018).

mod io;
#[cfg(test)]
mod tests;

use crate::Monotonic;
use aethereal_actor::{JobId, JobType, Runner, Services};
use aethereal_substrate::{Inline, Jobs, Taken};
use crossbeam_queue::SegQueue;
use io::Io;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle, Thread};

/// The native host's runners (ADR-0018): threads for computing, as many as it is made with, each running a job it
/// takes by polling it once, as [`Inline`] does; and one thread for waiting, which runs each `Io` job, [`Sleep`] among
/// them, polling it whenever it is woken, on a timer read from the clock the host's steps are timed by.
///
/// [`Sleep`]: aethereal_actor::Sleep
///
/// A thread with nothing to take parks until a job is handed out. It lets
/// any number of jobs be in flight: a job that waits holds no thread, so a cap
/// on them would only keep a computation waiting behind a sleep. Dropping it
/// stops its threads once each has finished what it was running, and drops
/// the jobs not finished: they go unanswered.
pub struct Runners {
    shared: Arc<Shared>,
    /// The threads for computing, which the jobs handed out wake in turn.
    computing: Vec<Thread>,
    /// Which of them the next job handed out wakes.
    next: AtomicUsize,
    io: Io,
    /// Every thread, to join once they are told to stop.
    threads: Vec<JoinHandle<()>>,
}

/// What the threads share: what runs each job, the jobs handed out and not yet taken, and whether to stop.
struct Shared {
    inline: Inline,
    queue: SegQueue<(JobId, JobType, Taken)>,
    stopping: AtomicBool,
}

impl Runners {
    /// Runners on `threads` threads of their own, their jobs reaching the host's `services` (ADR-0034).
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(threads: NonZeroUsize, services: Arc<Services>) -> Self {
        let inline = Inline::new(Arc::clone(&services));
        let shared = Arc::new(Shared { inline, queue: SegQueue::new(), stopping: AtomicBool::new(false) });
        let mut threads: Vec<JoinHandle<()>> = (0..threads.get())
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
        let computing = threads.iter().map(|thread| thread.thread().clone()).collect();
        let (io, waiting) = Io::new(Monotonic::new(), services);
        threads.push(waiting);

        Self { shared, computing, next: AtomicUsize::new(0), io, threads }
    }

    /// Runners on the cores an engine run on `workers` workers leaves free, one at the least, their jobs reaching the
    /// host's `services`.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn beside(workers: NonZeroUsize, services: Arc<Services>) -> Self {
        let cores = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let free = NonZeroUsize::new(cores.saturating_sub(workers.get())).unwrap_or(NonZeroUsize::MIN);

        Self::new(free, services)
    }
}

/// Takes jobs and runs each until it is told to stop, parking whenever there is none to take.
fn run(shared: &Shared) {
    loop {
        while let Some((id, of, job)) = shared.queue.pop() {
            shared.inline.start(id, of, job);
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        // A job handed out after the queue was found empty unparks this thread, and then this returns at once.
        thread::park();
    }
}

impl Jobs for Runners {
    fn start(&self, id: JobId, of: JobType, job: Taken) {
        let waits = of.runner() == Runner::Io;
        if waits {
            self.io.start(id, of, job);
            return;
        }
        self.shared.queue.push((id, of, job));
        let next = self.next.fetch_add(1, Ordering::Relaxed) % self.computing.len();
        if let Some(thread) = self.computing.get(next) {
            thread.unpark();
        }
    }

    /// A job that waits is dropped. One for computing is polled once, and has finished by the time it is taken.
    fn stop(&self, id: JobId) {
        self.io.stop(id);
    }

    fn most(&self) -> usize {
        usize::MAX
    }
}

impl Drop for Runners {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.io.end();
        for thread in &self.computing {
            thread.unpark();
        }
        for thread in self.threads.drain(..) {
            // A thread that panicked has gone already, and its job with it.
            let _ = thread.join();
        }
    }
}
