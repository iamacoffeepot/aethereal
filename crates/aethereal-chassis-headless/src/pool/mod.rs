//! The workers a step's shares run on: spinning, parked, or woken, as each step's plan says (ADR-0011).

mod worker;

use crate::Monotonic;
use aethereal_substrate::{Clock, Done, Fold, Folding, Reading, Share, Workers};
use std::num::NonZeroUsize;
use std::panic;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use worker::{Lent, Order, Reply, Report, Waiting};

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
    measured: Measured,
}

/// What the steps run so far cost, summed: for comparing one way of running a step with another (ADR-0011).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Measured {
    /// Steps run.
    pub steps: u64,
    /// Nanoseconds spent planning each step, on the stepping thread.
    pub plan_nanos: u64,
    /// Nanoseconds from handing out each step's shares to the last answer.
    pub step_nanos: u64,
    /// Nanoseconds spent applying each step, on the stepping thread.
    pub apply_nanos: u64,
    /// Nanoseconds the workers that ran each step stood idle in it while others still ran.
    pub idle_nanos: u64,
    /// Parked workers woken.
    pub wakes: u64,
    /// Running workers parked.
    pub parks: u64,
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
        let mut pool = Self { threads, replies, spinning: 0, wake_nanos: 0, measured: Measured::default() };
        pool.wake_nanos = pool.measure_wake(clock);

        pool
    }

    /// The workers as a step's plan sees them.
    pub const fn workers(&self) -> Workers {
        Workers { active: self.spinning + 1, max: self.threads.len() + 1, wake_nanos: self.wake_nanos }
    }

    /// What the steps run so far cost.
    pub const fn measured(&self) -> Measured {
        self.measured
    }

    /// Runs a step's shares: the first on this thread, each other on a worker,
    /// and answers with what they left. Workers past the shares are parked; the ones given a share spin after it.
    ///
    /// The workers read `reading` while they run, so this returns only once every one of them has answered.
    ///
    /// # Panics
    ///
    /// Panics with a share's own panic, once every worker has answered.
    pub fn lend(&mut self, reading: &Reading<'_>, clock: Monotonic, shares: Vec<Share>) -> Vec<Done> {
        let start = clock.nanos();
        let running = shares.len().saturating_sub(1).min(self.threads.len());
        for parked in &self.threads[running..self.spinning.max(running)] {
            let _ = parked.orders.send(Order::Park);
        }
        self.measured.wakes += u64::try_from(running.saturating_sub(self.spinning)).unwrap_or(0);
        self.measured.parks += u64::try_from(self.spinning.saturating_sub(running)).unwrap_or(0);
        self.spinning = running;

        let (done, reports) = self.spread(
            shares,
            |share| Order::Run { lend: Lent::run(reading, clock, share), then: Waiting::Spin },
            |share| worker::perform(reading, clock, share),
            |reply| match reply {
                Reply::Done(done, report) => Some((done, report)),
                _ => None,
            },
        );
        self.measure(clock.nanos().saturating_sub(start), &reports);

        done
    }

    /// Folds a step's folds: the first on this thread, each other on a worker that ran a share of the step and
    /// spins after it, and answers with them folded.
    ///
    /// The workers read `folding` while they fold, so this returns only once every one of them has answered.
    ///
    /// # Panics
    ///
    /// Panics with a fold's own panic, once every worker has answered.
    pub fn fold(&mut self, folding: &Folding<'_>, clock: Monotonic, folds: Vec<Fold>) -> Vec<Fold> {
        let (folded, _) = self.spread(
            folds,
            |share| Order::Fold { lend: Lent::fold(folding, clock, share) },
            |share| worker::fold(folding, clock, share),
            |reply| match reply {
                Reply::Folded(fold, report) => Some((fold, report)),
                _ => None,
            },
        );

        folded
    }

    /// Hands out `shares`: the first is done on this thread with `here`, each other is sent to a worker as `order`
    /// says, and any past the workers is done here after the first. Answers with what each left, as `answer` takes
    /// it from a worker's reply, and how long each kept its thread busy.
    fn spread<S, D>(
        &self,
        shares: Vec<S>,
        order: impl Fn(S) -> Order,
        here: impl Fn(S) -> (D, Report),
        answer: impl Fn(Reply) -> Option<(D, Report)>,
    ) -> (Vec<D>, Vec<Report>) {
        let mut shares = shares.into_iter();
        let first = shares.next();
        let theirs: Vec<S> = shares.by_ref().take(self.threads.len()).collect();
        // A plan has no more shares than workers; any past them would run here, after the first.
        let mine: Vec<S> = first.into_iter().chain(shares).collect();

        let mut sent = 0;
        for (thread, share) in self.threads.iter().zip(theirs) {
            let is_sent = thread.orders.send(order(share)).is_ok();
            sent += usize::from(is_sent);
        }

        let mut done = Vec::with_capacity(sent + mine.len());
        let mut reports = Vec::with_capacity(sent + 1);
        let mut panicked = Vec::new();
        for share in mine {
            match panic::catch_unwind(panic::AssertUnwindSafe(|| here(share))) {
                Ok((left, report)) => {
                    done.push(left);
                    reports.push(report);
                }
                Err(payload) => panicked.push(payload),
            }
        }
        // Every worker lent what it reads answers before it goes. A worker that is gone has stopped reading it.
        for _ in 0..sent {
            match self.replies.recv() {
                Ok(Reply::Panicked(payload)) => panicked.push(payload),
                Ok(reply) => {
                    if let Some((left, report)) = answer(reply) {
                        done.push(left);
                        reports.push(report);
                    }
                }
                Err(_) => break,
            }
        }

        if let Some(payload) = panicked.into_iter().next() {
            panic::resume_unwind(payload);
        }

        (done, reports)
    }

    /// Adds a step's planning and applying, which took `plan_nanos` and `apply_nanos`, to what is measured.
    pub const fn measure_around(&mut self, plan_nanos: u64, apply_nanos: u64) {
        self.measured.plan_nanos += plan_nanos;
        self.measured.apply_nanos += apply_nanos;
    }

    /// Adds a step that took `nanos`, whose workers reported `reports`, to what is measured.
    fn measure(&mut self, nanos: u64, reports: &[Report]) {
        self.measured.steps += 1;
        self.measured.step_nanos += nanos;
        for report in reports {
            self.measured.idle_nanos += nanos.saturating_sub(report.busy_nanos);
        }
    }

    /// Parks every spinning worker: for when no step is coming soon.
    pub fn park(&mut self) {
        for spinning in &self.threads[..self.spinning] {
            let _ = spinning.orders.send(Order::Park);
        }
        self.measured.parks += u64::try_from(self.spinning).unwrap_or(0);
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
