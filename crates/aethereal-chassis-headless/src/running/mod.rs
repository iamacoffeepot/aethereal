//! A program run through its lifecycle: started, cycled on a timer, and stopped (ADR-0020).

#[cfg(test)]
mod tests;
mod timer;

use crate::Headless;
use aethereal_actor::Time;
use aethereal_substrate::{Clock, Lifecycle};
use core::time::Duration;
use std::thread;
use timer::Timer;

/// How a program run to its end ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// `Stop` finished: every turn it set off ran, and every subscriber answered it, let it go, or ended.
    Stopped,
    /// `Stop` had not finished in the time the host gave it once the program was asked to stop: a subscriber still
    /// held it, or its chain still ran.
    OutOfTime,
}

/// Whether the program has been asked to stop, as the host keeps it.
#[derive(Clone, Copy)]
enum Stopping {
    /// It has not.
    Not,
    /// It has, and `Stop` must finish by this time on the host's clock, in nanoseconds.
    By(u64),
}

impl Headless {
    /// Runs the program through its lifecycle, then drops it (ADR-0020): starts it, says a cycle is due every
    /// `every`, brings in what its sources were pushed and its jobs finished before each step (ADR-0010, ADR-0018),
    /// steps while any actor has anything to run, and once it is asked to stop, by an actor's
    /// [`quit`](aethereal_actor::Turn::quit), waits at most `stop_within` for `Stop` to finish.
    ///
    /// Between steps with nothing to run it parks its workers and sleeps until the next cycle is due, or until the
    /// time it gives `Stop` has passed. What is pushed, or finishes, while it sleeps is brought in when it wakes, so
    /// it waits at most `every`. Cycles are due on a grid of `every` from the start, and one missed while the program
    /// was busy is skipped, not queued: the next `Tick` covers the time. An engine whose cycle holds no stage still
    /// wakes every `every`, and does nothing.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    #[must_use]
    pub fn run(mut self, every: Duration, stop_within: Duration) -> Exit {
        let every = nanos(every);
        let stop_within = nanos(stop_within);
        self.engine.start();
        let mut timer = Timer::new(self.clock.nanos(), every);
        let mut stopping = Stopping::Not;
        loop {
            let now = self.clock.nanos();
            match (self.engine.lifecycle(), stopping) {
                (Lifecycle::Stopped, _) => break Exit::Stopped,
                (Lifecycle::Stopping, Stopping::Not) => stopping = Stopping::By(now.saturating_add(stop_within)),
                (Lifecycle::Stopping, Stopping::By(by)) if now >= by => break Exit::OutOfTime,
                (Lifecycle::Waiting | Lifecycle::Running | Lifecycle::Stopping, _) => {}
            }
            if timer.fires(now) {
                self.engine.cycle(Time::from_nanos(now));
            }
            self.engine.bring();
            if !self.engine.is_idle() {
                self.step();
                continue;
            }
            self.pool.park();
            let wake = match stopping {
                Stopping::Not => timer.due(),
                Stopping::By(by) => by,
            };
            let rest = Duration::from_nanos(wake.saturating_sub(self.clock.nanos()));
            // The native host owns its thread, and waits on its own timer between steps (ADR-0001).
            #[allow(clippy::disallowed_methods)]
            thread::sleep(rest);
        }
    }
}

/// `duration` in nanoseconds, or the most a `u64` holds.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
