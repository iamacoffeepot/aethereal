//! The workers a step's shares run on: spinning, parked, or woken, as each step's plan says (ADR-0011).

mod cores;
mod slot;
mod worker;

use crate::Monotonic;
use aethereal_substrate::{Clock, Done, Fold, Folding, Reading, Share, SplitCost, Workers};
use cores::{Cores, Kept};
use slot::Board;
use std::num::NonZeroUsize;
use std::panic;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::vec::Drain;
use worker::{Lent, Order, Reply, Report, Waiting};

/// The threads besides the stepping thread that run a step's shares.
///
/// The stepping thread runs the first share itself, so a pool of one worker
/// has no thread of its own. Workers `0..spinning` spin between steps; the
/// rest are parked.
pub struct Pool {
    crew: Crew,
    /// What the workers left from running a step, kept from one step to the next for its room.
    done: Vec<Done>,
    /// The folds the workers folded, kept likewise.
    folded: Vec<Fold>,
    spinning: usize,
    wake_nanos: u64,
    measured: Measured,
    /// Where the stepping thread is kept while the pool lives.
    _kept: Kept,
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

/// The worker threads, and the board their orders and answers are left on.
struct Crew {
    /// Each worker's thread, by its number less one: the stepping thread is the first worker and has none.
    threads: Vec<JoinHandle<()>>,
    board: Arc<Board>,
    /// How many answers the stepping thread has taken from the board, ever.
    heard: usize,
    /// The answers taken from the board for the round of orders under way, empty between rounds.
    answers: Vec<Reply>,
}

/// How many times a parked worker is woken to measure what a wake costs.
const WAKES_MEASURED: usize = 16;

impl Pool {
    /// A pool of `workers` workers, the stepping thread among them, all parked, with the cost of a wake measured.
    ///
    /// The calling thread is the stepping thread: it is kept to the first workers' cores until the pool is dropped.
    pub fn new(workers: NonZeroUsize, clock: &Monotonic) -> Self {
        let cores = Arc::new(Cores::of_host());
        let kept = cores.keep_stepping();
        let board = Arc::new(Board::new(workers.get() - 1));
        let threads = (1..workers.get())
            .map(|number| {
                let cores = Arc::clone(&cores);
                let board = Arc::clone(&board);
                #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
                thread::spawn(move || {
                    cores.pin(number);
                    worker::work(&board, number - 1);
                })
            })
            .collect();
        let mut pool = Self {
            crew: Crew { threads, board, heard: 0, answers: Vec::new() },
            done: Vec::new(),
            folded: Vec::new(),
            spinning: 0,
            wake_nanos: 0,
            measured: Measured::default(),
            _kept: kept,
        };
        pool.wake_nanos = pool.measure_wake(clock);

        pool
    }

    /// The workers as a step's plan sees them, a split among them costing `split`.
    pub const fn workers(&self, split: SplitCost) -> Workers {
        Workers { active: self.spinning + 1, max: self.crew.threads.len() + 1, wake_nanos: self.wake_nanos, split }
    }

    /// What the steps run so far cost.
    pub const fn measured(&self) -> Measured {
        self.measured
    }

    /// Runs a step's shares: the first on this thread, each other on a worker,
    /// and answers with what they left. Of the workers past the shares, those the plan parks, by `delta` when it is
    /// below zero, are parked, and the rest spin through it, for the steps after it; the ones given a share spin
    /// after it.
    ///
    /// The workers read `reading` while they run, so this returns only once every one of them has answered.
    ///
    /// # Panics
    ///
    /// Panics with a share's own panic, once every worker has answered.
    pub fn lend(
        &mut self,
        reading: &Reading<'_>,
        clock: Monotonic,
        delta: isize,
        shares: Vec<Share>,
    ) -> Drain<'_, Done> {
        let start = clock.nanos();
        let running = shares.len().saturating_sub(1).min(self.crew.threads.len());
        let parked = delta.min(0).unsigned_abs();
        let kept = self.spinning.saturating_sub(parked).max(running);
        for spinning in kept..self.spinning {
            self.crew.post(spinning, Order::Park);
        }
        self.measured.wakes += u64::try_from(kept.saturating_sub(self.spinning)).unwrap_or(0);
        self.measured.parks += u64::try_from(self.spinning.saturating_sub(kept)).unwrap_or(0);
        self.spinning = kept;

        let busy = spread(
            &mut self.crew,
            clock,
            self.wake_nanos,
            shares,
            |share| Order::Run { lend: Lent::run(reading, clock, share), then: Waiting::Spin },
            |share| worker::perform(reading, clock, share),
            |reply| match reply {
                Reply::Done(done, report) => Some((done, report)),
                _ => None,
            },
            &mut self.done,
        );
        self.measure(clock.nanos().saturating_sub(start), busy);

        self.done.drain(..)
    }

    /// Runs a step planned for one worker on this thread, by `run`, and answers with what it left. None of the
    /// workers runs a share of it: those the plan parks, by `delta` when it is below zero, are parked, and the rest
    /// spin through it, for the steps after it.
    pub fn alone<T>(&mut self, clock: Monotonic, delta: isize, run: impl FnOnce() -> T) -> T {
        let start = clock.nanos();
        let parked = delta.min(0).unsigned_abs().min(self.spinning);
        let kept = self.spinning - parked;
        for spinning in kept..self.spinning {
            self.crew.post(spinning, Order::Park);
        }
        self.measured.parks += u64::try_from(parked).unwrap_or(0);
        self.spinning = kept;
        let left = run();
        let nanos = clock.nanos().saturating_sub(start);
        self.measure(nanos, Busy { shares: 1, nanos });

        left
    }

    /// Folds a step's folds: the first on this thread, each other on a worker that ran a share of the step and
    /// spins after it, and answers with them folded.
    ///
    /// The workers read `folding` while they fold, so this returns only once every one of them has answered.
    ///
    /// # Panics
    ///
    /// Panics with a fold's own panic, once every worker has answered.
    pub fn fold(&mut self, folding: &Folding<'_>, clock: Monotonic, folds: Vec<Fold>) -> Drain<'_, Fold> {
        spread(
            &mut self.crew,
            clock,
            self.wake_nanos,
            folds,
            |share| Order::Fold { lend: Lent::fold(folding, clock, share) },
            |share| worker::fold(folding, clock, share),
            |reply| match reply {
                Reply::Folded(fold, report) => Some((fold, report)),
                _ => None,
            },
            &mut self.folded,
        );

        self.folded.drain(..)
    }

    /// Adds a step's planning and applying, which took `plan_nanos` and `apply_nanos`, to what is measured.
    pub const fn measure_around(&mut self, plan_nanos: u64, apply_nanos: u64) {
        self.measured.plan_nanos += plan_nanos;
        self.measured.apply_nanos += apply_nanos;
    }

    /// Adds a step that took `nanos`, whose shares kept their threads busy as `busy` sums up, to what is measured.
    const fn measure(&mut self, nanos: u64, busy: Busy) {
        self.measured.steps += 1;
        self.measured.step_nanos += nanos;
        self.measured.idle_nanos += (nanos * busy.shares).saturating_sub(busy.nanos);
    }

    /// Forgets what the steps run so far cost: for steps a host ran for itself, such as a rehearsal's.
    pub fn forget_measured(&mut self) {
        self.measured = Measured::default();
    }

    /// Parks every spinning worker: for when no step is coming soon.
    pub fn park(&mut self) {
        for spinning in 0..self.spinning {
            self.crew.post(spinning, Order::Park);
        }
        self.measured.parks += u64::try_from(self.spinning).unwrap_or(0);
        self.spinning = 0;
    }

    /// The median time, in nanoseconds, from pinging a parked worker to its answer.
    fn measure_wake(&mut self, clock: &Monotonic) -> u64 {
        if self.crew.threads.is_empty() {
            return 0;
        }

        let mut wakes: Vec<u64> = (0..WAKES_MEASURED)
            .map(|_| {
                let start = clock.nanos();
                self.crew.post(0, Order::Ping);
                self.crew.wait(1, *clock, 0);
                self.crew.answers.clear();

                clock.nanos().saturating_sub(start)
            })
            .collect();
        wakes.sort_unstable();

        wakes.get(wakes.len() / 2).copied().unwrap_or(0)
    }
}

/// How long a step's shares kept their threads busy: how many there were, and their nanoseconds summed.
#[derive(Clone, Copy, Default)]
struct Busy {
    shares: u64,
    nanos: u64,
}

/// Hands out `shares` among the workers of `crew`: the first is done on this thread with `here`, each other is left
/// for a worker as `order` says, and any past the workers is done here after the first. Then waits for the workers'
/// answers, spinning for at most `spin_nanos` by `clock` before it parks. Adds what each left to `done`, as `answer`
/// takes it from a worker's reply, and answers with how long they kept their threads busy.
#[allow(
    clippy::too_many_arguments,
    reason = "one round of orders: the shares, how each is done and answered, and how to wait"
)]
fn spread<S, D>(
    crew: &mut Crew,
    clock: Monotonic,
    spin_nanos: u64,
    shares: Vec<S>,
    order: impl Fn(S) -> Order,
    here: impl Fn(S) -> (D, Report),
    answer: impl Fn(Reply) -> Option<(D, Report)>,
    done: &mut Vec<D>,
) -> Busy {
    let mut shares = shares.into_iter();
    let first = shares.next();
    let workers = crew.threads.len();
    let mut sent = 0;
    for share in shares.by_ref().take(workers) {
        crew.post(sent, order(share));
        sent += 1;
    }

    let mut busy = Busy::default();
    let mut panicked = Vec::new();
    // A plan has no more shares than workers; any past them run here, after the first.
    for share in first.into_iter().chain(shares) {
        match panic::catch_unwind(panic::AssertUnwindSafe(|| here(share))) {
            Ok((left, report)) => {
                done.push(left);
                busy.shares += 1;
                busy.nanos += report.busy_nanos;
            }
            Err(payload) => panicked.push(payload),
        }
    }
    // Every worker lent what it reads answers before it goes.
    crew.wait(sent, clock, spin_nanos);
    for reply in crew.answers.drain(..) {
        match reply {
            Reply::Panicked(payload) => panicked.push(payload),
            reply => {
                if let Some((left, report)) = answer(reply) {
                    done.push(left);
                    busy.shares += 1;
                    busy.nanos += report.busy_nanos;
                }
            }
        }
    }

    if let Some(payload) = panicked.into_iter().next() {
        panic::resume_unwind(payload);
    }

    busy
}

impl Crew {
    /// Leaves `order` for the worker numbered `worker`, less one, waking it if it is parked.
    fn post(&self, worker: usize, order: Order) {
        self.board.post(worker, self.threads[worker].thread(), order);
    }

    /// Waits for `count` more answers, and takes every worker's into `answers`. It spins for at most `spin_nanos` by
    /// `clock`, then parks: spinning longer than a wake costs saves less than the wake it spares.
    fn wait(&mut self, count: usize, clock: Monotonic, spin_nanos: u64) {
        self.heard += count;
        let start = clock.nanos();
        self.board.wait_for(self.heard, || clock.nanos().saturating_sub(start) < spin_nanos);
        for worker in 0..self.threads.len() {
            self.board.answers(worker, &mut self.answers);
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for worker in 0..self.crew.threads.len() {
            self.crew.post(worker, Order::Stop);
        }
        for handle in self.crew.threads.drain(..) {
            let _ = handle.join();
        }
    }
}
