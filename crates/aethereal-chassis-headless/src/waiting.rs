//! Waiting, in tests, on what the host's own threads do: the one place a test waits on real time.

use std::thread;
use std::time::{Duration, Instant};

/// How long a test waits on the host's threads: far past what a healthy run takes on a loaded machine, so only a run
/// that never gets there spends it.
pub const BUDGET: Duration = Duration::from_secs(30);

/// Asks `done` until it answers `true`, or until `budget` of real time has passed. The test then asserts on what it
/// saw, so a run that never got there fails naming where it stood.
pub fn until(budget: Duration, mut done: impl FnMut() -> bool) {
    #[allow(clippy::disallowed_methods)] // A test waits on the host's real threads, which run on real time.
    let began = Instant::now();
    loop {
        let is_done = done();
        let is_spent = began.elapsed() >= budget;
        if is_done || is_spent {
            return;
        }
        thread::yield_now();
    }
}
