//! The native clock, handed to the engine (ADR-0001).

use aethereal_substrate::Clock;
use std::time::Instant;

/// The native monotonic clock, counted from when it was made.
#[derive(Debug, Clone, Copy)]
pub struct Monotonic {
    start: Instant,
}

impl Monotonic {
    /// A clock that reads zero now.
    #[must_use]
    pub fn new() -> Self {
        #[allow(clippy::disallowed_methods)] // The native host owns the real clock (ADR-0001).
        let start = Instant::now();

        Self { start }
    }
}

impl Default for Monotonic {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for Monotonic {
    fn nanos(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}
