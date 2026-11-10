//! A bounded, even sample of a column's values as they come, for its median (ADR-0053: The three stops).
//!
//! At 60 Hz a column of a chart at 1080 holds about sixteen frames, which the sample keeps whole. A program that is
//! not held to its display's rate runs many more, so past [`MOST_KEPT`] the sample keeps every other value it holds
//! and from then takes every other value that comes, then every fourth, and so on: an even share of the column's
//! values, from its start to its end, never more than [`MOST_KEPT`] of them.

use aethereal_data::Plain;

/// The most values a column keeps for its median.
pub const MOST_KEPT: usize = 64;

/// How many values came, and every `stride`th of them kept, from the first.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Thinned<T> {
    count: Count,
    kept: Vec<T>,
}

/// How many values came, and how many apart those kept are.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
struct Count {
    seen: u64,
    stride: u64,
}

impl<T: Copy> Thinned<T> {
    /// The sample whose first value is `value`.
    pub fn first(value: T) -> Self {
        let mut kept = Vec::with_capacity(MOST_KEPT);
        kept.push(value);

        Self { count: Count { seen: 1, stride: 1 }, kept }
    }

    /// Takes in `value` when it is a `stride`th one. A sample that is full keeps every other value it holds first,
    /// and takes every other value from then.
    pub fn take(&mut self, value: T) {
        let index = self.count.seen;
        self.count.seen += 1;

        let full = self.kept.len() >= MOST_KEPT;
        let thinned = full && index.is_multiple_of(self.count.stride);
        if thinned {
            let mut position: usize = 0;
            self.kept.retain(|_| {
                let even = position.is_multiple_of(2);
                position += 1;
                even
            });
            self.count.stride *= 2;
        }
        if index.is_multiple_of(self.count.stride) {
            self.kept.push(value);
        }
    }

    /// The values kept.
    pub fn kept(&self) -> &[T] {
        &self.kept
    }
}
