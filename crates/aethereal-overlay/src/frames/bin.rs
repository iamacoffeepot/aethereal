//! One column's cycles as they come, and what they come to (ADR-0053: The three stops): the fastest and the slowest
//! exactly, and the median from a sample of at most [`BIN_SAMPLES`] of them.
//!
//! At 60 Hz a column of a chart at 1080 holds about sixteen cycles, which the sample keeps whole. A program that is
//! not held to its display's rate runs many more, so past [`BIN_SAMPLES`] the sample keeps every other cycle it holds
//! and from then takes every other cycle that comes, then every fourth, and so on: an even share of the column's
//! cycles, from its start to its end, never more than [`BIN_SAMPLES`] of them.

use super::Column;
use aethereal_data::Plain;

/// The most cycles a column keeps for its median.
pub const BIN_SAMPLES: usize = 64;

/// The cycles of one column so far: the span of the program's time it covers, the fastest and the slowest of them, how
/// many came, and every `stride`th of them kept, from the first.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    span: u64,
    fastest_nanos: u64,
    slowest_nanos: u64,
    seen: u64,
    stride: u64,
    kept: Vec<u64>,
}

impl Bin {
    /// The column of `span`, its first cycle `delta_nanos` long.
    pub fn first(span: u64, delta_nanos: u64) -> Self {
        let mut kept = Vec::with_capacity(BIN_SAMPLES);
        kept.push(delta_nanos);

        Self { span, fastest_nanos: delta_nanos, slowest_nanos: delta_nanos, seen: 1, stride: 1, kept }
    }

    /// The span of the program's time the column covers.
    pub const fn span(&self) -> u64 {
        self.span
    }

    /// Takes in a cycle `delta_nanos` long: into the fastest and the slowest always, and into the sample when it is a
    /// `stride`th one.
    /// A sample that is full keeps every other cycle it holds first, and takes every other cycle from then.
    pub fn take(&mut self, delta_nanos: u64) {
        self.fastest_nanos = self.fastest_nanos.min(delta_nanos);
        self.slowest_nanos = self.slowest_nanos.max(delta_nanos);
        let index = self.seen;
        self.seen += 1;

        let full = self.kept.len() >= BIN_SAMPLES;
        let thinned = full && index.is_multiple_of(self.stride);
        if thinned {
            let mut position: usize = 0;
            self.kept.retain(|_| {
                let even = position.is_multiple_of(2);
                position += 1;
                even
            });
            self.stride *= 2;
        }
        if index.is_multiple_of(self.stride) {
            self.kept.push(delta_nanos);
        }
    }

    /// The column the cycles so far come to: the fastest and the slowest exactly, and the median by nearest rank among
    /// those kept.
    pub fn column(&self) -> Column {
        let mut sorted = self.kept.clone();
        sorted.sort_unstable();
        let at = sorted.len().div_ceil(2).saturating_sub(1);
        let median_nanos = sorted.get(at).copied().unwrap_or(self.slowest_nanos);

        Column { span: self.span, fastest_nanos: self.fastest_nanos, median_nanos, slowest_nanos: self.slowest_nanos }
    }

    /// How many cycles the sample holds.
    #[cfg(test)]
    pub const fn kept(&self) -> usize {
        self.kept.len()
    }
}
