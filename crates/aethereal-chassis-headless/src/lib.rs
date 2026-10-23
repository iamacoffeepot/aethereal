//! The headless chassis: the native host with nothing but the engine, its workers, and its clock (ADR-0001).
//!
//! It is a program's [`Host`](aethereal_substrate::Host) (ADR-0029): `Headless::program()`, its roots listed, then
//! `run`, or `build` for a test to step. Its command line, environment, and a file are read into a program by
//! [`native`] (ADR-0028), which the desktop host shares.
//!
//! The substrate plans each step and runs a share of it wherever it is
//! asked to; this host owns the threads those shares run on and the clock
//! their turns are timed by. Between steps it wakes or parks workers as the
//! plan says (ADR-0011).

mod clock;
mod files;
#[cfg(not(target_arch = "wasm32"))]
pub mod guest;
mod jobs;
pub mod native;
mod pool;
mod program;
mod rehearsal;
mod running;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod waiting;

pub use clock::Monotonic;
pub use files::{Directories, FileRoots};
pub use jobs::Runners;

use aethereal_actor::Time;
use aethereal_substrate::{Clock, Engine, Split, SplitCost, Workers};
pub use pool::Measured;
pub use program::{RunningSettings, Serving};
pub use running::{Exit, Timer};

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

        Self { engine, pool, clock, split }
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
        let workers = self.pool.workers(SplitCost::new(&self.split));
        step(&mut self.engine, &mut self.pool, self.clock, workers);
    }

    /// Runs steps until no actor has anything to run, then parks every worker.
    pub fn run_until_idle(&mut self) {
        while !self.engine.is_idle() {
            self.step();
        }

        self.pool.park();
    }

    /// Parks every worker: for a host that steps the engine itself, as it waits on something else between steps.
    pub fn park(&mut self) {
        self.pool.park();
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
    let plan = engine.plan(workers, Time::from_nanos(start));
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
