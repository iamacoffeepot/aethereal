//! One column's cycles as they come, and what they come to (ADR-0053: The three stops): the fastest and the slowest
//! exactly, and the median from a bounded, even sample of them (`binned`'s `Thinned`).

use super::Column;
use crate::binned::{Thinned, median};
use aethereal_data::Plain;

#[cfg(test)]
pub use crate::binned::MOST_KEPT as BIN_SAMPLES;

/// The cycles of one column so far: the span of the program's time it covers, the fastest and the slowest of them, and
/// an even sample of them.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Bin {
    span: u64,
    fastest_nanos: u64,
    slowest_nanos: u64,
    sample: Thinned<u64>,
}

impl Bin {
    /// The column of `span`, its first cycle `delta_nanos` long.
    pub fn first(span: u64, delta_nanos: u64) -> Self {
        Self { span, fastest_nanos: delta_nanos, slowest_nanos: delta_nanos, sample: Thinned::first(delta_nanos) }
    }

    /// The span of the program's time the column covers.
    pub const fn span(&self) -> u64 {
        self.span
    }

    /// Takes in a cycle `delta_nanos` long: into the fastest and the slowest always, and into the sample as it thins.
    pub fn take(&mut self, delta_nanos: u64) {
        self.fastest_nanos = self.fastest_nanos.min(delta_nanos);
        self.slowest_nanos = self.slowest_nanos.max(delta_nanos);
        self.sample.take(delta_nanos);
    }

    /// The column the cycles so far come to: the fastest and the slowest exactly, and the median by nearest rank among
    /// those kept.
    pub fn column(&self) -> Column {
        let median_nanos = median(self.sample.kept().to_vec());

        Column { span: self.span, fastest_nanos: self.fastest_nanos, median_nanos, slowest_nanos: self.slowest_nanos }
    }

    /// How many cycles the sample holds.
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.sample.kept().len()
    }
}
