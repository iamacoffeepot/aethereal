//! When a timer says a cycle is due (ADR-0020).

#[cfg(test)]
mod tests;

/// When cycles are due on a timer of a period: one at its start, then one at each period after, on that grid however
/// late a cycle fires. A cycle missed while the program was busy is skipped, not queued: once one fires late, the next
/// is due at the first point of the grid after it, so a cycle that runs long makes the cycles fewer, never late in a
/// queue (ADR-0020).
pub struct Timer {
    every: u64,
    due: u64,
}

impl Timer {
    /// A timer that says a cycle is due at `start`, and each `every` nanoseconds after: at least one.
    pub fn new(start: u64, every: u64) -> Self {
        Self { every: every.max(1), due: start }
    }

    /// Whether a cycle is due at `now`. When one is, the next is due at the first point of the grid after `now`.
    pub fn fires(&mut self, now: u64) -> bool {
        if now < self.due {
            return false;
        }
        let missed = (now - self.due) / self.every;
        self.due = self.due.saturating_add(self.every.saturating_mul(missed + 1));

        true
    }

    /// When the next cycle is due.
    pub const fn due(&self) -> u64 {
        self.due
    }
}
