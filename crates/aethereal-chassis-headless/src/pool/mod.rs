//! The workers a step's shares run on: spinning, parked, or woken, as each step's plan says (ADR-0011).

mod worker;

use crate::Monotonic;
use aethereal_substrate::{Clock, Done, Reading, Share, Workers};
use std::num::NonZeroUsize;
use std::panic;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use worker::{Lend, Order, Reply, Waiting};

/// The threads besides the stepping thread that run a step's shares.
///
/// The stepping thread runs the first share itself, so a pool of one worker
/// has no thread of its own. Workers `0..spinning` spin between steps; the
/// rest are parked.
pub struct Pool {
    threads: Vec<Thread>,
    replies: Receiver<Reply>,
    spinning: usize,
    wake_nanos: u64,
}

/// One worker thread, and how to tell it things.
struct Thread {
    orders: Sender<Order>,
    handle: JoinHandle<()>,
}

/// How many times a parked worker is woken to measure what a wake costs.
const WAKES_MEASURED: usize = 16;

impl Pool {
    /// A pool of `workers` workers, the stepping thread among them, all parked, with the cost of a wake measured.
    pub fn new(workers: NonZeroUsize, clock: &Monotonic) -> Self {
        let (reply, replies) = mpsc::channel();
        let threads = (1..workers.get())
            .map(|_| {
                let (orders, inbox) = mpsc::channel();
                let reply = reply.clone();
                #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
                let handle = thread::spawn(move || worker::work(&inbox, &reply));

                Thread { orders, handle }
            })
            .collect();
        let mut pool = Self { threads, replies, spinning: 0, wake_nanos: 0 };
        pool.wake_nanos = pool.measure_wake(clock);

        pool
    }

    /// The workers as a step's plan sees them.
    pub const fn workers(&self) -> Workers {
        Workers { active: self.spinning + 1, max: self.threads.len() + 1, wake_nanos: self.wake_nanos }
    }

    /// Runs a step's shares: the first on this thread, each other on a worker, and answers with what they left,
    /// in the order of the shares. Workers past the shares are parked; the ones given a share spin after it.
    ///
    /// The workers read `reading` while they run, so this returns only once every one of them has answered.
    ///
    /// # Panics
    ///
    /// Panics with a share's own panic, once every worker has answered.
    pub fn lend(&mut self, reading: &Reading<'_>, clock: Monotonic, shares: Vec<Share>) -> Vec<Done> {
        let mut shares = shares.into_iter();
        let first = shares.next();
        let theirs: Vec<Share> = shares.by_ref().take(self.threads.len()).collect();
        // A plan has no more shares than workers; any past them would run here, after the first.
        let mine: Vec<Share> = first.into_iter().chain(shares).collect();
        let running = theirs.len();

        for parked in &self.threads[running..self.spinning.max(running)] {
            let _ = parked.orders.send(Order::Park);
        }
        self.spinning = running;

        let mut lent = 0;
        for (thread, share) in self.threads.iter().zip(theirs) {
            let order = Order::Run { lend: Lend::new(reading, clock, share), then: Waiting::Spin };
            let is_sent = thread.orders.send(order).is_ok();
            lent += usize::from(is_sent);
        }

        let mut done = Vec::with_capacity(lent + mine.len());
        let mut panicked = Vec::new();
        for share in mine {
            match panic::catch_unwind(panic::AssertUnwindSafe(|| reading.run(share, &clock))) {
                Ok(ran) => done.push(ran),
                Err(payload) => panicked.push(payload),
            }
        }
        // Every worker lent `reading` answers before it goes. A worker that is gone has stopped reading it.
        for _ in 0..lent {
            match self.replies.recv() {
                Ok(Reply::Done(ran)) => done.push(ran),
                Ok(Reply::Panicked(payload)) => panicked.push(payload),
                Ok(Reply::Pong) => {}
                Err(_) => break,
            }
        }

        if let Some(payload) = panicked.into_iter().next() {
            panic::resume_unwind(payload);
        }

        done
    }

    /// Parks every spinning worker: for when no step is coming soon.
    pub fn park(&mut self) {
        for spinning in &self.threads[..self.spinning] {
            let _ = spinning.orders.send(Order::Park);
        }
        self.spinning = 0;
    }

    /// The median time, in nanoseconds, from pinging a parked worker to its answer.
    fn measure_wake(&self, clock: &Monotonic) -> u64 {
        let Some(first) = self.threads.first() else {
            return 0;
        };

        let mut wakes: Vec<u64> = (0..WAKES_MEASURED)
            .filter_map(|_| {
                let start = clock.nanos();
                first.orders.send(Order::Ping).ok()?;
                self.replies.recv().ok()?;

                Some(clock.nanos().saturating_sub(start))
            })
            .collect();
        wakes.sort_unstable();

        wakes.get(wakes.len() / 2).copied().unwrap_or(0)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for Thread { orders, handle } in self.threads.drain(..) {
            drop(orders);
            let _ = handle.join();
        }
    }
}
