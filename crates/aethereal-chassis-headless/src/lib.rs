//! The headless chassis: the native host with nothing but the engine, its workers, and its clock (ADR-0001).
//!
//! The substrate plans each step and runs a share of it wherever it is
//! asked to; this host owns the threads those shares run on and the clock
//! their turns are timed by. Between steps it wakes or parks workers as the
//! plan says (ADR-0011).

mod clock;
mod pool;
mod rehearsal;
#[cfg(test)]
mod tests;

pub use clock::Monotonic;

use aethereal_substrate::{Clock, Engine, Split, SplitCost, Workers};
pub use pool::Measured;

use pool::Pool;
use std::num::NonZeroUsize;

/// An engine run on native workers.
pub struct Headless {
    engine: Engine,
    pool: Pool,
    clock: Monotonic,
    /// What splitting a step among the workers costs, as rehearsed when the host was made: among two first, then
    /// three, and so on, as [`SplitCost::new`] reads it.
    split: Vec<u64>,
    /// Spike: steps of the whole engine to run before parts are let step on their own again.
    backoff: usize,
    /// Spike: whether parts may step on their own at all.
    free: bool,
}

impl Headless {
    /// `engine` run on at most `workers` workers, the calling thread among them.
    ///
    /// Every worker but the calling thread starts parked. What waking one costs, and what splitting a step among
    /// them does, are measured here.
    #[must_use]
    pub fn new(engine: Engine, workers: NonZeroUsize) -> Self {
        let clock = Monotonic::new();
        let mut pool = Pool::new(workers, &clock);
        let split = rehearsal::split_cost(&engine, &mut pool, clock);

        #[allow(clippy::disallowed_methods, reason = "spike switch")]
        let free = std::env::var("FREE").is_ok_and(|on| on == "1");
        Self { engine, pool, clock, split, backoff: 0, free }
    }

    /// The engine, as of the last step.
    #[must_use]
    pub const fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The engine, to add roots to between steps.
    pub const fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// What the steps run so far cost.
    #[must_use]
    pub const fn measured(&self) -> Measured {
        self.pool.measured()
    }

    /// Runs one step: plans it for the workers there are, runs its shares on them, and applies what they left, each
    /// part of the views folded on the workers that ran the step. A step planned for one worker runs whole on this
    /// thread, where its parts lie, and the workers the plan keeps spin through it.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn step(&mut self) {
        self.step_sharing();
    }

    /// Runs one step as [`Headless::step`] does, and answers with how many workers it was split among.
    fn step_sharing(&mut self) -> usize {
        let workers = self.pool.workers(SplitCost::new(&self.split));
        step(&mut self.engine, &mut self.pool, self.clock, workers).sharing
    }

    /// Spike: lets every part step on its own on every worker for at most `budget_nanos`, or until no part has
    /// anything to step, and answers with the turns run and the nanoseconds taken, when the engine could.
    fn run_free(&mut self, budget_nanos: u64) -> Option<(u64, u64)> {
        let workers = self.pool.workers(SplitCost::NONE).max;
        let (shares, parts) = self.engine.free_shares(workers)?;
        let began = self.clock.nanos();
        let turns = aethereal_substrate::TURNS.load(std::sync::atomic::Ordering::Relaxed);
        let run = self.engine.free_run(workers, parts, began + budget_nanos);
        let done: Vec<_> = self.pool.free(&run, self.clock, shares).collect();
        let parts = run.into_parts();
        let steps = self.engine.end_free(done, parts);
        // Steps after it begin as any would, the workers parked: kept spinning, they make a step's split look free.
        self.pool.park();
        let took = self.clock.nanos() - began;
        let ran = aethereal_substrate::TURNS.load(std::sync::atomic::Ordering::Relaxed) - turns;
        #[allow(clippy::disallowed_methods, reason = "spike probe")]
        let shows = std::env::var("FREE_SHOW").is_ok();
        if shows {
            eprintln!("  free run: {steps} part steps, {ran} turns in {} us", took / 1000);
        }

        Some((ran, took))
    }

    /// Runs steps until no actor has anything to run, then parks every worker.
    pub fn run_until_idle(&mut self) {
        // Spike: the host weighs the two ways of running by what a turn took in each, over windows of time, and runs
        // the cheaper, giving the other one window in every few to measure it again: steps of the whole engine, and
        // parts stepping on their own until the window ends.
        let mut by_step = Rate::default();
        let mut by_free = Rate::default();
        let mut windows = 0_u32;
        while !self.engine.is_idle() {
            let measured = by_step.turn_nanos != 0 && by_free.turn_nanos != 0;
            let favours_free = measured && by_free.turn_nanos < by_step.turn_nanos;
            let probes = windows % PROBE_EVERY == PROBE_EVERY - 1;
            // The first two windows measure each way once: steps first.
            let runs_free = if measured { favours_free != probes } else { by_step.turn_nanos != 0 };
            let runs_free = self.free && runs_free;
            windows += 1;
            if runs_free {
                let Some((turns, nanos)) = self.run_free(WINDOW_NANOS) else {
                    self.free = false;
                    continue;
                };
                by_free.learn(turns, nanos);
            } else {
                let began = self.clock.nanos();
                let turns = aethereal_substrate::TURNS.load(std::sync::atomic::Ordering::Relaxed);
                while self.clock.nanos() - began < WINDOW_NANOS && !self.engine.is_idle() {
                    self.step_sharing();
                }
                let ran = aethereal_substrate::TURNS.load(std::sync::atomic::Ordering::Relaxed) - turns;
                by_step.learn(ran, self.clock.nanos() - began);
            }
            #[allow(clippy::disallowed_methods, reason = "spike probe")]
            let shows = std::env::var("FREE_SHOW").is_ok();
            if shows {
                eprintln!("  window {windows} free={runs_free}: free {} step {} (ns/turn x1024)", by_free.turn_nanos, by_step.turn_nanos);
            }
        }

        self.pool.park();
    }
}

/// Spike: in how many windows the way not favoured runs once.
const PROBE_EVERY: u32 = 16;
/// Spike: how long one window of either way runs.
const WINDOW_NANOS: u64 = 4_000_000;

/// Spike: what turns cost run one way, as a moving average.
#[derive(Default)]
struct Rate {
    /// Nanoseconds a turn took, in fixed point of 1/1024: zero until measured.
    turn_nanos: u64,
    /// Nanoseconds a run took.
    step_nanos: u64,
}

impl Rate {
    fn learn(&mut self, turns: u64, nanos: u64) {
        if turns == 0 {
            return;
        }
        let sample = nanos.saturating_mul(1024) / turns;
        self.turn_nanos = if self.turn_nanos == 0 { sample } else { self.turn_nanos - self.turn_nanos / 4 + sample / 4 };
        self.step_nanos = if self.step_nanos == 0 { nanos } else { self.step_nanos - self.step_nanos / 8 + nanos / 8 };
    }

    /// Whether turns run this way are known to cost less than run `other`'s, or this way is not yet measured.
    fn cheaper_than(&self, other: &Self) -> bool {
        self.turn_nanos == 0 || (other.turn_nanos != 0 && self.turn_nanos < other.turn_nanos)
    }
}

/// How long a step took, in nanoseconds, how long its plan expected its turns to take, and how many workers it was
/// split among: one when it ran whole.
struct Stepped {
    nanos: u64,
    expected_nanos: u64,
    sharing: usize,
}

/// Runs one step of `engine` on `pool`, planned for `workers`, and how long it took, as [`Headless::step`] does.
fn step(engine: &mut Engine, pool: &mut Pool, clock: Monotonic, workers: Workers) -> Stepped {
    let start = clock.nanos();
    let plan = engine.plan(workers);
    let (delta, expected_nanos) = (plan.delta(), plan.expected_nanos());
    let split = plan.into_split();
    let planned = clock.nanos();
    let (ran, sharing) = match split {
        Split::Whole(whole) => {
            let left = pool.alone(clock, delta, || engine.run_whole(whole, &clock));
            let ran = clock.nanos();
            engine.apply_whole(left);

            (ran, 1)
        }
        Split::Shares(shares) => {
            let sharing = shares.len();
            let done = pool.lend(&engine.reading(), clock, delta, shares);
            let ran = clock.nanos();
            let folds = engine.gather(done);
            let folded = pool.fold(&engine.folding(), clock, folds);
            engine.settle(folded);

            (ran, sharing)
        }
    };
    let applied = clock.nanos();
    pool.measure_around(planned.saturating_sub(start), applied.saturating_sub(ran));

    Stepped { nanos: applied.saturating_sub(start), expected_nanos, sharing }
}
