//! A program run through its lifecycle: started, cycled on a timer, and stopped (ADR-0020).

#[cfg(test)]
mod tests;
mod timer;

use crate::Headless;
use aethereal_actor::Time;
use aethereal_substrate::{Clock, Lifecycle};
use core::time::Duration;
use std::sync::Arc;
use std::task::{Wake, Waker};
use std::thread::{self, Thread};
pub use timer::Timer;

/// How a program run to its end ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// `Stop` finished: every turn it set off ran, and every subscriber answered it, let it go, or ended.
    Stopped,
    /// `Stop` had not finished in the time the host gave it once the program was asked to stop: a subscriber still
    /// held it, or its chain still ran.
    OutOfTime,
}

/// How long the host rests between steps with nothing to run.
#[derive(Clone, Copy)]
enum Rest {
    /// Until this time on the host's clock, in nanoseconds, or until it is woken, whichever comes first.
    Until(u64),
    /// Until it is woken: nothing is due.
    Woken,
}

/// What the engine wakes the host by: the thread that runs it, unparked (ADR-0010, ADR-0018).
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
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
    /// time it gives `Stop` has passed. A source pushed, or a job finished, while it sleeps wakes it, and what came in
    /// is brought in at once (ADR-0010, ADR-0018). Cycles are due on a grid of `every` from the start, and one missed
    /// while the program was busy is skipped, not queued: the next `Tick` covers the time. An engine whose cycle holds
    /// no stage has no cycle to wake for, and sleeps until something comes in.
    ///
    /// # Panics
    ///
    /// Panics with a turn's panic, and when a turn makes an actor under an id the table already holds (ADR-0008).
    #[must_use]
    pub fn run(mut self, every: Duration, stop_within: Duration) -> Exit {
        let every = nanos(every);
        let stop_within = nanos(stop_within);
        self.engine.set_wake(Waker::from(Arc::new(Unpark(thread::current()))));
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
            let rest = match stopping {
                Stopping::By(by) => Rest::Until(by),
                Stopping::Not if self.engine.has_stages() => Rest::Until(timer.due()),
                Stopping::Not => Rest::Woken,
            };
            // A wake that came since the last bring has left this thread unparked, so the park returns at once, and so
            // does one a step's own waits woke: either way the loop looks again.
            match rest {
                Rest::Until(at) => thread::park_timeout(Duration::from_nanos(at.saturating_sub(self.clock.nanos()))),
                Rest::Woken => thread::park(),
            }
        }
    }
}

/// `duration` in nanoseconds, or the most a `u64` holds.
fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}
