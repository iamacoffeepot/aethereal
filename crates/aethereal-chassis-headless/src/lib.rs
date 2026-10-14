//! The headless chassis: the native host with nothing but the engine, its workers, and its clock (ADR-0001).
//!
//! The substrate plans each step and runs a share of it wherever it is
//! asked to; this host owns the threads those shares run on and the clock
//! their turns are timed by. Between steps it wakes or parks workers as the
//! plan says (ADR-0011).

mod clock;
mod pool;
#[cfg(test)]
mod tests;

pub use clock::Monotonic;

use aethereal_substrate::Engine;
pub use pool::Measured;

use pool::Pool;
use std::num::NonZeroUsize;

/// An engine run on native workers.
pub struct Headless {
    engine: Engine,
    pool: Pool,
    clock: Monotonic,
}

impl Headless {
    /// `engine` run on at most `workers` workers, the calling thread among them.
    ///
    /// Every worker but the calling thread starts parked, and the cost of waking one is measured here.
    #[must_use]
    pub fn new(engine: Engine, workers: NonZeroUsize) -> Self {
        let clock = Monotonic::new();
        let pool = Pool::new(workers, &clock);

        Self { engine, pool, clock }
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

    /// Runs one step: plans it for the workers there are, runs its shares on them, and applies what they left.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    pub fn step(&mut self) {
        let shares = self.engine.plan(self.pool.workers()).into_shares();
        let done = self.pool.lend(&self.engine.reading(), self.clock, shares);

        self.engine.apply(done);
    }

    /// Runs steps until no actor has anything to run, then parks every worker.
    pub fn run_until_idle(&mut self) {
        while !self.engine.is_idle() {
            self.step();
        }

        self.pool.park();
    }
}
