//! The native clock, handed to the engine (ADR-0001).

use aethereal_substrate::Clock;
use std::sync::OnceLock;
use std::time::Instant;

/// The moment every native clock of the process counts from: when the first was made.
static FIRST: OnceLock<Instant> = OnceLock::new();

/// The native monotonic clock, counted from when the process's first was made, so every one of them reads the same
/// time: the clock a step's time is read from and the one the jobs it runs wait on agree (ADR-0018).
#[derive(Debug, Clone, Copy)]
pub struct Monotonic {
    start: Instant,
}

impl Monotonic {
    /// The clock, which reads zero when the process's first is made.
    #[must_use]
    pub fn new() -> Self {
        #[allow(clippy::disallowed_methods)] // The native host owns the real clock (ADR-0001).
        let start = *FIRST.get_or_init(Instant::now);

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
